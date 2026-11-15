//! Whole-machine snapshots. Hart and device state is always stored in full; RAM, disk and the
//! framebuffer are stored as 4 KiB pages, and only non-zero pages appear in a full snapshot.
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::{Exit, Machine};

/// Source of snapshot ids; ids are unique within the process.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
use crate::csr::Csrs;
use crate::Mode;

/// Bytes per memory page in a snapshot.
pub const PAGE_SIZE: usize = 4096;

/// Fixed overhead counted by `Snapshot::bytes` for registers and small device fields.
const FIXED_STATE_BYTES: usize = 8 * 32 + 256;

/// Memory that snapshots page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Region {
    /// Guest RAM, mapped at `RAM_BASE`.
    Ram,
    /// Block device disk image.
    Disk,
    /// Display framebuffer.
    Framebuffer,
}

/// One stored page of memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Memory the page belongs to.
    pub region: Region,
    /// Page number inside the region; the page starts at byte `index * PAGE_SIZE`.
    pub index: u64,
    /// Page contents: `PAGE_SIZE` bytes, fewer only at the end of a region.
    pub data: Vec<u8>,
}

/// Hart state: registers, pc, privilege, CSRs, reservation, instret and cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuState {
    /// Integer registers x0 to x31.
    pub x: [u64; 32],
    /// Program counter.
    pub pc: u64,
    /// Instructions retired (minstret).
    pub instret: u64,
    /// Privilege mode.
    pub mode: Mode,
    /// Virtualization bit.
    pub virt: bool,
    /// Load-reservation address, if one is held.
    pub reservation: Option<u64>,
    /// Cycle count, stored as the CLINT `mtime` value.
    pub cycle: u64,
    /// CSR storage, in the fixed order used by `csr_words`.
    pub csrs: Vec<u64>,
}

/// Device registers, FIFOs and interrupt-controller state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceState {
    /// UART receive FIFO.
    pub uart_rx: Vec<u8>,
    /// UART bytes written by the guest and not yet drained by the host.
    pub uart_tx: Vec<u8>,
    /// UART interrupt-enable register.
    pub uart_ier: u8,
    /// UART line-control register.
    pub uart_lcr: u8,
    /// UART modem-control register.
    pub uart_mcr: u8,
    /// UART scratch register.
    pub uart_scr: u8,
    /// Bytes the host queued into the UART.
    pub uart_in_bytes: u64,
    /// Bytes the guest wrote to the UART.
    pub uart_out_bytes: u64,
    /// Block device sector register.
    pub block_sector: u64,
    /// Block device DMA address register.
    pub block_dma: u64,
    /// Block device sector count register.
    pub block_count: u32,
    /// Block device status register.
    pub block_status: u32,
    /// Block device interrupt-enable register.
    pub block_irq_enable: u32,
    /// Block device done flag.
    pub block_done: bool,
    /// Accepted block reads since the last probe reset.
    pub block_reads: u64,
    /// Accepted block writes since the last probe reset.
    pub block_writes: u64,
    /// CLINT machine software-interrupt bit.
    pub clint_msip: bool,
    /// CLINT `mtimecmp` register.
    pub clint_mtimecmp: u64,
    /// PLIC source priorities, one byte per source.
    pub plic_priority: [u8; 32],
    /// PLIC pending bits.
    pub plic_pending: u32,
    /// PLIC sources claimed but not yet completed.
    pub plic_in_flight: u32,
    /// PLIC enable bits per context (0 = M, 1 = S).
    pub plic_enable: [u32; 2],
    /// PLIC priority thresholds per context.
    pub plic_threshold: [u32; 2],
    /// Key FIFO contents, oldest first.
    pub keys_fifo: Vec<u32>,
    /// Key device interrupt-enable register.
    pub keys_irq_enable: u32,
    /// Test-finisher exit status, if the guest has written one.
    pub exit: Option<Exit>,
}

/// Machine state, complete when `full` is set, otherwise the pages changed since a full base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Unique id of this snapshot within the process.
    pub id: u64,
    /// For a delta, the id of the full snapshot it was taken against; `None` for a full one.
    pub base_id: Option<u64>,
    /// True when the snapshot needs no base: its pages are every non-zero page.
    pub full: bool,
    /// Guest RAM size in bytes.
    pub ram_size: u64,
    /// Disk image size in bytes.
    pub disk_size: u64,
    /// Framebuffer size in bytes.
    pub fb_size: u64,
    /// Hart state, always complete.
    pub cpu: CpuState,
    /// Device state, always complete.
    pub devices: DeviceState,
    /// Stored memory pages.
    pub pages: Vec<Page>,
}

/// Why a snapshot chain could not be restored. The machine is unchanged on error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// The chain is empty or does not start with a full snapshot.
    NotFull,
    /// A snapshot was taken from a machine with different RAM, disk or framebuffer sizes.
    SizeMismatch,
    /// A snapshot holds a page or a CSR list that does not fit this machine.
    Malformed,
    /// A later element is not a delta taken against the element just before it.
    BrokenChain,
}

impl Snapshot {
    /// Approximate stored size: page data plus CSR, FIFO and fixed register storage.
    pub fn bytes(&self) -> usize {
        let pages = self
            .pages
            .iter()
            .fold(0usize, |acc, p| acc.saturating_add(p.data.len()));
        let csr = 8 * self.cpu.csrs.len();
        let fifo = self.devices.uart_rx.len()
            + self.devices.uart_tx.len()
            + 4 * self.devices.keys_fifo.len();
        pages
            .saturating_add(csr)
            .saturating_add(fifo)
            .saturating_add(FIXED_STATE_BYTES)
    }
}

// The CSR list, in storage order. Every CSR field of `Csrs` must appear here exactly once.
macro_rules! csr_codec {
    ($($f:ident),* $(,)?) => {
        /// Flatten the CSR file into a vector in a fixed order.
        fn csr_words(c: &Csrs) -> Vec<u64> {
            let mut v = [$(c.$f),*].to_vec();
            v.extend_from_slice(&c.pmpcfg);
            v.extend_from_slice(&c.pmpaddr);
            v
        }

        /// Load the CSR file from `w`; `None` if `w` has the wrong length.
        fn set_csr_words(c: &mut Csrs, w: &[u64]) -> Option<()> {
            if w.len() != csr_words(c).len() {
                return None;
            }
            let mut it = w.iter().copied();
            $( c.$f = it.next()?; )*
            for x in c.pmpcfg.iter_mut() {
                *x = it.next()?;
            }
            for x in c.pmpaddr.iter_mut() {
                *x = it.next()?;
            }
            Some(())
        }
    };
}

csr_codec!(
    mstatus, medeleg, mideleg, mie, mip, mtvec, mcounteren, mscratch, mepc, mcause, mtval, mtinst,
    mtval2, scounteren, stvec, sscratch, sepc, scause, stval, satp, hstatus, hedeleg, hideleg,
    hcounteren, henvcfg, htval, htinst, hgatp, vsstatus, vstvec, vsscratch, vsepc, vscause, vstval,
    vsatp, mcycle, time,
);

/// True if `n` bytes at page `index` lie inside a region of `len` bytes.
fn page_fits(index: u64, n: usize, len: u64) -> bool {
    index
        .checked_mul(PAGE_SIZE as u64)
        .and_then(|off| off.checked_add(n as u64))
        .is_some_and(|end| end <= len)
}

impl Machine {
    /// Capture the machine. With a full `base`, only the pages that differ from it are stored.
    /// With no base, or a delta base, every non-zero page is stored and the result is full.
    /// A delta against a delta is not produced: the machine keeps no memory of earlier snapshots,
    /// so it cannot diff against one; the result is a full snapshot with `base_id` `None`.
    pub fn snapshot(&self, base: Option<&Snapshot>) -> Snapshot {
        let base = base.filter(|b| b.full);
        let refs: BTreeMap<(Region, u64), &[u8]> = base
            .map(|b| {
                b.pages
                    .iter()
                    .map(|p| ((p.region, p.index), p.data.as_slice()))
                    .collect()
            })
            .unwrap_or_default();
        let regions: [(Region, &[u8]); 3] = [
            (Region::Ram, self.board.ram.bytes()),
            (Region::Disk, self.board.block.disk.as_slice()),
            (Region::Framebuffer, self.board.display.fb.as_slice()),
        ];
        let mut pages = Vec::new();
        for (region, mem) in regions {
            for (i, chunk) in mem.chunks(PAGE_SIZE).enumerate() {
                let index = i as u64;
                let changed = match refs.get(&(region, index)) {
                    Some(reference) => *reference != chunk,
                    None => chunk.iter().any(|&b| b != 0),
                };
                if changed {
                    pages.push(Page {
                        region,
                        index,
                        data: chunk.to_vec(),
                    });
                }
            }
        }
        Snapshot {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            base_id: base.map(|b| b.id),
            full: base.is_none(),
            ram_size: self.board.ram_size as u64,
            disk_size: self.board.block.disk.len() as u64,
            fb_size: self.board.display.fb.len() as u64,
            cpu: self.cpu_state(),
            devices: self.device_state(),
            pages,
        }
    }

    /// Restore from `chain`: a full snapshot, then deltas, each taken against the element just
    /// before it. The chain is checked before anything changes, so on error the machine is
    /// untouched.
    pub fn restore(&mut self, chain: &[&Snapshot]) -> Result<(), SnapshotError> {
        match chain.first() {
            Some(first) if first.full => {}
            _ => return Err(SnapshotError::NotFull),
        }
        for pair in chain.windows(2) {
            if pair[1].full || pair[1].base_id != Some(pair[0].id) {
                return Err(SnapshotError::BrokenChain);
            }
        }
        let ram = self.board.ram_size as u64;
        let disk = self.board.block.disk.len() as u64;
        let fb = self.board.display.fb.len() as u64;
        let csr_len = csr_words(&Csrs::default()).len();
        for snap in chain {
            if (snap.ram_size, snap.disk_size, snap.fb_size) != (ram, disk, fb) {
                return Err(SnapshotError::SizeMismatch);
            }
            if snap.cpu.csrs.len() != csr_len {
                return Err(SnapshotError::Malformed);
            }
            for page in &snap.pages {
                let len = match page.region {
                    Region::Ram => ram,
                    Region::Disk => disk,
                    Region::Framebuffer => fb,
                };
                if page.data.len() > PAGE_SIZE || !page_fits(page.index, page.data.len(), len) {
                    return Err(SnapshotError::Malformed);
                }
            }
        }
        self.clear_memory();
        for snap in chain {
            for page in &snap.pages {
                self.write_page(page).ok_or(SnapshotError::Malformed)?;
            }
            self.apply_state(snap)?;
        }
        if let Some(cache) = &mut self.board.cache {
            cache.invalidate_all();
        }
        Ok(())
    }

    /// Hart state as stored in a snapshot.
    fn cpu_state(&self) -> CpuState {
        let cpu = &self.cpu;
        CpuState {
            x: cpu.x,
            pc: cpu.pc,
            instret: cpu.instret,
            mode: cpu.mode,
            virt: cpu.virt,
            reservation: cpu.reservation,
            cycle: self.board.clint.mtime,
            csrs: csr_words(&cpu.csr),
        }
    }

    /// Device state as stored in a snapshot.
    fn device_state(&self) -> DeviceState {
        let b = &self.board;
        DeviceState {
            uart_rx: b.uart.rx.iter().copied().collect(),
            uart_tx: b.uart.tx.clone(),
            uart_ier: b.uart.ier,
            uart_lcr: b.uart.lcr,
            uart_mcr: b.uart.mcr,
            uart_scr: b.uart.scr,
            uart_in_bytes: b.uart.in_bytes,
            uart_out_bytes: b.uart.out_bytes,
            block_sector: b.block.sector,
            block_dma: b.block.dma_addr,
            block_count: b.block.count,
            block_status: b.block.status,
            block_irq_enable: b.block.irq_enable,
            block_done: b.block.done,
            block_reads: b.block.reads,
            block_writes: b.block.writes,
            clint_msip: b.clint.msip,
            clint_mtimecmp: b.clint.mtimecmp,
            plic_priority: b.plic.priority,
            plic_pending: b.plic.pending,
            plic_in_flight: b.plic.in_flight,
            plic_enable: b.plic.enable,
            plic_threshold: b.plic.threshold,
            keys_fifo: b.keys.fifo.iter().copied().collect(),
            keys_irq_enable: b.keys.irq_enable,
            exit: b.finisher.exit,
        }
    }

    /// Load hart and device state from `snap`; only a CSR length mismatch can fail.
    fn apply_state(&mut self, snap: &Snapshot) -> Result<(), SnapshotError> {
        let c = &snap.cpu;
        let cpu = &mut self.cpu;
        cpu.x = c.x;
        cpu.pc = c.pc;
        cpu.instret = c.instret;
        cpu.mode = c.mode;
        cpu.virt = c.virt;
        cpu.reservation = c.reservation;
        set_csr_words(&mut cpu.csr, &c.csrs).ok_or(SnapshotError::Malformed)?;
        self.board.clint.mtime = c.cycle;

        let d = &snap.devices;
        let b = &mut self.board;
        b.uart.rx = d.uart_rx.iter().copied().collect::<VecDeque<u8>>();
        b.uart.tx = d.uart_tx.clone();
        b.uart.ier = d.uart_ier;
        b.uart.lcr = d.uart_lcr;
        b.uart.mcr = d.uart_mcr;
        b.uart.scr = d.uart_scr;
        b.uart.in_bytes = d.uart_in_bytes;
        b.uart.out_bytes = d.uart_out_bytes;
        b.block.sector = d.block_sector;
        b.block.dma_addr = d.block_dma;
        b.block.count = d.block_count;
        b.block.status = d.block_status;
        b.block.irq_enable = d.block_irq_enable;
        b.block.done = d.block_done;
        b.block.reads = d.block_reads;
        b.block.writes = d.block_writes;
        b.clint.msip = d.clint_msip;
        b.clint.mtimecmp = d.clint_mtimecmp;
        b.plic.priority = d.plic_priority;
        b.plic.pending = d.plic_pending;
        b.plic.in_flight = d.plic_in_flight;
        b.plic.enable = d.plic_enable;
        b.plic.threshold = d.plic_threshold;
        b.keys.fifo = d.keys_fifo.iter().copied().collect::<VecDeque<u32>>();
        b.keys.irq_enable = d.keys_irq_enable;
        b.finisher.exit = d.exit;
        Ok(())
    }

    /// Zero RAM, disk and framebuffer before a full snapshot is applied.
    fn clear_memory(&mut self) {
        self.board.ram.bytes_mut().fill(0);
        self.board.block.disk.fill(0);
        self.board.display.fb.fill(0);
    }

    /// Copy one stored page into its region; `None` if it does not fit (checked earlier).
    fn write_page(&mut self, page: &Page) -> Option<()> {
        let mem: &mut [u8] = match page.region {
            Region::Ram => self.board.ram.bytes_mut(),
            Region::Disk => self.board.block.disk.as_mut_slice(),
            Region::Framebuffer => self.board.display.fb.as_mut_slice(),
        };
        let start = usize::try_from(page.index.checked_mul(PAGE_SIZE as u64)?).ok()?;
        let end = start.checked_add(page.data.len())?;
        mem.get_mut(start..end)?.copy_from_slice(&page.data);
        Some(())
    }
}
