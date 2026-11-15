//! Titan RISC-V machine session driver with UART prompt synchronization and rewind.
//!
//! Rewind works on instruction counts. Every checkpoint is a page delta against one full snapshot
//! taken at boot, so each restore is the chain `[base, delta]`. The base is never thinned, which
//! keeps every remaining delta restorable. Inputs are logged with the `instret` they arrived at.

use chisel::elf;
use hades::{mkfs, Fs, MemDisk, MkfsOptions};
use silicon::machine::{Config, Exit, Machine, Snapshot};
use silicon::probe::Probe;
use silicon::Bus;

const DISK_BLOCKS: u64 = 256;
const RAM_SIZE: usize = 32 * 1024 * 1024;
const UART_LSR_ADDR: u64 = 0x1000_0005;
/// Instructions between rewind checkpoints.
const CHECKPOINT_INTERVAL: u64 = 2_000_000;
/// Bytes the rewind checkpoints may hold in total, boot base included.
const CHECKPOINT_CAP: usize = 48 * 1024 * 1024;

/// Outcome of advancing a [`Titan`] session with [`Titan::pump`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pump {
    /// Guest printed the `$ ` prompt and has no queued UART input.
    WaitingForInput,
    /// Step budget was reached before hitting a prompt or exiting.
    Busy,
    /// Guest wrote to the test finisher and exited with this status code (`0` = pass).
    Exited(i32),
}

/// Host-side counters that `pump` reads. Saved with each checkpoint so a seek restores them too.
#[derive(Debug, Clone, Copy)]
struct HostMark {
    total_tx: u64,
    tail: [u8; 2],
    rx_was_nonempty: bool,
    min_tx_for_prompt: u64,
    exited: Option<i32>,
    /// Number of input-log entries that had reached the machine.
    log_pos: usize,
}

/// A saved machine state: the full boot base, or a page delta against it.
struct Checkpoint {
    snap: Snapshot,
    host: HostMark,
}

/// Interactive Titan VM session running on Silicon with a formatted Hades disk.
pub struct Titan {
    machine: Machine,
    output: Vec<u8>,
    tail: [u8; 2],
    total_tx: u64,
    min_tx_for_prompt: u64,
    rx_was_nonempty: bool,
    exited: Option<i32>,
    /// Full snapshot taken at boot. Every delta is taken against it.
    base: Checkpoint,
    /// Deltas in ascending instret order.
    deltas: Vec<Checkpoint>,
    /// Inputs in instret order. The first `log_pos` have reached the machine.
    log: Vec<(u64, String)>,
    log_pos: usize,
    interval: u64,
    cap: usize,
    /// Instret at which the next checkpoint is due.
    next_at: u64,
}

impl Titan {
    /// Boot a Titan kernel ELF image with 32 MiB RAM and a Hades shell disk.
    pub fn boot(elf_bytes: &[u8]) -> Result<Titan, String> {
        Self::boot_with_disk(elf_bytes, default_disk()?)
    }

    /// Boot a Titan kernel ELF image with 32 MiB RAM and the given Hades disk image.
    pub fn boot_with_disk(elf_bytes: &[u8], disk: Vec<u8>) -> Result<Titan, String> {
        validate_disk(&disk)?;
        let image = elf::read(elf_bytes).map_err(|e| format!("elf read failed: {e}"))?;
        let cfg = Config {
            ram_size: RAM_SIZE,
            disk,
            decode_cache: true,
            ..Config::default()
        };
        let mut machine = Machine::new(cfg);
        elf::load(&image, &mut machine).map_err(|e| format!("elf load failed: {e:?}"))?;
        machine.cpu.x[10] = 0;
        machine.cpu.x[11] = RAM_SIZE as u64;

        let base = Checkpoint {
            snap: machine.snapshot(None),
            host: HostMark {
                total_tx: 0,
                tail: [0, 0],
                rx_was_nonempty: false,
                min_tx_for_prompt: 2,
                exited: None,
                log_pos: 0,
            },
        };
        Ok(Titan {
            machine,
            output: Vec::new(),
            tail: [0, 0],
            total_tx: 0,
            min_tx_for_prompt: 2,
            rx_was_nonempty: false,
            exited: None,
            base,
            deltas: Vec::new(),
            log: Vec::new(),
            log_pos: 0,
            interval: CHECKPOINT_INTERVAL,
            cap: CHECKPOINT_CAP,
            next_at: CHECKPOINT_INTERVAL,
        })
    }

    /// Return a copy of the guest disk image after any writes performed so far.
    pub fn disk_bytes(&self) -> Vec<u8> {
        self.machine.board.disk().to_vec()
    }

    /// Queue `line` followed by a newline on the guest UART RX FIFO, at the current instret.
    /// Typing after a seek first drops the logged future: inputs not yet replayed, and checkpoints
    /// taken after the current instret.
    pub fn send(&mut self, line: &str) {
        let now = self.instret();
        self.log.truncate(self.log_pos);
        self.log.push((now, line.to_owned()));
        self.log_pos = self.log.len();
        self.deltas.retain(|c| c.snap.cpu.instret <= now);
        self.next_at = next_after(self.last_instret(), self.interval);
        self.apply(line);
    }

    /// Run up to `max_steps` instructions until the `$ ` prompt waits, the guest exits, or budget runs out.
    pub fn pump(&mut self, max_steps: u64) -> Pump {
        if let Some(code) = self.exited {
            return Pump::Exited(code);
        }
        self.inject_due();
        if !self.uart_rx_nonempty() && self.at_prompt() && !self.pending() {
            return Pump::WaitingForInput;
        }

        for _ in 0..max_steps {
            if let Err(code) = self.advance() {
                return Pump::Exited(code);
            }
            if !self.uart_rx_nonempty() && self.at_prompt() && !self.pending() {
                return Pump::WaitingForInput;
            }
        }

        Pump::Busy
    }

    /// Drain and return all UART output bytes emitted since the previous call.
    pub fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.output)
    }

    /// Move the session to `instret`: restore the nearest checkpoint at or before it, replay the
    /// logged input up to it, and drop output not yet taken. Fails if the guest exits, or is
    /// waiting for input with nothing logged, before reaching `instret`; the session then stays
    /// where it stopped.
    pub fn seek(&mut self, instret: u64) -> Result<(), String> {
        let found = self
            .deltas
            .partition_point(|c| c.snap.cpu.instret <= instret);
        let (chain, host): (Vec<&Snapshot>, HostMark) = match found.checked_sub(1) {
            Some(i) => match self.deltas.get(i) {
                Some(delta) => (vec![&self.base.snap, &delta.snap], delta.host),
                None => (vec![&self.base.snap], self.base.host),
            },
            None => (vec![&self.base.snap], self.base.host),
        };
        self.machine
            .restore(&chain)
            .map_err(|e| format!("rewind restore failed: {e:?}"))?;
        self.set_host(host);
        self.output.clear();
        self.next_at = next_after(self.last_instret(), self.interval);
        self.inject_due();
        while self.instret() < instret {
            if let Err(code) = self.advance() {
                return Err(format!("guest exited with {code} before instret {instret}"));
            }
            // Idle at the prompt with no pending input: no later instret can be reached.
            if !self.pending() && self.at_prompt() && !self.uart_rx_nonempty() {
                return Err(format!(
                    "guest waits for input at instret {} before {instret}",
                    self.instret()
                ));
            }
        }
        self.inject_due();
        self.output.clear();
        Ok(())
    }

    /// Instructions retired by the hart so far.
    pub fn instret(&self) -> u64 {
        self.machine.cpu.instret
    }

    /// Trap, ECALL and device counters since boot. Device counts rewind with `seek`; trap and
    /// ECALL counts do not, because checkpoints do not store them.
    pub fn probe(&self) -> Probe {
        self.machine.probe()
    }

    /// The counters as `name value` lines, one per counter, sorted by name. Used by `titan_probe`.
    pub fn probe_text(&self) -> String {
        let p = self.probe();
        let mut rows: Vec<(String, u64)> = vec![
            ("block.reads".to_owned(), p.block_reads),
            ("block.writes".to_owned(), p.block_writes),
            ("ecall.count".to_owned(), p.ecall_total()),
        ];
        rows.extend(
            p.exceptions
                .iter()
                .enumerate()
                .map(|(i, &n)| (format!("exception.{i:02}"), n)),
        );
        rows.extend(
            p.interrupts
                .iter()
                .enumerate()
                .map(|(i, &n)| (format!("interrupt.{i:02}"), n)),
        );
        rows.push(("uart.in".to_owned(), p.uart_in));
        rows.push(("uart.out".to_owned(), p.uart_out));
        rows.iter()
            .map(|(name, n)| format!("{name} {n}\n"))
            .collect()
    }

    /// Number of stored checkpoints, the boot base included.
    pub fn checkpoint_count(&self) -> usize {
        self.deltas.len() + 1
    }

    /// Bytes held by the stored checkpoints, as `Snapshot::bytes` counts them.
    pub fn checkpoint_bytes(&self) -> usize {
        self.deltas.iter().fold(self.base.snap.bytes(), |acc, c| {
            acc.saturating_add(c.snap.bytes())
        })
    }

    /// Entries in the input log, including inputs a seek has not replayed yet.
    pub fn input_log_len(&self) -> usize {
        self.log.len()
    }

    /// Set the checkpoint interval (instructions) and byte cap, then thin to the new cap.
    /// Tests use this to make checkpoints cheap; the defaults are used otherwise.
    pub fn set_rewind_limits(&mut self, interval: u64, cap_bytes: usize) {
        self.interval = interval.max(1);
        self.cap = cap_bytes;
        self.next_at = next_after(self.last_instret(), self.interval);
        self.thin();
    }

    fn at_prompt(&self) -> bool {
        !self.rx_was_nonempty && self.total_tx >= self.min_tx_for_prompt && self.tail == *b"$ "
    }

    fn pending(&self) -> bool {
        self.log_pos < self.log.len()
    }

    fn uart_rx_nonempty(&mut self) -> bool {
        self.machine
            .board
            .read(UART_LSR_ADDR, 1)
            .is_ok_and(|lsr| (lsr & 1) != 0)
    }

    /// Hand a line and its newline to the UART RX FIFO and update the prompt bookkeeping.
    fn apply(&mut self, line: &str) {
        self.machine.board.uart_input(line.as_bytes());
        self.machine.board.uart_input(b"\n");
        self.rx_was_nonempty = true;
        self.min_tx_for_prompt = self.total_tx.saturating_add(2);
    }

    /// Feed every logged input that is due at the current instret to the machine.
    fn inject_due(&mut self) {
        let now = self.instret();
        while let Some((at, line)) = self.log.get(self.log_pos) {
            if *at > now {
                break;
            }
            let line = line.clone();
            self.apply(&line);
            self.log_pos += 1;
        }
    }

    /// Execute one instruction and do the same host bookkeeping `pump` always did, then inject
    /// due inputs and take a checkpoint if one is due. `Err` carries the guest exit status.
    fn advance(&mut self) -> Result<(), i32> {
        if self.uart_rx_nonempty() {
            self.rx_was_nonempty = true;
        }

        let exit = self.machine.run(1);
        let chunk = self.machine.board.take_uart_output();
        if !chunk.is_empty() {
            self.push_output(&chunk);
        }

        match exit {
            Exit::Pass => {
                self.exited = Some(0);
                return Err(0);
            }
            Exit::Fail(code) => {
                let status = i32::try_from(code).unwrap_or(i32::MAX);
                self.exited = Some(status);
                return Err(status);
            }
            Exit::OutOfSteps => {}
        }

        let rx_nonempty = self.uart_rx_nonempty();
        if self.rx_was_nonempty && !rx_nonempty {
            self.rx_was_nonempty = false;
            self.min_tx_for_prompt = self.total_tx.saturating_add(2);
        }

        self.inject_due();
        if self.instret() >= self.next_at {
            self.checkpoint();
        }
        Ok(())
    }

    /// Store a page delta against the boot base, then thin to the cap.
    fn checkpoint(&mut self) {
        let snap = self.machine.snapshot(Some(&self.base.snap));
        let host = self.mark();
        self.deltas.push(Checkpoint { snap, host });
        self.next_at = next_after(self.instret(), self.interval);
        self.thin();
    }

    /// Drop the oldest deltas until the stored bytes fit the cap. The base stays, so every
    /// remaining delta still restores as `[base, delta]`.
    fn thin(&mut self) {
        let mut total = self.checkpoint_bytes();
        while total > self.cap && !self.deltas.is_empty() {
            let gone = self.deltas.remove(0);
            total = total.saturating_sub(gone.snap.bytes());
        }
    }

    /// Instret of the newest checkpoint.
    fn last_instret(&self) -> u64 {
        self.deltas
            .last()
            .map_or(self.base.snap.cpu.instret, |c| c.snap.cpu.instret)
    }

    fn mark(&self) -> HostMark {
        HostMark {
            total_tx: self.total_tx,
            tail: self.tail,
            rx_was_nonempty: self.rx_was_nonempty,
            min_tx_for_prompt: self.min_tx_for_prompt,
            exited: self.exited,
            log_pos: self.log_pos,
        }
    }

    fn set_host(&mut self, m: HostMark) {
        self.total_tx = m.total_tx;
        self.tail = m.tail;
        self.rx_was_nonempty = m.rx_was_nonempty;
        self.min_tx_for_prompt = m.min_tx_for_prompt;
        self.exited = m.exited;
        self.log_pos = m.log_pos;
    }

    fn push_output(&mut self, bytes: &[u8]) {
        self.output.extend_from_slice(bytes);
        self.total_tx = self.total_tx.saturating_add(bytes.len() as u64);
        for &b in bytes {
            self.tail[0] = self.tail[1];
            self.tail[1] = b;
        }
    }
}

/// The first checkpoint instret after `instret` on an `interval` grid.
fn next_after(instret: u64, interval: u64) -> u64 {
    (instret / interval)
        .saturating_add(1)
        .saturating_mul(interval)
}

/// Build the default formatted Hades disk image containing `/hello.txt` and `/etc/shell`.
pub fn default_disk() -> Result<Vec<u8>, String> {
    let mut disk = MemDisk::new(DISK_BLOCKS);
    mkfs(&mut disk, MkfsOptions::default()).map_err(|e| format!("mkfs failed: {e:?}"))?;
    let mut fs = Fs::mount(disk).map_err(|e| format!("mount failed: {e:?}"))?;
    fs.create("/hello.txt")
        .map_err(|e| format!("create /hello.txt failed: {e:?}"))?;
    let written = fs
        .write("/hello.txt", 0, b"hello")
        .map_err(|e| format!("write /hello.txt failed: {e:?}"))?;
    if written != 5 {
        return Err(String::from("short write to /hello.txt"));
    }
    fs.mkdir("/etc")
        .map_err(|e| format!("mkdir /etc failed: {e:?}"))?;
    fs.create("/etc/shell")
        .map_err(|e| format!("create /etc/shell failed: {e:?}"))?;
    let disk = fs.unmount().map_err(|e| format!("unmount failed: {e:?}"))?;
    Ok(disk.bytes().to_vec())
}

fn validate_disk(disk: &[u8]) -> Result<(), String> {
    if disk.is_empty() {
        return Err(String::from("disk image is empty"));
    }
    if !disk.len().is_multiple_of(hades::BLOCK) {
        return Err(format!(
            "disk image size ({} bytes) is not a multiple of {}-byte block",
            disk.len(),
            hades::BLOCK
        ));
    }
    let blocks = u64::try_from(disk.len() / hades::BLOCK)
        .map_err(|_| String::from("disk image block count overflow"))?;
    let mut mem = MemDisk::new(blocks);
    mem.bytes_mut().copy_from_slice(disk);
    let mut fs = Fs::mount(mem).map_err(|e| format!("disk mount failed: {e:?}"))?;
    let stat = fs
        .stat("/etc/shell")
        .map_err(|e| format!("disk missing /etc/shell: {e:?}"))?;
    if stat.kind != hades::Kind::File {
        return Err(String::from("disk /etc/shell is not a regular file"));
    }
    let _ = fs.unmount();
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn boot_rejects_empty_and_garbage_elf() {
        assert!(Titan::boot(&[]).is_err());
        assert!(Titan::boot(b"not an elf").is_err());
    }

    #[test]
    fn default_disk_mounts_and_has_expected_files() {
        let bytes = default_disk().unwrap();
        let blocks = u64::try_from(bytes.len() / 4096).unwrap();
        let mut disk = MemDisk::new(blocks);
        disk.bytes_mut().copy_from_slice(&bytes);
        let mut fs = Fs::mount(disk).unwrap();
        assert!(fs.stat("/etc/shell").is_ok());
        let mut buf = [0u8; 8];
        let n = fs.read("/hello.txt", 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello");
    }

    #[test]
    fn boot_with_disk_rejects_invalid_images() {
        assert!(Titan::boot_with_disk(&[], Vec::new()).is_err());
        assert!(Titan::boot_with_disk(&[], vec![0u8; 512]).is_err());
        assert!(Titan::boot_with_disk(&[], vec![0u8; 4097]).is_err());
        assert!(Titan::boot_with_disk(&[], vec![0u8; 256 * hades::BLOCK]).is_err());

        let mut no_shell = MemDisk::new(DISK_BLOCKS);
        mkfs(&mut no_shell, MkfsOptions::default()).unwrap();
        assert!(Titan::boot_with_disk(&[], no_shell.bytes().to_vec()).is_err());

        let mut dir_shell = MemDisk::new(DISK_BLOCKS);
        mkfs(&mut dir_shell, MkfsOptions::default()).unwrap();
        let mut fs = Fs::mount(dir_shell).unwrap();
        fs.mkdir("/etc").unwrap();
        fs.mkdir("/etc/shell").unwrap();
        let dir_shell = fs.unmount().unwrap();
        assert!(Titan::boot_with_disk(&[], dir_shell.bytes().to_vec()).is_err());
    }

    #[test]
    fn next_after_moves_to_the_following_grid_point() {
        assert_eq!(next_after(0, 10), 10);
        assert_eq!(next_after(9, 10), 10);
        assert_eq!(next_after(10, 10), 20);
        assert_eq!(next_after(u64::MAX, 10), u64::MAX);
    }
}
