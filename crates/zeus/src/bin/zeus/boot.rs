//! Boot table parser, Sv39x4 G-stage page tables, real UART/CLINT/finisher helpers, and guest line buffers.

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

pub const TWO_MIB: u64 = 2 * 1024 * 1024;
pub const MAX_GUEST_SIZE: u64 = 16 * 1024 * 1024;
pub const SLOT_BASE: u64 = 0x8100_0000;
pub const SLOT_STRIDE: u64 = 0x0100_0000;

const UART_BASE: usize = 0x1000_0000;
const FINISHER_ADDR: usize = 0x0010_0000;
const CLINT_MTIME: usize = 0x0200_BFF8;
const BOOT_TABLE_ADDR: usize = 0x8040_0000;
const BOOT_MAGIC: [u8; 8] = *b"ZEUSBOOT";

#[repr(C, align(16384))]
struct GStageRoot([u64; 2048]);

#[repr(C, align(4096))]
struct GStageL1([u64; 512]);

static mut GSTAGE_ROOTS: [GStageRoot; 3] = [
    GStageRoot([0; 2048]),
    GStageRoot([0; 2048]),
    GStageRoot([0; 2048]),
];

static mut GSTAGE_L1S: [GStageL1; 3] = [GStageL1([0; 512]), GStageL1([0; 512]), GStageL1([0; 512])];

/// Parsed guest entry from the host boot table.
#[derive(Clone, Copy)]
pub struct BootEntry {
    pub name: [u8; 16],
    pub name_len: usize,
    pub slot: u64,
    pub size: u64,
    pub entry: u64,
    pub a1: u64,
}

impl BootEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0; 16],
            name_len: 0,
            slot: 0,
            size: 0,
            entry: 0,
            a1: 0,
        }
    }
}

/// Per-guest 200-byte console line buffer.
#[derive(Clone, Copy)]
pub struct LineBuf {
    buf: [u8; 200],
    len: usize,
}

impl LineBuf {
    pub const fn new() -> Self {
        Self {
            buf: [0; 200],
            len: 0,
        }
    }

    pub fn push(&mut self, name: &[u8], byte: u8) {
        if byte == b'\n' {
            self.emit_line(name);
            return;
        }
        if self.len < self.buf.len() {
            self.buf[self.len] = byte;
            self.len += 1;
        }
        if self.len == self.buf.len() {
            self.emit_line(name);
        }
    }

    pub fn flush(&mut self, name: &[u8]) {
        if self.len > 0 {
            self.emit_line(name);
        }
    }

    fn emit_line(&mut self, name: &[u8]) {
        uart_putc(b'[');
        uart_write(name);
        uart_write(b"] ");
        uart_write(&self.buf[..self.len]);
        uart_putc(b'\n');
        self.len = 0;
    }
}

pub fn uart_putc(b: u8) {
    let lsr = (UART_BASE + 5) as *const u8;
    let thr = UART_BASE as *mut u8;
    // SAFETY: UART_BASE is Silicon's memory-mapped 16550 UART in HS-mode physical memory.
    unsafe {
        while (read_volatile(lsr) & 0x20) == 0 {}
        write_volatile(thr, b);
    }
}

pub fn uart_write(bytes: &[u8]) {
    for &b in bytes {
        uart_putc(b);
    }
}

pub fn print_str(s: &str) {
    uart_write(s.as_bytes());
}

pub fn print_u64(mut n: u64) {
    if n == 0 {
        uart_putc(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    uart_write(&buf[i..]);
}

pub fn read_mtime() -> u64 {
    let ptr = CLINT_MTIME as *const u64;
    // SAFETY: CLINT_MTIME is Silicon's memory-mapped 64-bit mtime register.
    unsafe { read_volatile(ptr) }
}

pub fn finish_machine(pass: bool) -> ! {
    let val: u32 = if pass { 0x5555 } else { (1 << 16) | 0x3333 };
    let ptr = FINISHER_ADDR as *mut u32;
    // SAFETY: FINISHER_ADDR is Silicon's test finisher device.
    unsafe {
        write_volatile(ptr, val);
        loop {
            asm!("wfi", options(nomem, nostack));
        }
    }
}

fn read_boot_u64(off: usize) -> u64 {
    let ptr = (BOOT_TABLE_ADDR + off) as *const u64;
    // SAFETY: BOOT_TABLE_ADDR..+4096 is valid physical RAM mapped at 0x8040_0000.
    unsafe { read_volatile(ptr) }
}

fn read_boot_u8(off: usize) -> u8 {
    let ptr = (BOOT_TABLE_ADDR + off) as *const u8;
    // SAFETY: BOOT_TABLE_ADDR..+4096 is valid physical RAM mapped at 0x8040_0000.
    unsafe { read_volatile(ptr) }
}

pub fn parse_boot_table() -> Option<(usize, [BootEntry; 3])> {
    for (i, &expected) in BOOT_MAGIC.iter().enumerate() {
        if read_boot_u8(i) != expected {
            return None;
        }
    }
    let count_u64 = read_boot_u64(8);
    if !(1..=3).contains(&count_u64) {
        return None;
    }
    let count = count_u64 as usize;
    let mut entries = [BootEntry::empty(); 3];

    for (i, entry) in entries.iter_mut().take(count).enumerate() {
        let base = 16 + i * 48;
        let mut name = [0u8; 16];
        let mut name_len = 16usize;
        let mut saw_nul = false;
        for (j, slot_byte) in name.iter_mut().enumerate() {
            let b = read_boot_u8(base + j);
            *slot_byte = b;
            if b == 0 {
                if !saw_nul {
                    name_len = j;
                    saw_nul = true;
                }
            } else if saw_nul || !matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-') {
                return None;
            }
        }
        if name_len == 0 {
            return None;
        }

        let slot = read_boot_u64(base + 16);
        let size = read_boot_u64(base + 24);
        let entry_pc = read_boot_u64(base + 32);
        let a1 = read_boot_u64(base + 40);

        let expected_slot = SLOT_BASE + (i as u64) * SLOT_STRIDE;
        if slot != expected_slot {
            return None;
        }
        if size == 0 || size > MAX_GUEST_SIZE || size % TWO_MIB != 0 {
            return None;
        }

        *entry = BootEntry {
            name,
            name_len,
            slot,
            size,
            entry: entry_pc,
            a1,
        };
    }

    Some((count, entries))
}

/// Build the Sv39x4 G-stage page table for guest `idx` and return its `hgatp` value.
pub fn build_gstage(idx: usize, slot: u64, size: u64) -> u64 {
    // SAFETY: Each guest index 0..3 owns its disjoint static root and L1 page table in `.bss`,
    // and `build_gstage` is called once per guest during single-hart initialization.
    unsafe {
        let root = &mut GSTAGE_ROOTS[idx];
        let l1 = &mut GSTAGE_L1S[idx];
        root.0.fill(0);
        l1.0.fill(0);

        let l1_pa = (&raw const l1.0) as u64;
        root.0[2] = ((l1_pa >> 12) << 10) | 1;

        let count = (size / TWO_MIB) as usize;
        for (k, pte) in l1.0.iter_mut().take(count).enumerate() {
            let hpa = slot + (k as u64) * TWO_MIB;
            *pte = ((hpa >> 12) << 10) | 0xdf;
        }

        let root_pa = (&raw const root.0) as u64;
        let vmid = (idx as u64) + 1;
        (8u64 << 60) | (vmid << 44) | ((root_pa >> 12) & ((1u64 << 44) - 1))
    }
}
