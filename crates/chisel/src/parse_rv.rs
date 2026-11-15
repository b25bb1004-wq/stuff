//! Parser for RV64IMA, Zifencei, privileged, and basic ALU pseudo instructions.

use silicon::inst::{encode, AmoOp, Inst};

use crate::encode_rv::{Imm12Kind, RvInst, ShiftKind};
use crate::expr::{parse_expr, Expr};
use crate::expr_eval::EvalCtx;
use crate::parse_rv_ctrl::parse_rv_ctrl_csr;
use crate::regs::{parse_fence_set, parse_mem_operand, parse_reg};
use crate::AsmError;

pub(crate) fn req_count(ops: &[&str], n: usize, mnem: &str, line: usize) -> Result<(), AsmError> {
    if ops.len() != n {
        return Err(AsmError::new(
            line,
            format!("'{mnem}' expects {n} operand(s), got {}", ops.len()),
        ));
    }
    Ok(())
}

pub(crate) fn reg(s: &str, line: usize) -> Result<u8, AsmError> {
    parse_reg(s).ok_or_else(|| AsmError::new(line, format!("unknown register '{s}'")))
}

fn parse_amo(mnem: &str, ops: &[&str], line: usize) -> Result<Option<RvInst>, AsmError> {
    let (base, aq, rl) = if let Some(b) = mnem.strip_suffix(".aqrl") {
        (b, true, true)
    } else if let Some(b) = mnem.strip_suffix(".aq") {
        (b, true, false)
    } else if let Some(b) = mnem.strip_suffix(".rl") {
        (b, false, true)
    } else {
        (mnem, false, false)
    };
    let (op_name, is_d) = if let Some(p) = base.strip_suffix(".w") {
        (p, false)
    } else if let Some(p) = base.strip_suffix(".d") {
        (p, true)
    } else {
        return Ok(None);
    };

    if op_name == "lr" {
        req_count(ops, 2, mnem, line)?;
        let rd = reg(ops[0], line)?;
        let (imm, rs1) = parse_mem_operand(ops[1], line)?;
        let inst = if is_d {
            Inst::LrD { rd, rs1, aq, rl }
        } else {
            Inst::LrW { rd, rs1, aq, rl }
        };
        return Ok(Some(RvInst::Amo {
            word: encode(inst),
            imm,
        }));
    }
    if op_name == "sc" {
        req_count(ops, 3, mnem, line)?;
        let rd = reg(ops[0], line)?;
        let rs2 = reg(ops[1], line)?;
        let (imm, rs1) = parse_mem_operand(ops[2], line)?;
        let inst = if is_d {
            Inst::ScD {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            }
        } else {
            Inst::ScW {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            }
        };
        return Ok(Some(RvInst::Amo {
            word: encode(inst),
            imm,
        }));
    }
    let amo_op = match op_name {
        "amoswap" => AmoOp::Swap,
        "amoadd" => AmoOp::Add,
        "amoxor" => AmoOp::Xor,
        "amoand" => AmoOp::And,
        "amoor" => AmoOp::Or,
        "amomin" => AmoOp::Min,
        "amomax" => AmoOp::Max,
        "amominu" => AmoOp::Minu,
        "amomaxu" => AmoOp::Maxu,
        _ => return Ok(None),
    };
    req_count(ops, 3, mnem, line)?;
    let rd = reg(ops[0], line)?;
    let rs2 = reg(ops[1], line)?;
    let (imm, rs1) = parse_mem_operand(ops[2], line)?;
    let inst = if is_d {
        Inst::AmoD {
            op: amo_op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        }
    } else {
        Inst::AmoW {
            op: amo_op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        }
    };
    Ok(Some(RvInst::Amo {
        word: encode(inst),
        imm,
    }))
}

fn r_type(mnem: &str, rd: u8, rs1: u8, rs2: u8) -> Option<Inst> {
    Some(match mnem {
        "add" => Inst::Add { rd, rs1, rs2 },
        "sub" => Inst::Sub { rd, rs1, rs2 },
        "sll" => Inst::Sll { rd, rs1, rs2 },
        "slt" => Inst::Slt { rd, rs1, rs2 },
        "sltu" => Inst::Sltu { rd, rs1, rs2 },
        "xor" => Inst::Xor { rd, rs1, rs2 },
        "srl" => Inst::Srl { rd, rs1, rs2 },
        "sra" => Inst::Sra { rd, rs1, rs2 },
        "or" => Inst::Or { rd, rs1, rs2 },
        "and" => Inst::And { rd, rs1, rs2 },
        "addw" => Inst::Addw { rd, rs1, rs2 },
        "subw" => Inst::Subw { rd, rs1, rs2 },
        "sllw" => Inst::Sllw { rd, rs1, rs2 },
        "srlw" => Inst::Srlw { rd, rs1, rs2 },
        "sraw" => Inst::Sraw { rd, rs1, rs2 },
        "mul" => Inst::Mul { rd, rs1, rs2 },
        "mulh" => Inst::Mulh { rd, rs1, rs2 },
        "mulhsu" => Inst::Mulhsu { rd, rs1, rs2 },
        "mulhu" => Inst::Mulhu { rd, rs1, rs2 },
        "div" => Inst::Div { rd, rs1, rs2 },
        "divu" => Inst::Divu { rd, rs1, rs2 },
        "rem" => Inst::Rem { rd, rs1, rs2 },
        "remu" => Inst::Remu { rd, rs1, rs2 },
        "mulw" => Inst::Mulw { rd, rs1, rs2 },
        "divw" => Inst::Divw { rd, rs1, rs2 },
        "divuw" => Inst::Divuw { rd, rs1, rs2 },
        "remw" => Inst::Remw { rd, rs1, rs2 },
        "remuw" => Inst::Remuw { rd, rs1, rs2 },
        _ => return None,
    })
}

fn parse_fixed_or_pseudo(
    mnem: &str,
    ops: &[&str],
    line: usize,
) -> Result<Option<RvInst>, AsmError> {
    let w = match mnem {
        "nop" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Addi {
                rd: 0,
                rs1: 0,
                imm: 0,
            })
        }
        "ecall" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Ecall)
        }
        "ebreak" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Ebreak)
        }
        "mret" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Mret)
        }
        "sret" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Sret)
        }
        "wfi" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Wfi)
        }
        "fence.i" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::FenceI)
        }
        "fence" => {
            if ops.is_empty() {
                encode(Inst::Fence)
            } else {
                req_count(ops, 2, mnem, line)?;
                let pred = parse_fence_set(ops[0]).ok_or_else(|| {
                    AsmError::new(line, format!("invalid fence pred '{}'", ops[0]))
                })?;
                let succ = parse_fence_set(ops[1]).ok_or_else(|| {
                    AsmError::new(line, format!("invalid fence succ '{}'", ops[1]))
                })?;
                (pred << 24) | (succ << 20) | 0x0f
            }
        }
        "sfence.vma" => {
            let (rs1, rs2) = match ops.len() {
                0 => (0, 0),
                1 => (reg(ops[0], line)?, 0),
                2 => (reg(ops[0], line)?, reg(ops[1], line)?),
                _ => {
                    return Err(AsmError::new(
                        line,
                        format!("'sfence.vma' expects 0..2 operands, got {}", ops.len()),
                    ));
                }
            };
            encode(Inst::SfenceVma { rs1, rs2 })
        }
        "ret" => {
            req_count(ops, 0, mnem, line)?;
            encode(Inst::Jalr {
                rd: 0,
                rs1: 1,
                imm: 0,
            })
        }
        "jr" => {
            req_count(ops, 1, mnem, line)?;
            let rs1 = reg(ops[0], line)?;
            encode(Inst::Jalr { rd: 0, rs1, imm: 0 })
        }
        "mv" | "not" | "neg" | "negw" | "sext.w" | "seqz" | "snez" | "sltz" | "sgtz" => {
            req_count(ops, 2, mnem, line)?;
            let rd = reg(ops[0], line)?;
            let rs = reg(ops[1], line)?;
            let inst = match mnem {
                "mv" => Inst::Addi {
                    rd,
                    rs1: rs,
                    imm: 0,
                },
                "not" => Inst::Xori {
                    rd,
                    rs1: rs,
                    imm: -1,
                },
                "neg" => Inst::Sub {
                    rd,
                    rs1: 0,
                    rs2: rs,
                },
                "negw" => Inst::Subw {
                    rd,
                    rs1: 0,
                    rs2: rs,
                },
                "sext.w" => Inst::Addiw {
                    rd,
                    rs1: rs,
                    imm: 0,
                },
                "seqz" => Inst::Sltiu {
                    rd,
                    rs1: rs,
                    imm: 1,
                },
                "snez" => Inst::Sltu {
                    rd,
                    rs1: 0,
                    rs2: rs,
                },
                "sltz" => Inst::Slt {
                    rd,
                    rs1: rs,
                    rs2: 0,
                },
                _ => Inst::Slt {
                    rd,
                    rs1: 0,
                    rs2: rs,
                },
            };
            encode(inst)
        }
        _ => return Ok(None),
    };
    Ok(Some(RvInst::Fixed(w)))
}

/// Parse any 32-bit or pseudo instruction (excluding `c.*`).
pub(crate) fn parse_rv(mnem: &str, ops: &[&str], ctx: &EvalCtx<'_>) -> Result<RvInst, AsmError> {
    let line = ctx.line;
    if let Some(amo) = parse_amo(mnem, ops, line)? {
        return Ok(amo);
    }
    if let Some(fixed) = parse_fixed_or_pseudo(mnem, ops, line)? {
        return Ok(fixed);
    }

    if let Some(kind) = match mnem {
        "addi" => Some(Imm12Kind::Addi),
        "slti" => Some(Imm12Kind::Slti),
        "sltiu" => Some(Imm12Kind::Sltiu),
        "xori" => Some(Imm12Kind::Xori),
        "ori" => Some(Imm12Kind::Ori),
        "andi" => Some(Imm12Kind::Andi),
        "addiw" => Some(Imm12Kind::Addiw),
        _ => None,
    } {
        req_count(ops, 3, mnem, line)?;
        return Ok(RvInst::Imm12 {
            kind,
            r1: reg(ops[0], line)?,
            r2: reg(ops[1], line)?,
            imm: parse_expr(ops[2], line)?,
        });
    }

    if let Some(kind) = match mnem {
        "lb" => Some(Imm12Kind::Lb),
        "lh" => Some(Imm12Kind::Lh),
        "lw" => Some(Imm12Kind::Lw),
        "ld" => Some(Imm12Kind::Ld),
        "lbu" => Some(Imm12Kind::Lbu),
        "lhu" => Some(Imm12Kind::Lhu),
        "lwu" => Some(Imm12Kind::Lwu),
        "sb" => Some(Imm12Kind::Sb),
        "sh" => Some(Imm12Kind::Sh),
        "sw" => Some(Imm12Kind::Sw),
        "sd" => Some(Imm12Kind::Sd),
        _ => None,
    } {
        req_count(ops, 2, mnem, line)?;
        let r1 = reg(ops[0], line)?;
        let (imm, base) = parse_mem_operand(ops[1], line)?;
        return Ok(RvInst::Imm12 {
            kind,
            r1,
            r2: base,
            imm: imm.unwrap_or(Expr::Num(0)),
        });
    }

    if let Some((kind, max)) = match mnem {
        "slli" => Some((ShiftKind::Slli, 63)),
        "srli" => Some((ShiftKind::Srli, 63)),
        "srai" => Some((ShiftKind::Srai, 63)),
        "slliw" => Some((ShiftKind::Slliw, 31)),
        "srliw" => Some((ShiftKind::Srliw, 31)),
        "sraiw" => Some((ShiftKind::Sraiw, 31)),
        _ => None,
    } {
        req_count(ops, 3, mnem, line)?;
        return Ok(RvInst::Shift {
            kind,
            rd: reg(ops[0], line)?,
            rs1: reg(ops[1], line)?,
            shamt: parse_expr(ops[2], line)?,
            max,
        });
    }

    if r_type(mnem, 0, 0, 0).is_some() {
        req_count(ops, 3, mnem, line)?;
        let rd = reg(ops[0], line)?;
        let rs1 = reg(ops[1], line)?;
        let rs2 = reg(ops[2], line)?;
        let inst = r_type(mnem, rd, rs1, rs2).ok_or_else(|| AsmError::new(line, "bad r-type"))?;
        return Ok(RvInst::Fixed(encode(inst)));
    }

    parse_rv_ctrl_csr(mnem, ops, ctx)
}
