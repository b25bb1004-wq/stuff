//! Control and status registers: storage, WARL rules, the host read and write API, and the
//! access checks that instructions apply.
use crate::{Cpu, Mode};

pub(crate) const SIE: u64 = 1 << 1;
pub(crate) const MIE: u64 = 1 << 3;
pub(crate) const SPIE: u64 = 1 << 5;
pub(crate) const MPIE: u64 = 1 << 7;
pub(crate) const SPP: u64 = 1 << 8;
pub(crate) const MPP: u64 = 3 << 11;
pub(crate) const MPRV: u64 = 1 << 17;
pub(crate) const SUM: u64 = 1 << 18;
pub(crate) const MXR: u64 = 1 << 19;
pub(crate) const TVM: u64 = 1 << 20;
pub(crate) const TW: u64 = 1 << 21;
pub(crate) const TSR: u64 = 1 << 22;

const SSIP: u64 = 1 << 1;
const UXL: u64 = 2 << 32;
const SXL: u64 = 2 << 34;
const SSTATUS_MASK: u64 = SIE | SPIE | SPP | SUM | MXR;
const MSTATUS_WMASK: u64 = SIE | MIE | SPIE | MPIE | SPP | MPP | MPRV | SUM | MXR | TVM | TW | TSR;
/// Supervisor interrupt bits: SSI (1), STI (5), SEI (9).
const S_INT: u64 = (1 << 1) | (1 << 5) | (1 << 9);
/// Bits software may write in mip from M-mode: SSIP, STIP, SEIP. MSIP, MTIP, MEIP are hardware only.
const MIP_SW: u64 = S_INT;
/// Writable mie bits: all six interrupt enables.
const MIE_MASK: u64 = 0xAAA;
/// Delegable exceptions: every cause up to 15 except 10, 11 and 14 (reserved, and ECALL from M).
const MEDELEG_MASK: u64 = 0xB3FF;
/// misa: RV64 (MXL=2) with A, C, I, M, S and U.
const MISA: u64 = (2 << 62) | (1 << 0) | (1 << 2) | (1 << 8) | (1 << 12) | (1 << 18) | (1 << 20);

/// Raw storage for the CSRs that hold state. Read and write rules live in `Cpu` below.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Csrs {
    pub(crate) mstatus: u64,
    pub(crate) medeleg: u64,
    pub(crate) mideleg: u64,
    pub(crate) mie: u64,
    pub(crate) mip: u64,
    pub(crate) mtvec: u64,
    pub(crate) mcounteren: u64,
    pub(crate) mscratch: u64,
    pub(crate) mepc: u64,
    pub(crate) mcause: u64,
    pub(crate) mtval: u64,
    pub(crate) scounteren: u64,
    pub(crate) stvec: u64,
    pub(crate) sscratch: u64,
    pub(crate) sepc: u64,
    pub(crate) scause: u64,
    pub(crate) stval: u64,
    pub(crate) satp: u64,
    pub(crate) mcycle: u64,
    pub(crate) time: u64,
}

/// A trap vector base: MODE 0 (direct) and 1 (vectored) are legal, 2 and 3 keep the old mode.
pub(crate) fn tvec(old: u64, value: u64) -> u64 {
    let mode = if value & 3 <= 1 { value & 3 } else { old & 3 };
    (value & !3) | mode
}

impl Cpu {
    /// Read a CSR without a privilege check (host access). `None` if there is no such CSR.
    pub fn csr_read(&self, addr: u16) -> Option<u64> {
        let c = &self.csr;
        Some(match addr {
            0x100 => (c.mstatus & SSTATUS_MASK) | UXL,
            0x104 => c.mie & c.mideleg & S_INT,
            0x105 => c.stvec,
            0x106 => c.scounteren,
            0x140 => c.sscratch,
            0x141 => c.sepc,
            0x142 => c.scause,
            0x143 => c.stval,
            0x144 => c.mip & c.mideleg & S_INT,
            0x180 => c.satp,
            0x300 => c.mstatus | UXL | SXL,
            0x301 => MISA,
            0x302 => c.medeleg,
            0x303 => c.mideleg,
            0x304 => c.mie,
            0x305 => c.mtvec,
            0x306 => c.mcounteren,
            0x340 => c.mscratch,
            0x341 => c.mepc,
            0x342 => c.mcause,
            0x343 => c.mtval,
            0x344 => c.mip,
            // PMP: pmpcfg0..15 exist on RV64 at even indices only; pmpaddr0..63 all exist.
            0x3a0..=0x3af if addr & 1 == 0 => 0,
            0x3b0..=0x3ef => 0,
            0xb00 => c.mcycle,
            0xb02 => self.instret,
            0xc00 => c.mcycle,
            0xc01 => c.time,
            0xc02 => self.instret,
            0xf11..=0xf14 => 0,
            _ => return None,
        })
    }

    /// Write a CSR without a privilege check (host access), applying the WARL rules. Writes to
    /// read-only CSRs are ignored, except the host may set `time` (0xC01). `false` if there is
    /// no such CSR.
    pub fn csr_write(&mut self, addr: u16, value: u64) -> bool {
        if self.csr_read(addr).is_none() {
            return false;
        }
        let c = &mut self.csr;
        match addr {
            0x100 => write_mstatus(c, value, SSTATUS_MASK),
            0x104 => {
                let m = c.mideleg & S_INT;
                c.mie = (c.mie & !m) | (value & m);
            }
            0x105 => c.stvec = tvec(c.stvec, value),
            0x106 => c.scounteren = value & 7,
            0x140 => c.sscratch = value,
            0x141 => c.sepc = value & !1,
            0x142 => c.scause = value,
            0x143 => c.stval = value,
            0x144 => {
                let m = c.mideleg & SSIP;
                c.mip = (c.mip & !m) | (value & m);
            }
            // Only the Bare (0) and Sv39 (8) modes exist; other mode values leave satp alone.
            0x180 => {
                if matches!(value >> 60, 0 | 8) {
                    c.satp = value;
                }
            }
            0x300 => write_mstatus(c, value, MSTATUS_WMASK),
            0x302 => c.medeleg = value & MEDELEG_MASK,
            0x303 => c.mideleg = value & S_INT,
            0x304 => c.mie = value & MIE_MASK,
            0x305 => c.mtvec = tvec(c.mtvec, value),
            0x306 => c.mcounteren = value & 7,
            0x340 => c.mscratch = value,
            0x341 => c.mepc = value & !1,
            0x342 => c.mcause = value,
            0x343 => c.mtval = value,
            0x344 => c.mip = (c.mip & !MIP_SW) | (value & MIP_SW),
            0xb00 => c.mcycle = value,
            0xb02 => self.instret = value,
            0xc01 => c.time = value,
            _ => {}
        }
        true
    }

    /// Set or clear one interrupt-pending bit (1 SSIP, 3 MSIP, 5 STIP, 7 MTIP, 9 SEIP, 11 MEIP).
    /// Other bit numbers are ignored.
    pub fn set_pending(&mut self, bit: u8, on: bool) {
        if !matches!(bit, 1 | 3 | 5 | 7 | 9 | 11) {
            return;
        }
        let mask = 1u64 << bit;
        if on {
            self.csr.mip |= mask;
        } else {
            self.csr.mip &= !mask;
        }
    }

    /// Whether an instruction in the current mode may access `addr`: the CSR exists, the
    /// privilege level suffices, writes target a writable CSR, counters are enabled, and
    /// satp is not trapped by TVM in S-mode.
    pub(crate) fn csr_allowed(&self, addr: u16, write: bool) -> bool {
        if self.csr_read(addr).is_none() {
            return false;
        }
        let level = (u64::from(addr) >> 8) & 3;
        if level > self.mode.level() {
            return false;
        }
        if write && (u64::from(addr) >> 10) & 3 == 3 {
            return false;
        }
        if (0xc00..=0xc02).contains(&addr) {
            let bit = 1u64 << (addr - 0xc00);
            if self.mode != Mode::Machine && (self.csr.mcounteren & bit) == 0 {
                return false;
            }
            if self.mode == Mode::User && (self.csr.scounteren & bit) == 0 {
                return false;
            }
        }
        !(addr == 0x180 && self.mode == Mode::Supervisor && (self.csr.mstatus & TVM) != 0)
    }
}

/// Write the bits of `mstatus` selected by `mask`. A reserved MPP value (2) keeps the old MPP.
fn write_mstatus(c: &mut Csrs, value: u64, mask: u64) {
    let old = c.mstatus;
    let mut new = (old & !mask) | (value & mask);
    if mask & MPP != 0 && (value & MPP) == 0x1000 {
        new = (new & !MPP) | (old & MPP);
    }
    c.mstatus = new;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::Cpu;

    #[test]
    fn misa_and_ids_read_fixed_values() {
        let cpu = Cpu::new(0);
        assert_eq!(cpu.csr_read(0x301), Some(super::MISA));
        assert_eq!(cpu.csr_read(0xf14), Some(0));
        assert_eq!(cpu.csr_read(0x7ff), None);
    }

    #[test]
    fn tvec_keeps_old_mode_for_reserved_values() {
        assert_eq!(super::tvec(0, 0x1003), 0x1000);
        assert_eq!(super::tvec(0x2001, 0x1002), 0x1001);
    }
}
