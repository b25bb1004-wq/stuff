//! Atomic memory operations (RV64A): LR.W/D, SC.W/D and AMO*.W/D.
use crate::alu::sext32;
use crate::bus::Bus;
use crate::inst::AmoOp;
use crate::{Access, Cpu, Trap};

fn amo32(op: AmoOp, a: u32, b: u32) -> u32 {
    match op {
        AmoOp::Swap => b,
        AmoOp::Add => a.wrapping_add(b),
        AmoOp::Xor => a ^ b,
        AmoOp::And => a & b,
        AmoOp::Or => a | b,
        AmoOp::Min => {
            if (a as i32) < (b as i32) {
                a
            } else {
                b
            }
        }
        AmoOp::Max => {
            if (a as i32) > (b as i32) {
                a
            } else {
                b
            }
        }
        AmoOp::Minu => a.min(b),
        AmoOp::Maxu => a.max(b),
    }
}

fn amo64(op: AmoOp, a: u64, b: u64) -> u64 {
    match op {
        AmoOp::Swap => b,
        AmoOp::Add => a.wrapping_add(b),
        AmoOp::Xor => a ^ b,
        AmoOp::And => a & b,
        AmoOp::Or => a | b,
        AmoOp::Min => {
            if (a as i64) < (b as i64) {
                a
            } else {
                b
            }
        }
        AmoOp::Max => {
            if (a as i64) > (b as i64) {
                a
            } else {
                b
            }
        }
        AmoOp::Minu => a.min(b),
        AmoOp::Maxu => a.max(b),
    }
}

impl Cpu {
    /// Load-reserved of `size` bytes (4 or 8) from `rs1`. Returns `(value, physical_addr)`.
    pub(crate) fn lr(&self, bus: &mut impl Bus, rs1: u8, size: u8) -> Result<(u64, u64), Trap> {
        let addr = self.reg(rs1);
        if addr & (u64::from(size) - 1) != 0 {
            return Err(Trap::LoadMisaligned(addr));
        }
        let pa = self.translate_with(bus, addr, Access::Load)?;
        let raw = bus.read(pa, size).map_err(|_| Trap::LoadFault(addr))?;
        let val = if size == 4 { sext32(raw as u32) } else { raw };
        Ok((val, pa))
    }

    /// Store-conditional of `size` bytes (4 or 8) from `rs2` to `rs1`. Returns 0 on success, 1 on failure.
    pub(crate) fn sc(&self, bus: &mut impl Bus, rs1: u8, rs2: u8, size: u8) -> Result<u64, Trap> {
        let addr = self.reg(rs1);
        if addr & (u64::from(size) - 1) != 0 {
            return Err(Trap::StoreMisaligned(addr));
        }
        let pa = self.translate_with(bus, addr, Access::Store)?;
        if self.reservation == Some(pa) {
            bus.write(pa, size, self.reg(rs2))
                .map_err(|_| Trap::StoreFault(addr))?;
            Ok(0)
        } else {
            Ok(1)
        }
    }

    /// Atomic read-modify-write of `size` bytes (4 or 8) at `rs1` with `rs2`. Returns old value.
    pub(crate) fn amo(
        &self,
        bus: &mut impl Bus,
        op: AmoOp,
        rs1: u8,
        rs2: u8,
        size: u8,
    ) -> Result<u64, Trap> {
        let addr = self.reg(rs1);
        if addr & (u64::from(size) - 1) != 0 {
            return Err(Trap::StoreMisaligned(addr));
        }
        let pa = self.translate_with(bus, addr, Access::Store)?;
        let raw = bus.read(pa, size).map_err(|_| Trap::StoreFault(addr))?;
        let b = self.reg(rs2);
        let (rd_val, write_val) = if size == 4 {
            let a = raw as u32;
            (sext32(a), u64::from(amo32(op, a, b as u32)))
        } else {
            (raw, amo64(op, raw, b))
        };
        bus.write(pa, size, write_val)
            .map_err(|_| Trap::StoreFault(addr))?;
        Ok(rd_val)
    }
}
