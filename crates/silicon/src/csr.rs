//! Control and status registers: storage, WARL rules, the host read and write API, and the
//! access checks that instructions apply.
use crate::{Cpu, Mode, Trap};

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
pub(crate) const MSTATUS_GVA: u64 = 1 << 38;
pub(crate) const MPV: u64 = 1 << 39;

pub(crate) const HSTATUS_GVA: u64 = 1 << 6;
pub(crate) const SPV: u64 = 1 << 7;
pub(crate) const SPVP: u64 = 1 << 8;
pub(crate) const HU: u64 = 1 << 9;
pub(crate) const VTVM: u64 = 1 << 20;
pub(crate) const VTW: u64 = 1 << 21;
pub(crate) const VTSR: u64 = 1 << 22;

const SSIP: u64 = 1 << 1;
const VSSIP: u64 = 1 << 2;
const UXL: u64 = 2 << 32;
const SXL: u64 = 2 << 34;
const VSXL: u64 = 2 << 32;
const SSTATUS_MASK: u64 = SIE | SPIE | SPP | SUM | MXR;
const MSTATUS_WMASK: u64 =
    SIE | MIE | SPIE | MPIE | SPP | MPP | MPRV | SUM | MXR | TVM | TW | TSR | MSTATUS_GVA | MPV;
const HSTATUS_WMASK: u64 = HSTATUS_GVA | SPV | SPVP | HU | VTVM | VTW | VTSR;
/// Supervisor interrupt bits: SSI (1), STI (5), SEI (9).
const S_INT: u64 = (1 << 1) | (1 << 5) | (1 << 9);
/// Virtual-supervisor interrupt bits: VSSI (2), VSTI (6), VSEI (10).
pub(crate) const VS_INT: u64 = (1 << 2) | (1 << 6) | (1 << 10);
/// Hypervisor interrupt bits always delegated in mideleg: VSSI (2), VSTI (6), VSEI (10), SGEI (12).
pub(crate) const H_INT: u64 = VS_INT | (1 << 12);
/// Bits software may write in mip from M-mode: SSIP, VSSIP, STIP, SEIP. MSIP, MTIP, MEIP are hardware only.
const MIP_SW: u64 = S_INT | VSSIP;
/// Writable mie bits: all M, S, VS and SGEI interrupt enables.
const MIE_MASK: u64 = 0x1eee;
/// Delegable exceptions in medeleg.
const MEDELEG_MASK: u64 = 0xf0_b7ff;
/// Delegable exceptions in hedeleg: 0..=8, 12, 13, 15.
const HEDELEG_MASK: u64 = 0xb1ff;
/// Writable hgatp mask: MODE (63..60) and PPN[43:2].
const HGATP_MASK: u64 = (0xf << 60) | (((1u64 << 44) - 1) & !3);
/// misa: RV64 (MXL=2) with A, C, H, I, M, S and U.
const MISA: u64 =
    (2 << 62) | (1 << 0) | (1 << 2) | (1 << 7) | (1 << 8) | (1 << 12) | (1 << 18) | (1 << 20);
/// Writable pmpcfg bits per byte: bits 6..5 are reserved (read 0).
const PMPCFG_MASK: u64 = 0x9f9f_9f9f_9f9f_9f9f;
/// Writable pmpaddr bits: 53..0 (granularity 0).
const PMPADDR_MASK: u64 = (1 << 54) - 1;

/// Raw storage for the CSRs that hold state. Read and write rules live in `Cpu` below.
///
/// Known gap: `pmpcfg0`/`pmpcfg2` and `pmpaddr0..15` are stored and masked, but memory accesses
/// are not checked against PMP entries.
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
    pub(crate) mtinst: u64,
    pub(crate) mtval2: u64,
    pub(crate) scounteren: u64,
    pub(crate) stvec: u64,
    pub(crate) sscratch: u64,
    pub(crate) sepc: u64,
    pub(crate) scause: u64,
    pub(crate) stval: u64,
    pub(crate) satp: u64,
    pub(crate) hstatus: u64,
    pub(crate) hedeleg: u64,
    pub(crate) hideleg: u64,
    pub(crate) hcounteren: u64,
    pub(crate) henvcfg: u64,
    pub(crate) htval: u64,
    pub(crate) htinst: u64,
    pub(crate) hgatp: u64,
    pub(crate) vsstatus: u64,
    pub(crate) vstvec: u64,
    pub(crate) vsscratch: u64,
    pub(crate) vsepc: u64,
    pub(crate) vscause: u64,
    pub(crate) vstval: u64,
    pub(crate) vsatp: u64,
    pub(crate) mcycle: u64,
    pub(crate) time: u64,
    pub(crate) pmpcfg: [u64; 2],
    pub(crate) pmpaddr: [u64; 16],
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
            0x200 => (c.vsstatus & SSTATUS_MASK) | UXL,
            0x204 => (c.mie & c.hideleg & VS_INT) >> 1,
            0x205 => c.vstvec,
            0x240 => c.vsscratch,
            0x241 => c.vsepc,
            0x242 => c.vscause,
            0x243 => c.vstval,
            0x244 => (c.mip & c.hideleg & VS_INT) >> 1,
            0x280 => c.vsatp,
            0x300 => c.mstatus | UXL | SXL,
            0x301 => MISA,
            0x302 => c.medeleg,
            0x303 => c.mideleg | H_INT,
            0x304 => c.mie,
            0x305 => c.mtvec,
            0x306 => c.mcounteren,
            0x340 => c.mscratch,
            0x341 => c.mepc,
            0x342 => c.mcause,
            0x343 => c.mtval,
            0x344 => c.mip,
            0x34a => c.mtinst,
            0x34b => c.mtval2,
            // PMP: pmpcfg0/2 and pmpaddr0..15 are stored; higher PMP CSRs read 0 (no PMP enforcement).
            0x3a0 => c.pmpcfg[0],
            0x3a2 => c.pmpcfg[1],
            0x3a4..=0x3af if addr & 1 == 0 => 0,
            0x3b0..=0x3bf => c
                .pmpaddr
                .get(usize::from(addr - 0x3b0))
                .copied()
                .unwrap_or(0),
            0x3c0..=0x3ef => 0,
            0x600 => (c.hstatus & HSTATUS_WMASK) | VSXL,
            0x602 => c.hedeleg,
            0x603 => c.hideleg,
            0x604 => c.mie & H_INT,
            0x606 => c.hcounteren,
            0x607 => 0,
            0x60a => c.henvcfg,
            0x643 => c.htval,
            0x644 => c.mip & VS_INT,
            0x645 => c.mip & VS_INT,
            0x64a => c.htinst,
            0x680 => c.hgatp,
            0xb00 => c.mcycle,
            0xb02 => self.instret,
            0xc00 => c.mcycle,
            0xc01 => c.time,
            0xc02 => self.instret,
            0xe12 => 0,
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
            0x200 => c.vsstatus = value & SSTATUS_MASK,
            0x204 => {
                let m = c.hideleg & VS_INT;
                c.mie = (c.mie & !m) | (((value & S_INT) << 1) & m);
            }
            0x205 => c.vstvec = value,
            0x240 => c.vsscratch = value,
            0x241 => c.vsepc = value & !1,
            0x242 => c.vscause = value,
            0x243 => c.vstval = value,
            0x244 => {
                let m = c.hideleg & VSSIP;
                c.mip = (c.mip & !m) | (((value & SSIP) << 1) & m);
            }
            0x280 => {
                if matches!(value >> 60, 0 | 8) {
                    c.vsatp = value;
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
            0x34a => c.mtinst = value,
            0x34b => c.mtval2 = value,
            0x3a0 => c.pmpcfg[0] = value & PMPCFG_MASK,
            0x3a2 => c.pmpcfg[1] = value & PMPCFG_MASK,
            0x3b0..=0x3bf => {
                if let Some(slot) = c.pmpaddr.get_mut(usize::from(addr - 0x3b0)) {
                    *slot = value & PMPADDR_MASK;
                }
            }
            0x600 => c.hstatus = value & HSTATUS_WMASK,
            0x602 => c.hedeleg = value & HEDELEG_MASK,
            0x603 => c.hideleg = value & VS_INT,
            0x604 => c.mie = (c.mie & !H_INT) | (value & H_INT),
            0x606 => c.hcounteren = value,
            0x60a => c.henvcfg = value,
            0x643 => c.htval = value,
            0x644 => c.mip = (c.mip & !VSSIP) | (value & VSSIP),
            0x645 => c.mip = (c.mip & !VS_INT) | (value & VS_INT),
            0x64a => c.htinst = value,
            0x680 => {
                if matches!(value >> 60, 0 | 8) {
                    c.hgatp = value & HGATP_MASK;
                }
            }
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

    /// Check whether an instruction in the current mode may access `addr`, returning the
    /// effective CSR address (after VS redirection) or the appropriate `Trap`.
    pub(crate) fn csr_check(&self, addr: u16, write: bool, word: u32) -> Result<u16, Trap> {
        let illegal = Trap::IllegalInstruction(word);
        let virt_inst = Trap::VirtualInstruction(word);
        if self.csr_read(addr).is_none() || (write && (u64::from(addr) >> 10) & 3 == 3) {
            return Err(illegal);
        }
        let level = (u64::from(addr) >> 8) & 3;
        match level {
            3 => {
                if self.mode != Mode::Machine || self.virt {
                    return Err(illegal);
                }
                Ok(addr)
            }
            2 => {
                if self.virt {
                    return Err(virt_inst);
                }
                if self.mode == Mode::User {
                    return Err(illegal);
                }
                Ok(addr)
            }
            1 => {
                if self.virt {
                    if self.mode == Mode::User || (addr == 0x180 && (self.csr.hstatus & VTVM) != 0)
                    {
                        return Err(virt_inst);
                    }
                    let eff = match addr {
                        0x100 | 0x104 | 0x105 | 0x140..=0x144 | 0x180 => addr + 0x100,
                        _ => addr,
                    };
                    return Ok(eff);
                }
                if self.mode == Mode::User
                    || (addr == 0x180
                        && self.mode == Mode::Supervisor
                        && (self.csr.mstatus & TVM) != 0)
                {
                    return Err(illegal);
                }
                Ok(addr)
            }
            _ => {
                if (0xc00..=0xc02).contains(&addr) {
                    let bit = 1u64 << (addr - 0xc00);
                    if self.mode != Mode::Machine && (self.csr.mcounteren & bit) == 0 {
                        return Err(illegal);
                    }
                    if self.virt {
                        if (self.csr.hcounteren & bit) == 0
                            || (self.mode == Mode::User && (self.csr.scounteren & bit) == 0)
                        {
                            return Err(virt_inst);
                        }
                    } else if self.mode == Mode::User && (self.csr.scounteren & bit) == 0 {
                        return Err(illegal);
                    }
                }
                Ok(addr)
            }
        }
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
