//! Fetch, decode and execute one instruction. Effects are computed first and committed last,
//! so a trap leaves the cpu exactly as it was.
use crate::bus::Bus;
use crate::csr::{MIE, MPIE, MPP, MPRV, MPV, SIE, SPIE, SPP, SPV, TSR, TVM, TW, VTSR, VTVM, VTW};
use crate::inst::{decode, decode_c, Inst};
use crate::mmu::extend;
use crate::{Access, Cpu, Mode, Trap};

/// Everything one instruction changes, produced before anything is committed.
struct Effect {
    pc: u64,
    rd: Option<(u8, u64)>,
    csr: Option<(u16, u64)>,
    csr2: Option<(u16, u64)>,
    mode: Option<Mode>,
    virt: Option<bool>,
    reservation: Option<Option<u64>>,
    wfi: bool,
    sfence: bool,
}

impl Effect {
    /// Continue at `pc` with no other change.
    fn at(pc: u64) -> Effect {
        Effect {
            pc,
            rd: None,
            csr: None,
            csr2: None,
            mode: None,
            virt: None,
            reservation: None,
            wfi: false,
            sfence: false,
        }
    }

    /// Continue at `pc` after writing `value` to `rd`.
    fn wb(pc: u64, rd: u8, value: u64) -> Effect {
        Effect {
            rd: Some((rd, value)),
            ..Effect::at(pc)
        }
    }
}

type Outcome = Result<Effect, Trap>;

/// How a CSR instruction combines the old value with its source.
#[derive(Clone, Copy)]
enum Op {
    Replace,
    Set,
    Clear,
}

/// Fail with `FetchMisaligned` unless `target` is a multiple of 2 (IALIGN=16).
fn check_pc(target: u64) -> Result<(), Trap> {
    if target & 1 != 0 {
        Err(Trap::FetchMisaligned(target))
    } else {
        Ok(())
    }
}

/// Jump to `target` and write the link value; the target must be a multiple of 2.
fn jump(target: u64, link: u64, rd: u8) -> Outcome {
    check_pc(target)?;
    Ok(Effect::wb(target, rd, link))
}

/// Conditional branch: fall through to `next`, or jump to `pc + imm` when `taken`.
fn branch(pc: u64, next: u64, taken: bool, imm: i64) -> Outcome {
    if !taken {
        return Ok(Effect::at(next));
    }
    let target = pc.wrapping_add(imm as u64);
    check_pc(target)?;
    Ok(Effect::at(target))
}

/// Sideband flags from `step_inner_with`.
pub(crate) struct StepFlags {
    pub(crate) wfi: bool,
    pub(crate) sfence: bool,
}

impl Cpu {
    /// Execute one instruction using `fetch_fn` and commit it. On error, returns `(Trap, from_hlv_hsv)`.
    #[inline(always)]
    pub(crate) fn step_inner_with<B: Bus>(
        &mut self,
        bus: &mut B,
        fetch_fn: impl FnOnce(&Self, &mut B, u64) -> Result<(Inst, u32, u64), Trap>,
    ) -> Result<StepFlags, (Trap, bool)> {
        let pc = self.pc;
        check_pc(pc).map_err(|t| (t, false))?;
        let (inst, word, len) = fetch_fn(self, bus, pc).map_err(|t| (t, false))?;
        let next = pc.wrapping_add(len);
        let from_hlv_hsv = matches!(inst, Inst::Hlv { .. } | Inst::Hsv { .. });
        let eff = self
            .execute(inst, word, next, bus)
            .map_err(|t| (t, from_hlv_hsv))?;
        if let Some((rd, value)) = eff.rd {
            self.set(rd, value);
        }
        let wrote_csr = eff.csr.map(|(addr, _)| addr);
        if let Some((addr, value)) = eff.csr {
            self.csr_write(addr, value);
        }
        if let Some((addr, value)) = eff.csr2 {
            self.csr_write(addr, value);
        }
        if let Some(mode) = eff.mode {
            self.mode = mode;
        }
        if let Some(virt) = eff.virt {
            self.virt = virt;
        }
        if let Some(res) = eff.reservation {
            self.reservation = res;
        }
        self.pc = eff.pc;
        if wrote_csr != Some(0xb02) {
            self.instret = self.instret.wrapping_add(1);
        }
        if wrote_csr != Some(0xb00) {
            self.csr.mcycle = self.csr.mcycle.wrapping_add(1);
        }
        let sfence =
            eff.sfence || eff.virt.is_some() || matches!(wrote_csr, Some(0x180 | 0x280 | 0x680));
        Ok(StepFlags {
            wfi: eff.wfi,
            sfence,
        })
    }

    /// Execute the instruction at `pc`. On `Err`, pc, registers and CSRs are unchanged.
    pub fn step(&mut self, bus: &mut impl Bus) -> Result<(), Trap> {
        self.step_inner_with(bus, |cpu, bus, pc| cpu.fetch(bus, pc))
            .map(|_| ())
            .map_err(|(t, _)| t)
    }

    /// Fetch and decode one 16-bit compressed or 32-bit instruction at `pc`.
    pub(crate) fn fetch(&self, bus: &mut impl Bus, pc: u64) -> Result<(Inst, u32, u64), Trap> {
        let pa0 = self.translate_with(bus, pc, Access::Fetch)?;
        let lo = bus.read(pa0, 2).map_err(|_| Trap::FetchFault(pc))? as u16;
        if lo & 3 != 3 {
            let word = u32::from(lo);
            let inst = decode_c(lo).ok_or(Trap::IllegalInstruction(word))?;
            Ok((inst, word, 2))
        } else {
            let pc2 = pc.wrapping_add(2);
            let pa1 = self.translate_with(bus, pc2, Access::Fetch)?;
            let hi = bus.read(pa1, 2).map_err(|_| Trap::FetchFault(pc2))? as u16;
            let word = u32::from(lo) | (u32::from(hi) << 16);
            let inst = decode(word).ok_or(Trap::IllegalInstruction(word))?;
            Ok((inst, word, 4))
        }
    }

    /// Fetch and decode one instruction at `pc` with a known contiguous `pa0`.
    pub(crate) fn fetch_at_pa(
        &self,
        bus: &mut impl Bus,
        pc: u64,
        pa0: u64,
    ) -> Result<(Inst, u32, u64), Trap> {
        let lo = bus.read(pa0, 2).map_err(|_| Trap::FetchFault(pc))? as u16;
        if lo & 3 != 3 {
            let word = u32::from(lo);
            let inst = decode_c(lo).ok_or(Trap::IllegalInstruction(word))?;
            Ok((inst, word, 2))
        } else {
            let pc2 = pc.wrapping_add(2);
            let pa1 = pa0.wrapping_add(2);
            let hi = bus.read(pa1, 2).map_err(|_| Trap::FetchFault(pc2))? as u16;
            let word = u32::from(lo) | (u32::from(hi) << 16);
            let inst = decode(word).ok_or(Trap::IllegalInstruction(word))?;
            Ok((inst, word, 4))
        }
    }

    /// Write register `rd`; writes to `x0` are discarded.
    fn set(&mut self, rd: u8, value: u64) {
        let i = usize::from(rd & 31);
        if i != 0 {
            if let Some(slot) = self.x.get_mut(i) {
                *slot = value;
            }
        }
    }

    /// Compute the effect of `inst` without changing any state.
    fn execute(&self, inst: Inst, word: u32, next: u64, bus: &mut impl Bus) -> Outcome {
        let pc = self.pc;
        let illegal = Trap::IllegalInstruction(word);
        let virt_inst = Trap::VirtualInstruction(word);
        match inst {
            Inst::Jal { rd, imm } => jump(pc.wrapping_add(imm as u64), next, rd),
            Inst::Jalr { rd, rs1, imm } => {
                jump(self.reg(rs1).wrapping_add(imm as u64) & !1, next, rd)
            }
            Inst::Beq { rs1, rs2, imm } => branch(pc, next, self.reg(rs1) == self.reg(rs2), imm),
            Inst::Bne { rs1, rs2, imm } => branch(pc, next, self.reg(rs1) != self.reg(rs2), imm),
            Inst::Blt { rs1, rs2, imm } => branch(
                pc,
                next,
                (self.reg(rs1) as i64) < (self.reg(rs2) as i64),
                imm,
            ),
            Inst::Bge { rs1, rs2, imm } => branch(
                pc,
                next,
                (self.reg(rs1) as i64) >= (self.reg(rs2) as i64),
                imm,
            ),
            Inst::Bltu { rs1, rs2, imm } => branch(pc, next, self.reg(rs1) < self.reg(rs2), imm),
            Inst::Bgeu { rs1, rs2, imm } => branch(pc, next, self.reg(rs1) >= self.reg(rs2), imm),
            Inst::Lb { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 1, true)?))
            }
            Inst::Lh { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 2, true)?))
            }
            Inst::Lw { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 4, true)?))
            }
            Inst::Ld { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 8, true)?))
            }
            Inst::Lbu { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 1, false)?))
            }
            Inst::Lhu { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 2, false)?))
            }
            Inst::Lwu { rd, rs1, imm } => {
                Ok(Effect::wb(next, rd, self.load(bus, rs1, imm, 4, false)?))
            }
            Inst::Sb { rs1, rs2, imm } => {
                self.store(bus, rs1, rs2, imm, 1).map(|()| Effect::at(next))
            }
            Inst::Sh { rs1, rs2, imm } => {
                self.store(bus, rs1, rs2, imm, 2).map(|()| Effect::at(next))
            }
            Inst::Sw { rs1, rs2, imm } => {
                self.store(bus, rs1, rs2, imm, 4).map(|()| Effect::at(next))
            }
            Inst::Sd { rs1, rs2, imm } => {
                self.store(bus, rs1, rs2, imm, 8).map(|()| Effect::at(next))
            }
            Inst::Hlv {
                rd,
                rs1,
                width,
                unsigned,
                exec,
            } => Ok(Effect::wb(
                next,
                rd,
                self.hlv(bus, word, rs1, width, unsigned, exec)?,
            )),
            Inst::Hsv { rs1, rs2, width } => self
                .hsv(bus, word, rs1, rs2, width)
                .map(|()| Effect::at(next)),
            Inst::Ecall => Err(Trap::Ecall),
            Inst::Ebreak => Err(Trap::Ebreak),
            Inst::Fence | Inst::FenceI => Ok(Effect::at(next)),
            Inst::Csrrw { rd, rs1, csr } => {
                self.csr_op(word, rd, csr, self.reg(rs1), Op::Replace, true)
            }
            Inst::Csrrs { rd, rs1, csr } => {
                self.csr_op(word, rd, csr, self.reg(rs1), Op::Set, rs1 != 0)
            }
            Inst::Csrrc { rd, rs1, csr } => {
                self.csr_op(word, rd, csr, self.reg(rs1), Op::Clear, rs1 != 0)
            }
            Inst::Csrrwi { rd, uimm, csr } => {
                self.csr_op(word, rd, csr, u64::from(uimm & 31), Op::Replace, true)
            }
            Inst::Csrrsi { rd, uimm, csr } => {
                self.csr_op(word, rd, csr, u64::from(uimm & 31), Op::Set, uimm & 31 != 0)
            }
            Inst::Csrrci { rd, uimm, csr } => self.csr_op(
                word,
                rd,
                csr,
                u64::from(uimm & 31),
                Op::Clear,
                uimm & 31 != 0,
            ),
            Inst::LrW { rd, rs1, .. } => {
                let (val, pa) = self.lr(bus, rs1, 4)?;
                Ok(Effect {
                    reservation: Some(Some(pa)),
                    ..Effect::wb(next, rd, val)
                })
            }
            Inst::LrD { rd, rs1, .. } => {
                let (val, pa) = self.lr(bus, rs1, 8)?;
                Ok(Effect {
                    reservation: Some(Some(pa)),
                    ..Effect::wb(next, rd, val)
                })
            }
            Inst::ScW { rd, rs1, rs2, .. } => {
                let val = self.sc(bus, rs1, rs2, 4)?;
                Ok(Effect {
                    reservation: Some(None),
                    ..Effect::wb(next, rd, val)
                })
            }
            Inst::ScD { rd, rs1, rs2, .. } => {
                let val = self.sc(bus, rs1, rs2, 8)?;
                Ok(Effect {
                    reservation: Some(None),
                    ..Effect::wb(next, rd, val)
                })
            }
            Inst::AmoW {
                op, rd, rs1, rs2, ..
            } => Ok(Effect::wb(next, rd, self.amo(bus, op, rs1, rs2, 4)?)),
            Inst::AmoD {
                op, rd, rs1, rs2, ..
            } => Ok(Effect::wb(next, rd, self.amo(bus, op, rs1, rs2, 8)?)),
            Inst::Mret => self.mret(word),
            Inst::Sret => self.sret(word),
            Inst::Wfi => {
                if (self.mode != Mode::Machine && (self.csr.mstatus & TW) != 0)
                    || (!self.virt && self.mode == Mode::User)
                {
                    return Err(illegal);
                }
                if self.virt && (self.mode == Mode::User || (self.csr.hstatus & VTW) != 0) {
                    return Err(virt_inst);
                }
                Ok(Effect {
                    wfi: true,
                    ..Effect::at(next)
                })
            }
            Inst::SfenceVma { .. } => {
                if self.virt {
                    if self.mode == Mode::User || (self.csr.hstatus & VTVM) != 0 {
                        return Err(virt_inst);
                    }
                } else if self.mode == Mode::User
                    || (self.mode == Mode::Supervisor && (self.csr.mstatus & TVM) != 0)
                {
                    return Err(illegal);
                }
                Ok(Effect {
                    sfence: true,
                    ..Effect::at(next)
                })
            }
            Inst::HfenceVvma { .. } => {
                if self.virt {
                    return Err(virt_inst);
                }
                if self.mode == Mode::User {
                    return Err(illegal);
                }
                Ok(Effect {
                    sfence: true,
                    ..Effect::at(next)
                })
            }
            Inst::HfenceGvma { .. } => {
                if self.virt {
                    return Err(virt_inst);
                }
                if self.mode == Mode::User
                    || (self.mode == Mode::Supervisor && (self.csr.mstatus & TVM) != 0)
                {
                    return Err(illegal);
                }
                Ok(Effect {
                    sfence: true,
                    ..Effect::at(next)
                })
            }
            _ => Ok(Effect {
                rd: self.alu(inst, pc),
                ..Effect::at(next)
            }),
        }
    }

    /// CSR read-modify-write. The write happens only when `writes` is set (CSRRW always, the
    /// set and clear forms only with a non-zero source field).
    fn csr_op(&self, word: u32, rd: u8, csr: u16, src: u64, op: Op, writes: bool) -> Outcome {
        let eff_csr = self.csr_check(csr, writes, word)?;
        let old = self
            .csr_read(eff_csr)
            .ok_or(Trap::IllegalInstruction(word))?;
        let new = match op {
            Op::Replace => src,
            Op::Set => old | src,
            Op::Clear => old & !src,
        };
        Ok(Effect {
            rd: Some((rd, old)),
            csr: writes.then_some((eff_csr, new)),
            ..Effect::at(self.pc.wrapping_add(4))
        })
    }

    /// MRET: return to MPP, with MIE taken from MPIE and virt from MPV; pc goes to mepc.
    fn mret(&self, word: u32) -> Outcome {
        if self.mode != Mode::Machine || self.virt {
            return Err(Trap::IllegalInstruction(word));
        }
        let st = self.csr.mstatus;
        let to = Mode::from_field(st >> 11);
        let to_virt = to != Mode::Machine && (st & MPV) != 0;
        let mut new = (st & !(MIE | MPIE | MPP | MPV)) | ((st & MPIE) >> 4) | MPIE;
        if to != Mode::Machine {
            new &= !MPRV;
        }
        Ok(Effect {
            mode: Some(to),
            virt: Some(to_virt),
            csr: Some((0x300, new)),
            ..Effect::at(self.csr.mepc)
        })
    }

    /// SRET: return from HS (using mstatus/hstatus.SPV/sepc) or VS (using vsstatus/vsepc).
    fn sret(&self, word: u32) -> Outcome {
        if self.virt {
            if self.mode == Mode::User || (self.csr.hstatus & VTSR) != 0 {
                return Err(Trap::VirtualInstruction(word));
            }
            let vs = self.csr.vsstatus;
            let to = if vs & SPP != 0 {
                Mode::Supervisor
            } else {
                Mode::User
            };
            let new = (vs & !(SIE | SPIE | SPP)) | ((vs & SPIE) >> 4) | SPIE;
            return Ok(Effect {
                mode: Some(to),
                virt: Some(true),
                csr: Some((0x200, new)),
                ..Effect::at(self.csr.vsepc)
            });
        }
        let st = self.csr.mstatus;
        if self.mode == Mode::User || (self.mode == Mode::Supervisor && (st & TSR) != 0) {
            return Err(Trap::IllegalInstruction(word));
        }
        let to = if st & SPP != 0 {
            Mode::Supervisor
        } else {
            Mode::User
        };
        let to_virt = (self.csr.hstatus & SPV) != 0;
        let new = (st & !(SIE | SPIE | SPP)) | ((st & SPIE) >> 4) | SPIE;
        Ok(Effect {
            mode: Some(to),
            virt: Some(to_virt),
            csr: Some((0x300, new)),
            csr2: Some((0x600, self.csr.hstatus & !SPV)),
            ..Effect::at(self.csr.sepc)
        })
    }

    /// Load `size` bytes from `rs1 + imm`, extended to 64 bits.
    fn load(
        &self,
        bus: &mut impl Bus,
        rs1: u8,
        imm: i64,
        size: u8,
        signed: bool,
    ) -> Result<u64, Trap> {
        let addr = self.reg(rs1).wrapping_add(imm as u64);
        if addr & (u64::from(size) - 1) == 0 {
            let pa = self.translate_with(bus, addr, Access::Load)?;
            let raw = bus.read(pa, size).map_err(|_| Trap::LoadFault(addr))?;
            return Ok(extend(raw, size, signed));
        }
        let mut pas = [0u64; 8];
        for (i, pa_slot) in pas.iter_mut().take(usize::from(size)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            *pa_slot = self.translate_with(bus, va, Access::Load)?;
        }
        let mut raw = 0u64;
        for (i, &pa) in pas.iter().take(usize::from(size)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            let byte = bus.read(pa, 1).map_err(|_| Trap::LoadFault(va))?;
            raw |= (byte & 0xff) << (8 * i);
        }
        Ok(extend(raw, size, signed))
    }

    /// Store the low `size` bytes of `rs2` at `rs1 + imm`.
    fn store(&self, bus: &mut impl Bus, rs1: u8, rs2: u8, imm: i64, size: u8) -> Result<(), Trap> {
        let addr = self.reg(rs1).wrapping_add(imm as u64);
        if addr & (u64::from(size) - 1) == 0 {
            let pa = self.translate_with(bus, addr, Access::Store)?;
            return bus
                .write(pa, size, self.reg(rs2))
                .map_err(|_| Trap::StoreFault(addr));
        }
        let mut pas = [0u64; 8];
        for (i, pa_slot) in pas.iter_mut().take(usize::from(size)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            *pa_slot = self.translate_with(bus, va, Access::Store)?;
        }
        let val = self.reg(rs2);
        for (i, &pa) in pas.iter().take(usize::from(size)).enumerate() {
            let va = addr.wrapping_add(i as u64);
            let byte = (val >> (8 * i)) & 0xff;
            bus.write(pa, 1, byte).map_err(|_| Trap::StoreFault(va))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::extend;

    #[test]
    fn extend_signs_and_zeroes() {
        assert_eq!(extend(0x80, 1, true), u64::MAX - 0x7f);
        assert_eq!(extend(0x80, 1, false), 0x80);
        assert_eq!(extend(0xffff_ffff, 4, true), u64::MAX);
    }
}
