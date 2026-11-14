//! Fetch, decode and execute one instruction. Effects are computed first and committed last,
//! so a trap leaves the cpu exactly as it was.
use crate::bus::Bus;
use crate::inst::{decode, Inst};
use crate::{Cpu, Trap};

/// Next pc and an optional register write, produced before anything is committed.
type Outcome = Result<(u64, Option<(u8, u64)>), Trap>;

/// Sign or zero extend the low `size` bytes of `raw`.
fn extend(raw: u64, size: u8, signed: bool) -> u64 {
    let shift = 64 - 8 * u32::from(size);
    if signed {
        (((raw << shift) as i64) >> shift) as u64
    } else {
        (raw << shift) >> shift
    }
}

/// Jump to `target` and write the link value; the target must be a multiple of 4.
fn jump(target: u64, link: u64, rd: u8) -> Outcome {
    check_pc(target)?;
    Ok((target, Some((rd, link))))
}

/// Fail with `FetchMisaligned` unless `target` is a multiple of 4.
fn check_pc(target: u64) -> Result<(), Trap> {
    if target & 3 != 0 {
        Err(Trap::FetchMisaligned(target))
    } else {
        Ok(())
    }
}

/// Conditional branch: fall through, or jump to `pc + imm` when `taken`.
fn branch(pc: u64, taken: bool, imm: i64) -> Outcome {
    if !taken {
        return Ok((pc.wrapping_add(4), None));
    }
    let target = pc.wrapping_add(imm as u64);
    check_pc(target)?;
    Ok((target, None))
}

impl Cpu {
    /// Execute the instruction at `pc`. On `Err`, pc, registers and `instret` are unchanged.
    pub fn step(&mut self, bus: &mut impl Bus) -> Result<(), Trap> {
        let pc = self.pc;
        check_pc(pc)?;
        let word = bus.read(pc, 4).map_err(|_| Trap::FetchFault(pc))? as u32;
        let inst = decode(word).ok_or(Trap::IllegalInstruction(word))?;
        let (next, write) = self.execute(inst, pc, bus)?;
        if let Some((rd, value)) = write {
            self.set(rd, value);
        }
        self.pc = next;
        self.instret = self.instret.wrapping_add(1);
        Ok(())
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
    fn execute(&self, inst: Inst, pc: u64, bus: &mut impl Bus) -> Outcome {
        let next = pc.wrapping_add(4);
        match inst {
            Inst::Jal { rd, imm } => jump(pc.wrapping_add(imm as u64), next, rd),
            Inst::Jalr { rd, rs1, imm } => {
                jump(self.reg(rs1).wrapping_add(imm as u64) & !1, next, rd)
            }
            Inst::Beq { rs1, rs2, imm } => branch(pc, self.reg(rs1) == self.reg(rs2), imm),
            Inst::Bne { rs1, rs2, imm } => branch(pc, self.reg(rs1) != self.reg(rs2), imm),
            Inst::Blt { rs1, rs2, imm } => {
                branch(pc, (self.reg(rs1) as i64) < (self.reg(rs2) as i64), imm)
            }
            Inst::Bge { rs1, rs2, imm } => {
                branch(pc, (self.reg(rs1) as i64) >= (self.reg(rs2) as i64), imm)
            }
            Inst::Bltu { rs1, rs2, imm } => branch(pc, self.reg(rs1) < self.reg(rs2), imm),
            Inst::Bgeu { rs1, rs2, imm } => branch(pc, self.reg(rs1) >= self.reg(rs2), imm),
            Inst::Lb { rd, rs1, imm } => Ok((next, Some((rd, self.load(bus, rs1, imm, 1, true)?)))),
            Inst::Lh { rd, rs1, imm } => Ok((next, Some((rd, self.load(bus, rs1, imm, 2, true)?)))),
            Inst::Lw { rd, rs1, imm } => Ok((next, Some((rd, self.load(bus, rs1, imm, 4, true)?)))),
            Inst::Ld { rd, rs1, imm } => Ok((next, Some((rd, self.load(bus, rs1, imm, 8, true)?)))),
            Inst::Lbu { rd, rs1, imm } => {
                Ok((next, Some((rd, self.load(bus, rs1, imm, 1, false)?))))
            }
            Inst::Lhu { rd, rs1, imm } => {
                Ok((next, Some((rd, self.load(bus, rs1, imm, 2, false)?))))
            }
            Inst::Lwu { rd, rs1, imm } => {
                Ok((next, Some((rd, self.load(bus, rs1, imm, 4, false)?))))
            }
            Inst::Sb { rs1, rs2, imm } => self.store(bus, rs1, rs2, imm, 1).map(|()| (next, None)),
            Inst::Sh { rs1, rs2, imm } => self.store(bus, rs1, rs2, imm, 2).map(|()| (next, None)),
            Inst::Sw { rs1, rs2, imm } => self.store(bus, rs1, rs2, imm, 4).map(|()| (next, None)),
            Inst::Sd { rs1, rs2, imm } => self.store(bus, rs1, rs2, imm, 8).map(|()| (next, None)),
            Inst::Ecall => Err(Trap::Ecall),
            Inst::Ebreak => Err(Trap::Ebreak),
            _ => Ok((next, self.alu(inst, pc))),
        }
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
        let raw = bus.read(addr, size).map_err(|_| Trap::LoadFault(addr))?;
        Ok(extend(raw, size, signed))
    }

    /// Store the low `size` bytes of `rs2` at `rs1 + imm`.
    fn store(&self, bus: &mut impl Bus, rs1: u8, rs2: u8, imm: i64, size: u8) -> Result<(), Trap> {
        let addr = self.reg(rs1).wrapping_add(imm as u64);
        bus.write(addr, size, self.reg(rs2))
            .map_err(|_| Trap::StoreFault(addr))
    }
}
