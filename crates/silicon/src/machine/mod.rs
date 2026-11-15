//! RISC-V "virt"-style machine board: RAM, test finisher, CLINT, PLIC, UART, block, keys and display.
mod block;
mod cache;
mod clint;
mod display;
mod finisher;
mod keys;
mod plic;
mod uart;

use alloc::vec::Vec;

use crate::inst::Inst;
use crate::{Bus, Cpu, Fault, Ram, Step, Trap};

use block::{Block, BLOCK_SIZE};
use cache::DecodeCache;
use clint::{Clint, CLINT_SIZE};
use display::{Display, DISPLAY_SIZE};
use finisher::Finisher;
use keys::{Keys, KEYS_SIZE};
use plic::{Plic, PLIC_SIZE};
use uart::{Uart, UART_SIZE};

/// Base physical address of guest RAM.
pub const RAM_BASE: u64 = 0x8000_0000;

const FINISHER_BASE: u64 = 0x0010_0000;
const FINISHER_SIZE: u64 = 4;
const CLINT_BASE: u64 = 0x0200_0000;
const PLIC_BASE: u64 = 0x0C00_0000;
const UART_BASE: u64 = 0x1000_0000;
const BLOCK_BASE: u64 = 0x1000_1000;
const KEYS_BASE: u64 = 0x1000_2000;
const DISPLAY_BASE: u64 = 0x1000_3000;
const FB_BASE: u64 = 0x3000_0000;

const PLIC_SRC_BLOCK: u32 = 8;
const PLIC_SRC_UART: u32 = 10;
const PLIC_SRC_KEYS: u32 = 11;

/// Hardware configuration for a [`Machine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Guest RAM size in bytes, mapped at [`RAM_BASE`].
    pub ram_size: usize,
    /// Initial raw block-device disk image.
    pub disk: Vec<u8>,
    /// Display width in pixels.
    pub fb_width: u32,
    /// Display height in pixels.
    pub fb_height: u32,
    /// Enable the decoded-instruction cache.
    pub decode_cache: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ram_size: 16 * 1024 * 1024,
            disk: Vec::new(),
            fb_width: 320,
            fb_height: 240,
            decode_cache: true,
        }
    }
}

/// Outcome of [`Machine::run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Guest wrote `0x5555` to the test finisher.
    Pass,
    /// Guest wrote `(code << 16) | 0x3333` to the test finisher.
    Fail(u32),
    /// Step limit was reached before the test finisher was written.
    OutOfSteps,
}

/// Memory-mapped board holding RAM and all devices.
#[derive(Debug, Clone)]
pub struct Board {
    ram: Ram,
    ram_size: usize,
    finisher: Finisher,
    clint: Clint,
    plic: Plic,
    uart: Uart,
    block: Block,
    keys: Keys,
    display: Display,
    cache: Option<DecodeCache>,
}

impl Board {
    fn new(cfg: Config) -> Self {
        let ram = Ram::new(RAM_BASE, cfg.ram_size);
        let cache = cfg.decode_cache.then(DecodeCache::new);
        Self {
            ram,
            ram_size: cfg.ram_size,
            finisher: Finisher::default(),
            clint: Clint::default(),
            plic: Plic::default(),
            uart: Uart::default(),
            block: Block::new(cfg.disk),
            keys: Keys::default(),
            display: Display::new(cfg.fb_width, cfg.fb_height),
            cache,
        }
    }

    /// True if `[pa, pa + len)` lies completely within guest RAM.
    #[inline]
    fn ram_contains(&self, pa: u64, len: u64) -> bool {
        let Some(off) = pa.checked_sub(RAM_BASE) else {
            return false;
        };
        off.checked_add(len)
            .is_some_and(|end| end <= self.ram_size as u64)
    }

    /// Queue bytes for the guest to read from the UART RX FIFO (unbounded).
    pub fn uart_input(&mut self, bytes: &[u8]) {
        self.uart.push_input(bytes);
    }

    /// Drain all bytes written by the guest to the UART TX FIFO.
    pub fn take_uart_output(&mut self) -> Vec<u8> {
        self.uart.take_output()
    }

    /// Push a key code into the 64-deep key FIFO (extra keys are dropped when full).
    pub fn press_key(&mut self, code: u32) {
        self.keys.push(code);
    }

    /// Borrow the `fb_width * fb_height * 4` byte RGBA framebuffer.
    pub fn framebuffer(&self) -> &[u8] {
        self.display.framebuffer()
    }

    /// Borrow the raw block-device disk image.
    pub fn disk(&self) -> &[u8] {
        self.block.disk()
    }

    /// Current value of the CLINT `mtime` counter.
    pub fn mtime(&self) -> u64 {
        self.clint.mtime()
    }

    fn device_lines(&self) -> u32 {
        let mut mask = 0u32;
        if self.block.irq_line() {
            mask |= 1 << PLIC_SRC_BLOCK;
        }
        if self.uart.irq_line() {
            mask |= 1 << PLIC_SRC_UART;
        }
        if self.keys.irq_line() {
            mask |= 1 << PLIC_SRC_KEYS;
        }
        mask
    }
}

/// Offset of `addr` inside `[base, base + len)`, if it falls within that window.
fn in_window(addr: u64, base: u64, len: u64) -> Option<u64> {
    let off = addr.checked_sub(base)?;
    (off < len).then_some(off)
}

impl Bus for Board {
    fn read(&mut self, addr: u64, size: u8) -> Result<u64, Fault> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err(Fault::Unmapped(addr));
        }
        if let Some(off) = in_window(addr, FINISHER_BASE, FINISHER_SIZE) {
            return self.finisher.read(addr, off, size);
        }
        if let Some(off) = in_window(addr, CLINT_BASE, CLINT_SIZE) {
            return self.clint.read(addr, off, size);
        }
        if let Some(off) = in_window(addr, PLIC_BASE, PLIC_SIZE) {
            return self.plic.read(addr, off, size);
        }
        if let Some(off) = in_window(addr, UART_BASE, UART_SIZE) {
            return self.uart.read(addr, off, size);
        }
        if let Some(off) = in_window(addr, BLOCK_BASE, BLOCK_SIZE) {
            return self.block.read(addr, off, size);
        }
        if let Some(off) = in_window(addr, KEYS_BASE, KEYS_SIZE) {
            return self.keys.read(addr, off, size);
        }
        if let Some(off) = in_window(addr, DISPLAY_BASE, DISPLAY_SIZE) {
            return self.display.read_reg(addr, off, size);
        }
        if let Some(off) = in_window(addr, FB_BASE, self.display.fb_len()) {
            return self.display.read_fb(addr, off, size);
        }
        self.ram.read(addr, size)
    }

    fn write(&mut self, addr: u64, size: u8, value: u64) -> Result<(), Fault> {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err(Fault::Unmapped(addr));
        }
        if let Some(off) = in_window(addr, FINISHER_BASE, FINISHER_SIZE) {
            return self.finisher.write(addr, off, size, value);
        }
        if let Some(off) = in_window(addr, CLINT_BASE, CLINT_SIZE) {
            return self.clint.write(addr, off, size, value);
        }
        if let Some(off) = in_window(addr, PLIC_BASE, PLIC_SIZE) {
            return self.plic.write(addr, off, size, value);
        }
        if let Some(off) = in_window(addr, UART_BASE, UART_SIZE) {
            return self.uart.write(addr, off, size, value);
        }
        if let Some(off) = in_window(addr, BLOCK_BASE, BLOCK_SIZE) {
            let res = self
                .block
                .write(addr, off, size, value, &mut self.ram, self.ram_size);
            if res.is_ok() && off == 0x14 && size == 4 && (value as u32) == 1 {
                if let Some(cache) = &mut self.cache {
                    cache.invalidate_all();
                }
            }
            return res;
        }
        if let Some(off) = in_window(addr, KEYS_BASE, KEYS_SIZE) {
            return self.keys.write(addr, off, size, value);
        }
        if let Some(off) = in_window(addr, DISPLAY_BASE, DISPLAY_SIZE) {
            return self.display.write_reg(addr, off, size);
        }
        if let Some(off) = in_window(addr, FB_BASE, self.display.fb_len()) {
            return self.display.write_fb(addr, off, size, value);
        }
        self.ram.write(addr, size, value)?;
        if let Some(cache) = &mut self.cache {
            cache.invalidate_write(addr, usize::from(size));
        }
        Ok(())
    }
}

/// Complete RISC-V board pairing a [`Cpu`] with a [`Board`].
#[derive(Debug, Clone)]
pub struct Machine {
    /// The RV64IM hart.
    pub cpu: Cpu,
    /// RAM and memory-mapped peripherals.
    pub board: Board,
}

impl Machine {
    /// Construct a machine in M-mode starting at [`RAM_BASE`] with the given configuration.
    pub fn new(cfg: Config) -> Machine {
        Machine {
            cpu: Cpu::new(RAM_BASE),
            board: Board::new(cfg),
        }
    }

    /// Copy `data` into guest RAM at `addr`.
    pub fn load(&mut self, addr: u64, data: &[u8]) -> Result<(), Fault> {
        self.board.ram.load(addr, data)?;
        if let Some(cache) = &mut self.board.cache {
            cache.invalidate_write(addr, data.len());
        }
        Ok(())
    }

    /// One tick: `mtime += 1`; sample every device's interrupt line into the PLIC; set the hart's
    /// MTIP, MSIP, MEIP, SEIP from CLINT/PLIC state (`set_pending`); then `cpu.tick(&mut board)`.
    pub fn step(&mut self) -> Step {
        self.board.clint.tick();
        let lines = self.board.device_lines();
        self.board.plic.sample_lines(lines);
        self.cpu.set_pending(7, self.board.clint.mtip());
        self.cpu.set_pending(3, self.board.clint.msip());
        self.cpu.set_pending(11, self.board.plic.has_eligible(0));
        self.cpu.set_pending(9, self.board.plic.has_eligible(1));
        self.cpu.csr_write(0xC01, self.board.mtime());
        let step = if self.board.cache.is_some() {
            let (step, sfence) = self.cpu.tick_with(&mut self.board, fetch_cached);
            if sfence {
                if let Some(cache) = &mut self.board.cache {
                    cache.flush_itlb();
                }
            }
            step
        } else {
            self.cpu.tick(&mut self.board)
        };
        self.cpu.csr_write(0xC01, self.board.mtime());
        step
    }

    /// Step until the test finisher is written (`Pass` / `Fail(value >> 16)`) or `max_steps` ran.
    pub fn run(&mut self, max_steps: u64) -> Exit {
        for _ in 0..max_steps {
            self.step();
            if let Some(exit) = self.board.finisher.exit() {
                return exit;
            }
        }
        Exit::OutOfSteps
    }
}

/// Fetch and decode one instruction at `pc`, using the board's ITLB and decode cache.
fn fetch_cached(cpu: &Cpu, board: &mut Board, pc: u64) -> Result<(Inst, u32, u64), Trap> {
    if (pc & 0xFFF) == 0xFFE {
        return cpu.fetch(board, pc);
    }
    let pa = if cpu.is_fetch_bare() {
        pc
    } else {
        let vpage = pc >> 12;
        let keys = cpu.itlb_keys();
        let mode = cpu.mode;
        let virt = cpu.virt;
        if let Some(ppage) = board
            .cache
            .as_ref()
            .and_then(|c| c.lookup_itlb(vpage, keys, mode, virt))
        {
            ppage | (pc & 0xFFF)
        } else {
            let mut pt_mask = 0u64;
            let pa = cpu.translate_fetch_tracked(board, pc, &mut pt_mask)?;
            if let Some(cache) = &mut board.cache {
                cache.pt_pages_mask |= pt_mask;
                cache.insert_itlb(vpage, keys, mode, virt, pa & !0xFFF);
            }
            pa
        }
    };
    if let Some(hit) = board.cache.as_ref().and_then(|c| c.lookup_inst(pa)) {
        return Ok(hit);
    }
    let res = cpu.fetch_at_pa(board, pc, pa)?;
    if board.ram_contains(pa, res.2) {
        if let Some(cache) = &mut board.cache {
            cache.insert_inst(pa, res.0, res.1, res.2);
        }
    }
    Ok(res)
}
