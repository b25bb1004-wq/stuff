//! Titan RISC-V machine session driver with UART prompt synchronization.

use chisel::elf;
use hades::{mkfs, Fs, MemDisk, MkfsOptions};
use silicon::machine::{Config, Exit, Machine};
use silicon::Bus;

const DISK_BLOCKS: u64 = 256;
const RAM_SIZE: usize = 32 * 1024 * 1024;
const UART_LSR_ADDR: u64 = 0x1000_0005;

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

/// Interactive Titan VM session running on Silicon with a formatted Hades disk.
pub struct Titan {
    machine: Machine,
    output: Vec<u8>,
    tail: [u8; 2],
    total_tx: u64,
    min_tx_for_prompt: u64,
    rx_was_nonempty: bool,
    exited: Option<i32>,
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

        Ok(Titan {
            machine,
            output: Vec::new(),
            tail: [0, 0],
            total_tx: 0,
            min_tx_for_prompt: 2,
            rx_was_nonempty: false,
            exited: None,
        })
    }

    /// Return a copy of the guest disk image after any writes performed so far.
    pub fn disk_bytes(&self) -> Vec<u8> {
        self.machine.board.disk().to_vec()
    }

    /// Queue `line` followed by a newline on the guest UART RX FIFO.
    pub fn send(&mut self, line: &str) {
        self.machine.board.uart_input(line.as_bytes());
        self.machine.board.uart_input(b"\n");
        self.rx_was_nonempty = true;
        self.min_tx_for_prompt = self.total_tx.saturating_add(2);
    }

    /// Run up to `max_steps` instructions until the `$ ` prompt waits, the guest exits, or budget runs out.
    pub fn pump(&mut self, max_steps: u64) -> Pump {
        if let Some(code) = self.exited {
            return Pump::Exited(code);
        }
        if !self.uart_rx_nonempty() && self.at_prompt() {
            return Pump::WaitingForInput;
        }

        for _ in 0..max_steps {
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
                    return Pump::Exited(0);
                }
                Exit::Fail(code) => {
                    let status = i32::try_from(code).unwrap_or(i32::MAX);
                    self.exited = Some(status);
                    return Pump::Exited(status);
                }
                Exit::OutOfSteps => {}
            }

            let rx_nonempty = self.uart_rx_nonempty();
            if self.rx_was_nonempty && !rx_nonempty {
                self.rx_was_nonempty = false;
                self.min_tx_for_prompt = self.total_tx.saturating_add(2);
            }

            if !rx_nonempty && self.at_prompt() {
                return Pump::WaitingForInput;
            }
        }

        Pump::Busy
    }

    /// Drain and return all UART output bytes emitted since the previous call.
    pub fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.output)
    }

    fn at_prompt(&self) -> bool {
        !self.rx_was_nonempty && self.total_tx >= self.min_tx_for_prompt && self.tail == *b"$ "
    }

    fn uart_rx_nonempty(&mut self) -> bool {
        self.machine
            .board
            .read(UART_LSR_ADDR, 1)
            .is_ok_and(|lsr| (lsr & 1) != 0)
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
}
