//! M-mode CSR emulation and guest-page-fault MMIO instruction decoding & device emulation.

use core::arch::asm;

use crate::boot::read_mtime;
use crate::guest::{hlvx_u16, GuestState, MSTATUS_MASK, MSTATUS_MPP, S_INT_MASK};

const MISA_GUEST: u64 =
    (2 << 62) | (1 << 0) | (1 << 2) | (1 << 8) | (1 << 12) | (1 << 18) | (1 << 20);
const GUEST_MEDELEG_MASK: u64 = 0xb7ff;
const GUEST_MIE_MASK: u64 = 0xaaa;

const FINISHER_BASE: u64 = 0x0010_0000;
const FINISHER_LEN: u64 = 4;
const CLINT_BASE: u64 = 0x0200_0000;
const CLINT_LEN: u64 = 0x1_0000;
const PLIC_BASE: u64 = 0x0C00_0000;
const PLIC_LEN: u64 = 0x0400_0000;
const UART_BASE: u64 = 0x1000_0000;
const UART_LEN: u64 = 8;
const BLOCK_BASE: u64 = 0x1000_1000;
const KEYS_BASE: u64 = 0x1000_2000;
const DISPLAY_BASE: u64 = 0x1000_3000;
const DEV_PAGE_LEN: u64 = 4096;

pub enum MmioOutcome {
    Done,
    Exited,
    AccessFault,
}

enum MemOp {
    Load { rd: usize, size: u64, signed: bool },
    Store { rs2: usize, size: u64 },
}

pub fn emulate_m_csr(g: &mut GuestState, inst: u32) -> bool {
    if (inst & 0x7f) != 0x73 {
        return false;
    }
    let funct3 = (inst >> 12) & 7;
    if !matches!(funct3, 1 | 2 | 3 | 5 | 6 | 7) {
        return false;
    }
    let rd = ((inst >> 7) & 0x1f) as usize;
    let rs1 = ((inst >> 15) & 0x1f) as usize;
    let csr = ((inst >> 20) & 0xfff) as u16;
    let is_write = matches!(funct3, 1 | 5) || rs1 != 0;

    if is_write && ((csr >> 10) & 3) == 3 {
        return false;
    }

    let Some(old) = read_m_csr(g, csr) else {
        return false;
    };

    let src = if funct3 >= 5 {
        rs1 as u64
    } else {
        g.regs.x[rs1]
    };
    let new_val = match funct3 {
        1 | 5 => src,
        2 | 6 => old | src,
        3 | 7 => old & !src,
        _ => return false,
    };

    if is_write {
        write_m_csr(g, csr, new_val);
    }
    if rd != 0 {
        g.regs.x[rd] = old;
    }
    g.regs.sepc = g.regs.sepc.wrapping_add(4);
    true
}

pub fn emulate_m_satp_or_sfence(g: &mut GuestState, inst: u32) -> bool {
    // sfence.vma: funct7=0001001 (0x09), funct3=0, rd=0, opcode=0x73
    if (inst & 0xfe00_7fff) == 0x1200_0073 {
        // hfence.vvma x0, x0
        // SAFETY: Executed in HS-mode to flush VS translations on guest sfence.vma.
        unsafe { asm!(".word 0x22000073", options(nomem, nostack)) };
        g.regs.sepc = g.regs.sepc.wrapping_add(4);
        return true;
    }
    if (inst & 0x7f) != 0x73 {
        return false;
    }
    let funct3 = (inst >> 12) & 7;
    let csr = ((inst >> 20) & 0xfff) as u16;
    if csr != 0x180 || !matches!(funct3, 1 | 2 | 3 | 5 | 6 | 7) {
        return false;
    }
    let rd = ((inst >> 7) & 0x1f) as usize;
    let rs1 = ((inst >> 15) & 0x1f) as usize;
    let is_write = matches!(funct3, 1 | 5) || rs1 != 0;
    let old = g.vsatp;
    let src = if funct3 >= 5 {
        rs1 as u64
    } else {
        g.regs.x[rs1]
    };
    let new_val = match funct3 {
        1 | 5 => src,
        2 | 6 => old | src,
        3 | 7 => old & !src,
        _ => return false,
    };
    if is_write && matches!(new_val >> 60, 0 | 8) {
        g.vsatp = new_val;
    }
    if rd != 0 {
        g.regs.x[rd] = old;
    }
    g.regs.sepc = g.regs.sepc.wrapping_add(4);
    true
}

fn read_m_csr(g: &GuestState, csr: u16) -> Option<u64> {
    Some(match csr {
        0x300 => g.mstatus & MSTATUS_MASK,
        0x301 => MISA_GUEST,
        0x302 => g.medeleg,
        0x303 => g.mideleg,
        0x304 => g.mie,
        0x305 => g.mtvec,
        0x306 | 0x30a => 0,
        0x340 => g.mscratch,
        0x341 => g.mepc,
        0x342 => g.mcause,
        0x343 => g.mtval,
        0x344 => {
            let mtip = if read_mtime() >= g.mtimecmp {
                1 << 7
            } else {
                0
            };
            (g.mip_sw & S_INT_MASK) | mtip
        }
        0x3a0..=0x3af if csr & 1 == 0 => 0,
        0x3b0..=0x3ef => 0,
        0xf11..=0xf14 => 0,
        _ => return None,
    })
}

fn write_m_csr(g: &mut GuestState, csr: u16, value: u64) {
    match csr {
        0x300 => {
            let mut v = value & MSTATUS_MASK;
            if (value & MSTATUS_MPP) == 0x1000 {
                v = (v & !MSTATUS_MPP) | (g.mstatus & MSTATUS_MPP);
            }
            g.mstatus = v;
        }
        0x302 => g.medeleg = value & GUEST_MEDELEG_MASK,
        0x303 => g.mideleg = value & S_INT_MASK,
        0x304 => g.mie = value & GUEST_MIE_MASK,
        0x305 => g.mtvec = value & !3,
        0x340 => g.mscratch = value,
        0x341 => g.mepc = value & !1,
        0x342 => g.mcause = value,
        0x343 => g.mtval = value,
        0x344 => g.mip_sw = value & S_INT_MASK,
        _ => {}
    }
}

pub fn emulate_mmio(g: &mut GuestState, is_store: bool, gpa: u64) -> MmioOutcome {
    let pc = g.regs.sepc;
    let Some(lo) = hlvx_u16(pc) else {
        return MmioOutcome::AccessFault;
    };

    let (op, inst_len) = if (lo & 3) != 3 {
        let Some(op) = decode_compressed_mem(lo) else {
            return MmioOutcome::AccessFault;
        };
        (op, 2u64)
    } else {
        let Some(hi) = hlvx_u16(pc.wrapping_add(2)) else {
            return MmioOutcome::AccessFault;
        };
        let inst32 = u32::from(lo) | (u32::from(hi) << 16);
        let Some(op) = decode_mem32(inst32) else {
            return MmioOutcome::AccessFault;
        };
        (op, 4u64)
    };

    match op {
        MemOp::Load { rd, size, signed } => {
            if is_store {
                return MmioOutcome::AccessFault;
            }
            let Some(raw) = mmio_read(g, gpa, size) else {
                return MmioOutcome::AccessFault;
            };
            let val = if signed {
                sign_extend(raw, size)
            } else {
                zero_mask(raw, size)
            };
            if rd != 0 {
                g.regs.x[rd] = val;
            }
            g.regs.sepc = pc.wrapping_add(inst_len);
            MmioOutcome::Done
        }
        MemOp::Store { rs2, size } => {
            if !is_store {
                return MmioOutcome::AccessFault;
            }
            let val = g.regs.x[rs2];
            match mmio_write(g, gpa, size, val) {
                MmioOutcome::Done => {
                    g.regs.sepc = pc.wrapping_add(inst_len);
                    MmioOutcome::Done
                }
                MmioOutcome::Exited => {
                    g.regs.sepc = pc.wrapping_add(inst_len);
                    MmioOutcome::Exited
                }
                MmioOutcome::AccessFault => MmioOutcome::AccessFault,
            }
        }
    }
}

fn decode_compressed_mem(inst: u16) -> Option<MemOp> {
    let quad = inst & 3;
    let funct3 = (inst >> 13) & 7;
    match quad {
        0 => {
            let reg = usize::from(8 + ((inst >> 2) & 7));
            match funct3 {
                2 => Some(MemOp::Load {
                    rd: reg,
                    size: 4,
                    signed: true,
                }),
                3 => Some(MemOp::Load {
                    rd: reg,
                    size: 8,
                    signed: false,
                }),
                6 => Some(MemOp::Store { rs2: reg, size: 4 }),
                7 => Some(MemOp::Store { rs2: reg, size: 8 }),
                _ => None,
            }
        }
        2 => match funct3 {
            2 => {
                let rd = usize::from((inst >> 7) & 0x1f);
                (rd != 0).then_some(MemOp::Load {
                    rd,
                    size: 4,
                    signed: true,
                })
            }
            3 => {
                let rd = usize::from((inst >> 7) & 0x1f);
                (rd != 0).then_some(MemOp::Load {
                    rd,
                    size: 8,
                    signed: false,
                })
            }
            6 => Some(MemOp::Store {
                rs2: usize::from((inst >> 2) & 0x1f),
                size: 4,
            }),
            7 => Some(MemOp::Store {
                rs2: usize::from((inst >> 2) & 0x1f),
                size: 8,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn decode_mem32(inst: u32) -> Option<MemOp> {
    let opcode = inst & 0x7f;
    let funct3 = (inst >> 12) & 7;
    match opcode {
        0x03 => {
            let rd = ((inst >> 7) & 0x1f) as usize;
            match funct3 {
                0 => Some(MemOp::Load {
                    rd,
                    size: 1,
                    signed: true,
                }),
                1 => Some(MemOp::Load {
                    rd,
                    size: 2,
                    signed: true,
                }),
                2 => Some(MemOp::Load {
                    rd,
                    size: 4,
                    signed: true,
                }),
                3 => Some(MemOp::Load {
                    rd,
                    size: 8,
                    signed: false,
                }),
                4 => Some(MemOp::Load {
                    rd,
                    size: 1,
                    signed: false,
                }),
                5 => Some(MemOp::Load {
                    rd,
                    size: 2,
                    signed: false,
                }),
                6 => Some(MemOp::Load {
                    rd,
                    size: 4,
                    signed: false,
                }),
                _ => None,
            }
        }
        0x23 => {
            let rs2 = ((inst >> 20) & 0x1f) as usize;
            match funct3 {
                0 => Some(MemOp::Store { rs2, size: 1 }),
                1 => Some(MemOp::Store { rs2, size: 2 }),
                2 => Some(MemOp::Store { rs2, size: 4 }),
                3 => Some(MemOp::Store { rs2, size: 8 }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn in_window(gpa: u64, size: u64, base: u64, len: u64) -> Option<u64> {
    let off = gpa.checked_sub(base)?;
    let end = off.checked_add(size)?;
    (end <= len).then_some(off)
}

fn mmio_read(g: &GuestState, gpa: u64, size: u64) -> Option<u64> {
    if let Some(off) = in_window(gpa, size, UART_BASE, UART_LEN) {
        let mut out = 0u64;
        for b in 0..size {
            if off + b == 5 {
                out |= 0x60u64 << (b * 8);
            }
        }
        return Some(out);
    }
    if let Some(off) = in_window(gpa, size, CLINT_BASE, CLINT_LEN) {
        return Some(match (off, size) {
            (0x4000, 8) => g.mtimecmp,
            (0x4000, 4) => g.mtimecmp & 0xffff_ffff,
            (0x4004, 4) => g.mtimecmp >> 32,
            (0xbff8, 8) => read_mtime(),
            (0xbff8, 4) => read_mtime() & 0xffff_ffff,
            (0xbffc, 4) => read_mtime() >> 32,
            _ => 0,
        });
    }
    if in_window(gpa, size, FINISHER_BASE, FINISHER_LEN).is_some()
        || in_window(gpa, size, BLOCK_BASE, DEV_PAGE_LEN).is_some()
        || in_window(gpa, size, KEYS_BASE, DEV_PAGE_LEN).is_some()
        || in_window(gpa, size, DISPLAY_BASE, DEV_PAGE_LEN).is_some()
        || in_window(gpa, size, PLIC_BASE, PLIC_LEN).is_some()
    {
        return Some(0);
    }
    None
}

fn mmio_write(g: &mut GuestState, gpa: u64, size: u64, val: u64) -> MmioOutcome {
    if let Some(off) = in_window(gpa, size, UART_BASE, UART_LEN) {
        if off == 0 {
            let mut name_copy = [0u8; 16];
            name_copy.copy_from_slice(&g.name);
            let name_len = g.name_len;
            g.console.push(&name_copy[..name_len], (val & 0xff) as u8);
        }
        return MmioOutcome::Done;
    }
    if let Some(off) = in_window(gpa, size, CLINT_BASE, CLINT_LEN) {
        match (off, size) {
            (0x4000, 8) => g.mtimecmp = val,
            (0x4000, 4) => g.mtimecmp = (g.mtimecmp & !0xffff_ffff) | (val & 0xffff_ffff),
            (0x4004, 4) => g.mtimecmp = (g.mtimecmp & 0xffff_ffff) | ((val & 0xffff_ffff) << 32),
            _ => {}
        }
        return MmioOutcome::Done;
    }
    if let Some(off) = in_window(gpa, size, FINISHER_BASE, FINISHER_LEN) {
        if off == 0 {
            let v = val as u32;
            if v == 0x5555 {
                g.exit_code(0, true);
                return MmioOutcome::Exited;
            }
            if (v & 0xffff) == 0x3333 {
                g.exit_code(v >> 16, false);
                return MmioOutcome::Exited;
            }
        }
        return MmioOutcome::Done;
    }
    if in_window(gpa, size, BLOCK_BASE, DEV_PAGE_LEN).is_some()
        || in_window(gpa, size, KEYS_BASE, DEV_PAGE_LEN).is_some()
        || in_window(gpa, size, DISPLAY_BASE, DEV_PAGE_LEN).is_some()
        || in_window(gpa, size, PLIC_BASE, PLIC_LEN).is_some()
    {
        return MmioOutcome::Done;
    }
    MmioOutcome::AccessFault
}

fn sign_extend(val: u64, size: u64) -> u64 {
    match size {
        1 => ((val as i8) as i64) as u64,
        2 => ((val as i16) as i64) as u64,
        4 => ((val as i32) as i64) as u64,
        _ => val,
    }
}

fn zero_mask(val: u64, size: u64) -> u64 {
    match size {
        1 => val & 0xff,
        2 => val & 0xffff,
        4 => val & 0xffff_ffff,
        _ => val,
    }
}
