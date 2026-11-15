//! Encoder for explicit RV64C (`c.*`) 16-bit instructions.

use crate::expr::Expr;
use crate::expr_eval::{eval_expr, EvalCtx, EvalVal};
use crate::AsmError;

/// Parsed compressed instruction ready for Pass 2 evaluation.
#[derive(Debug, Clone)]
pub(crate) enum CInst {
    Addi4spn {
        rd_p: u8,
        imm: Expr,
    },
    Lw {
        rd_p: u8,
        rs1_p: u8,
        imm: Option<Expr>,
    },
    Ld {
        rd_p: u8,
        rs1_p: u8,
        imm: Option<Expr>,
    },
    Sw {
        rs2_p: u8,
        rs1_p: u8,
        imm: Option<Expr>,
    },
    Sd {
        rs2_p: u8,
        rs1_p: u8,
        imm: Option<Expr>,
    },
    Nop,
    Addi {
        rd: u8,
        imm: Expr,
    },
    Addiw {
        rd: u8,
        imm: Expr,
    },
    Li {
        rd: u8,
        imm: Expr,
    },
    Addi16sp {
        imm: Expr,
    },
    Lui {
        rd: u8,
        imm: Expr,
    },
    Srli {
        rd_p: u8,
        shamt: Expr,
    },
    Srai {
        rd_p: u8,
        shamt: Expr,
    },
    Andi {
        rd_p: u8,
        imm: Expr,
    },
    Arith {
        funct6: u16,
        funct2: u16,
        rd_p: u8,
        rs2_p: u8,
    },
    J {
        target: Expr,
    },
    Branch {
        funct3: u16,
        rs1_p: u8,
        target: Expr,
    },
    Slli {
        rd: u8,
        shamt: Expr,
    },
    Lwsp {
        rd: u8,
        imm: Option<Expr>,
    },
    Ldsp {
        rd: u8,
        imm: Option<Expr>,
    },
    Jr {
        rs1: u8,
    },
    Mv {
        rd: u8,
        rs2: u8,
    },
    Ebreak,
    Jalr {
        rs1: u8,
    },
    Add {
        rd: u8,
        rs2: u8,
    },
    Swsp {
        rs2: u8,
        imm: Option<Expr>,
    },
    Sdsp {
        rs2: u8,
        imm: Option<Expr>,
    },
}

fn eval_abs(expr: &Expr, ctx: &EvalCtx<'_>) -> Result<i64, AsmError> {
    let v = eval_expr(expr, ctx)?;
    if v.label_balance != 0 {
        return Err(AsmError::new(
            ctx.line,
            "expected absolute immediate expression",
        ));
    }
    Ok(v.val)
}

fn eval_opt_abs(expr: &Option<Expr>, ctx: &EvalCtx<'_>) -> Result<i64, AsmError> {
    match expr {
        Some(e) => eval_abs(e, ctx),
        None => Ok(0),
    }
}

pub(crate) fn eval_rel_target(expr: &Expr, ctx: &EvalCtx<'_>) -> Result<i64, AsmError> {
    let EvalVal {
        val, label_balance, ..
    } = eval_expr(expr, ctx)?;
    match label_balance {
        0 => Ok(val),
        1 => Ok(val.wrapping_sub(ctx.pc as i64)),
        _ => Err(AsmError::new(
            ctx.line,
            "invalid branch/jump target expression",
        )),
    }
}

fn encode_ci(funct3: u16, rd: u8, imm: i64, op: u16) -> u16 {
    let u = imm as u16;
    (funct3 << 13) | (((u >> 5) & 1) << 12) | (u16::from(rd) << 7) | ((u & 0x1f) << 2) | op
}

fn encode_cl_cs_w(funct3: u16, rs1_p: u8, r_p: u8, u: u16) -> u16 {
    (funct3 << 13)
        | (((u >> 3) & 7) << 10)
        | (u16::from(rs1_p) << 7)
        | (((u >> 2) & 1) << 6)
        | (((u >> 6) & 1) << 5)
        | (u16::from(r_p) << 2)
}

fn encode_cl_cs_d(funct3: u16, rs1_p: u8, r_p: u8, u: u16) -> u16 {
    (funct3 << 13)
        | (((u >> 3) & 7) << 10)
        | (u16::from(rs1_p) << 7)
        | (((u >> 6) & 3) << 5)
        | (u16::from(r_p) << 2)
}

/// Encode a parsed [`CInst`] to its 16-bit compressed halfword.
pub(crate) fn encode_c(inst: &CInst, ctx: &EvalCtx<'_>) -> Result<u16, AsmError> {
    let line = ctx.line;
    match *inst {
        CInst::Addi4spn { rd_p, ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if !(4..=1020).contains(&v) || v % 4 != 0 {
                return Err(AsmError::new(line, "c.addi4spn immediate out of range"));
            }
            let u = v as u16;
            Ok((((u >> 4) & 3) << 11)
                | (((u >> 6) & 0xf) << 7)
                | (((u >> 2) & 1) << 6)
                | (((u >> 3) & 1) << 5)
                | (u16::from(rd_p) << 2))
        }
        CInst::Lw {
            rd_p,
            rs1_p,
            ref imm,
        } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=124).contains(&v) || v % 4 != 0 {
                return Err(AsmError::new(line, "c.lw offset out of range"));
            }
            Ok(encode_cl_cs_w(0b010, rs1_p, rd_p, v as u16))
        }
        CInst::Ld {
            rd_p,
            rs1_p,
            ref imm,
        } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=248).contains(&v) || v % 8 != 0 {
                return Err(AsmError::new(line, "c.ld offset out of range"));
            }
            Ok(encode_cl_cs_d(0b011, rs1_p, rd_p, v as u16))
        }
        CInst::Sw {
            rs2_p,
            rs1_p,
            ref imm,
        } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=124).contains(&v) || v % 4 != 0 {
                return Err(AsmError::new(line, "c.sw offset out of range"));
            }
            Ok(encode_cl_cs_w(0b110, rs1_p, rs2_p, v as u16))
        }
        CInst::Sd {
            rs2_p,
            rs1_p,
            ref imm,
        } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=248).contains(&v) || v % 8 != 0 {
                return Err(AsmError::new(line, "c.sd offset out of range"));
            }
            Ok(encode_cl_cs_d(0b111, rs1_p, rs2_p, v as u16))
        }
        CInst::Nop => Ok(0x0001),
        CInst::Addi { rd, ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if v == 0 || !(-32..=31).contains(&v) {
                return Err(AsmError::new(line, "c.addi immediate out of range"));
            }
            Ok(encode_ci(0b000, rd, v, 0b01))
        }
        CInst::Addiw { rd, ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if !(-32..=31).contains(&v) {
                return Err(AsmError::new(line, "c.addiw immediate out of range"));
            }
            Ok(encode_ci(0b001, rd, v, 0b01))
        }
        CInst::Li { rd, ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if !(-32..=31).contains(&v) {
                return Err(AsmError::new(line, "c.li immediate out of range"));
            }
            Ok(encode_ci(0b010, rd, v, 0b01))
        }
        CInst::Addi16sp { ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if v == 0 || !(-512..=496).contains(&v) || v % 16 != 0 {
                return Err(AsmError::new(line, "c.addi16sp immediate out of range"));
            }
            let u = v as u16;
            Ok((0b011 << 13)
                | (((u >> 9) & 1) << 12)
                | (2 << 7)
                | (((u >> 4) & 1) << 6)
                | (((u >> 6) & 1) << 5)
                | (((u >> 7) & 3) << 3)
                | (((u >> 5) & 1) << 2)
                | 0b01)
        }
        CInst::Lui { rd, ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if !((1..=31).contains(&v) || (0xfffe0..=0xfffff).contains(&v)) {
                return Err(AsmError::new(line, "c.lui immediate out of range"));
            }
            Ok(encode_ci(0b011, rd, v, 0b01))
        }
        CInst::Srli { rd_p, ref shamt } => {
            let v = eval_abs(shamt, ctx)?;
            if !(1..=63).contains(&v) {
                return Err(AsmError::new(line, "c.srli shift amount out of range"));
            }
            let u = v as u16;
            Ok((0b100 << 13)
                | (((u >> 5) & 1) << 12)
                | (u16::from(rd_p) << 7)
                | ((u & 0x1f) << 2)
                | 0b01)
        }
        CInst::Srai { rd_p, ref shamt } => {
            let v = eval_abs(shamt, ctx)?;
            if !(1..=63).contains(&v) {
                return Err(AsmError::new(line, "c.srai shift amount out of range"));
            }
            let u = v as u16;
            Ok((0b100 << 13)
                | (((u >> 5) & 1) << 12)
                | (0b01 << 10)
                | (u16::from(rd_p) << 7)
                | ((u & 0x1f) << 2)
                | 0b01)
        }
        CInst::Andi { rd_p, ref imm } => {
            let v = eval_abs(imm, ctx)?;
            if !(-32..=31).contains(&v) {
                return Err(AsmError::new(line, "c.andi immediate out of range"));
            }
            let u = v as u16;
            Ok((0b100 << 13)
                | (((u >> 5) & 1) << 12)
                | (0b10 << 10)
                | (u16::from(rd_p) << 7)
                | ((u & 0x1f) << 2)
                | 0b01)
        }
        CInst::Arith {
            funct6,
            funct2,
            rd_p,
            rs2_p,
        } => Ok((funct6 << 10)
            | (u16::from(rd_p) << 7)
            | (funct2 << 5)
            | (u16::from(rs2_p) << 2)
            | 0b01),
        CInst::J { ref target } => {
            let off = eval_rel_target(target, ctx)?;
            if !(-2048..=2046).contains(&off) || (off & 1) != 0 {
                return Err(AsmError::new(line, "c.j target out of range or unaligned"));
            }
            let u = off as u16;
            Ok((0b101 << 13)
                | (((u >> 11) & 1) << 12)
                | (((u >> 4) & 1) << 11)
                | (((u >> 8) & 3) << 9)
                | (((u >> 10) & 1) << 8)
                | (((u >> 6) & 1) << 7)
                | (((u >> 7) & 1) << 6)
                | (((u >> 1) & 7) << 3)
                | (((u >> 5) & 1) << 2)
                | 0b01)
        }
        CInst::Branch {
            funct3,
            rs1_p,
            ref target,
        } => {
            let off = eval_rel_target(target, ctx)?;
            if !(-256..=254).contains(&off) || (off & 1) != 0 {
                return Err(AsmError::new(line, "c.beqz/c.bnez target out of range"));
            }
            let u = off as u16;
            Ok((funct3 << 13)
                | (((u >> 8) & 1) << 12)
                | (((u >> 3) & 3) << 10)
                | (u16::from(rs1_p) << 7)
                | (((u >> 6) & 3) << 5)
                | (((u >> 1) & 3) << 3)
                | (((u >> 5) & 1) << 2)
                | 0b01)
        }
        CInst::Slli { rd, ref shamt } => {
            let v = eval_abs(shamt, ctx)?;
            if !(1..=63).contains(&v) {
                return Err(AsmError::new(line, "c.slli shift amount out of range"));
            }
            Ok(encode_ci(0b000, rd, v, 0b10))
        }
        CInst::Lwsp { rd, ref imm } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=252).contains(&v) || v % 4 != 0 {
                return Err(AsmError::new(line, "c.lwsp offset out of range"));
            }
            let u = v as u16;
            Ok((0b010 << 13)
                | (((u >> 5) & 1) << 12)
                | (u16::from(rd) << 7)
                | (((u >> 2) & 7) << 4)
                | (((u >> 6) & 3) << 2)
                | 0b10)
        }
        CInst::Ldsp { rd, ref imm } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=504).contains(&v) || v % 8 != 0 {
                return Err(AsmError::new(line, "c.ldsp offset out of range"));
            }
            let u = v as u16;
            Ok((0b011 << 13)
                | (((u >> 5) & 1) << 12)
                | (u16::from(rd) << 7)
                | (((u >> 3) & 3) << 5)
                | (((u >> 6) & 7) << 2)
                | 0b10)
        }
        CInst::Jr { rs1 } => Ok((0b1000 << 12) | (u16::from(rs1) << 7) | 0b10),
        CInst::Mv { rd, rs2 } => {
            Ok((0b1000 << 12) | (u16::from(rd) << 7) | (u16::from(rs2) << 2) | 0b10)
        }
        CInst::Ebreak => Ok(0x9002),
        CInst::Jalr { rs1 } => Ok((0b1001 << 12) | (u16::from(rs1) << 7) | 0b10),
        CInst::Add { rd, rs2 } => {
            Ok((0b1001 << 12) | (u16::from(rd) << 7) | (u16::from(rs2) << 2) | 0b10)
        }
        CInst::Swsp { rs2, ref imm } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=252).contains(&v) || v % 4 != 0 {
                return Err(AsmError::new(line, "c.swsp offset out of range"));
            }
            let u = v as u16;
            Ok((0b110 << 13)
                | (((u >> 2) & 0xf) << 9)
                | (((u >> 6) & 3) << 7)
                | (u16::from(rs2) << 2)
                | 0b10)
        }
        CInst::Sdsp { rs2, ref imm } => {
            let v = eval_opt_abs(imm, ctx)?;
            if !(0..=504).contains(&v) || v % 8 != 0 {
                return Err(AsmError::new(line, "c.sdsp offset out of range"));
            }
            let u = v as u16;
            Ok((0b111 << 13)
                | (((u >> 3) & 7) << 10)
                | (((u >> 6) & 7) << 7)
                | (u16::from(rs2) << 2)
                | 0b10)
        }
    }
}
