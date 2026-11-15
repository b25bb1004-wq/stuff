//! Register and immediate arithmetic: the integer ALU, the W forms and the M extension.
use crate::inst::Inst;
use crate::Cpu;

/// Sign-extend a 32-bit result to 64 bits. Every W-form result goes through this.
pub(crate) fn sext32(v: u32) -> u64 {
    i64::from(v as i32) as u64
}

fn div_s(a: u64, b: u64) -> u64 {
    if b == 0 {
        u64::MAX
    } else {
        (a as i64).wrapping_div(b as i64) as u64
    }
}

fn div_u(a: u64, b: u64) -> u64 {
    a.checked_div(b).unwrap_or(u64::MAX)
}

fn rem_s(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        (a as i64).wrapping_rem(b as i64) as u64
    }
}

fn rem_u(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        a % b
    }
}

fn mulh(a: u64, b: u64) -> u64 {
    ((i128::from(a as i64) * i128::from(b as i64)) >> 64) as u64
}

fn mulhsu(a: u64, b: u64) -> u64 {
    ((i128::from(a as i64) * i128::from(b)) >> 64) as u64
}

fn mulhu(a: u64, b: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) >> 64) as u64
}

fn divw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as u32 as i32, b as u32 as i32);
    if b == 0 {
        u64::MAX
    } else {
        sext32(a.wrapping_div(b) as u32)
    }
}

fn divuw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as u32, b as u32);
    a.checked_div(b).map_or(u64::MAX, sext32)
}

fn remw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as u32 as i32, b as u32 as i32);
    if b == 0 {
        sext32(a as u32)
    } else {
        sext32(a.wrapping_rem(b) as u32)
    }
}

fn remuw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as u32, b as u32);
    if b == 0 {
        sext32(a)
    } else {
        sext32(a % b)
    }
}

impl Cpu {
    /// Read register `i`; `x0` reads as zero.
    pub(crate) fn reg(&self, i: u8) -> u64 {
        self.x.get(usize::from(i & 31)).copied().unwrap_or(0)
    }

    /// Apply `f` to two registers.
    fn bin(&self, rs1: u8, rs2: u8, f: fn(u64, u64) -> u64) -> u64 {
        f(self.reg(rs1), self.reg(rs2))
    }

    /// Destination register and value for an arithmetic instruction; `None` for anything else.
    pub(crate) fn alu(&self, inst: Inst, pc: u64) -> Option<(u8, u64)> {
        let (rd, v) = match inst {
            Inst::Lui { rd, imm } => (rd, imm as u64),
            Inst::Auipc { rd, imm } => (rd, pc.wrapping_add(imm as u64)),
            Inst::Addi { rd, rs1, imm } => (rd, self.reg(rs1).wrapping_add(imm as u64)),
            Inst::Slti { rd, rs1, imm } => (rd, u64::from((self.reg(rs1) as i64) < imm)),
            Inst::Sltiu { rd, rs1, imm } => (rd, u64::from(self.reg(rs1) < imm as u64)),
            Inst::Xori { rd, rs1, imm } => (rd, self.reg(rs1) ^ imm as u64),
            Inst::Ori { rd, rs1, imm } => (rd, self.reg(rs1) | imm as u64),
            Inst::Andi { rd, rs1, imm } => (rd, self.reg(rs1) & imm as u64),
            Inst::Slli { rd, rs1, imm } => (rd, self.reg(rs1) << (imm as u64 & 63)),
            Inst::Srli { rd, rs1, imm } => (rd, self.reg(rs1) >> (imm as u64 & 63)),
            Inst::Srai { rd, rs1, imm } => {
                (rd, ((self.reg(rs1) as i64) >> (imm as u64 & 63)) as u64)
            }
            Inst::Addiw { rd, rs1, imm } => {
                (rd, sext32((self.reg(rs1) as u32).wrapping_add(imm as u32)))
            }
            Inst::Slliw { rd, rs1, imm } => {
                (rd, sext32((self.reg(rs1) as u32) << (imm as u32 & 31)))
            }
            Inst::Srliw { rd, rs1, imm } => {
                (rd, sext32((self.reg(rs1) as u32) >> (imm as u32 & 31)))
            }
            Inst::Sraiw { rd, rs1, imm } => (
                rd,
                sext32(((self.reg(rs1) as u32 as i32) >> (imm as u32 & 31)) as u32),
            ),
            Inst::Add { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, u64::wrapping_add)),
            Inst::Sub { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, u64::wrapping_sub)),
            Inst::Sll { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, |a, b| a << (b & 63))),
            Inst::Slt { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| u64::from((a as i64) < (b as i64))),
            ),
            Inst::Sltu { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, |a, b| u64::from(a < b))),
            Inst::Xor { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, |a, b| a ^ b)),
            Inst::Srl { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, |a, b| a >> (b & 63))),
            Inst::Sra { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| ((a as i64) >> (b & 63)) as u64),
            ),
            Inst::Or { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, |a, b| a | b)),
            Inst::And { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, |a, b| a & b)),
            Inst::Addw { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| sext32((a as u32).wrapping_add(b as u32))),
            ),
            Inst::Subw { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| sext32((a as u32).wrapping_sub(b as u32))),
            ),
            Inst::Sllw { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| sext32((a as u32) << (b & 31))),
            ),
            Inst::Srlw { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| sext32((a as u32) >> (b & 31))),
            ),
            Inst::Sraw { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| {
                    sext32(((a as u32 as i32) >> (b & 31)) as u32)
                }),
            ),
            Inst::Mul { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, u64::wrapping_mul)),
            Inst::Mulh { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, mulh)),
            Inst::Mulhsu { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, mulhsu)),
            Inst::Mulhu { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, mulhu)),
            Inst::Div { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, div_s)),
            Inst::Divu { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, div_u)),
            Inst::Rem { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, rem_s)),
            Inst::Remu { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, rem_u)),
            Inst::Mulw { rd, rs1, rs2 } => (
                rd,
                self.bin(rs1, rs2, |a, b| sext32((a as u32).wrapping_mul(b as u32))),
            ),
            Inst::Divw { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, divw)),
            Inst::Divuw { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, divuw)),
            Inst::Remw { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, remw)),
            Inst::Remuw { rd, rs1, rs2 } => (rd, self.bin(rs1, rs2, remuw)),
            _ => return None,
        };
        Some((rd, v))
    }
}
