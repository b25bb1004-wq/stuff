//! Parser for RV64I control-flow, upper-immediate, `li`, and Zicsr instructions.

use crate::encode_rv::{BranchKind, CsrOp, CsrSpec, Imm12Kind, RvInst};
use crate::expr::{parse_expr, Expr};
use crate::expr_eval::{eval_expr, EvalCtx};
use crate::li::synth_li;
use crate::parse_rv::{reg, req_count};
use crate::regs::{parse_csr_name, parse_mem_operand};
use crate::AsmError;

fn csr_spec(s: &str, line: usize) -> Result<CsrSpec, AsmError> {
    if let Some(c) = parse_csr_name(s) {
        Ok(CsrSpec::Fixed(c))
    } else {
        Ok(CsrSpec::Expr(parse_expr(s, line)?))
    }
}

pub(crate) fn parse_rv_ctrl_csr(
    mnem: &str,
    ops: &[&str],
    ctx: &EvalCtx<'_>,
) -> Result<RvInst, AsmError> {
    let line = ctx.line;
    match mnem {
        "lui" | "auipc" => {
            req_count(ops, 2, mnem, line)?;
            Ok(RvInst::UImm20 {
                is_auipc: mnem == "auipc",
                rd: reg(ops[0], line)?,
                imm: parse_expr(ops[1], line)?,
            })
        }
        "jal" => match ops.len() {
            1 => Ok(RvInst::Jump {
                rd: 1,
                target: parse_expr(ops[0], line)?,
            }),
            2 => Ok(RvInst::Jump {
                rd: reg(ops[0], line)?,
                target: parse_expr(ops[1], line)?,
            }),
            _ => Err(AsmError::new(line, "wrong operand count for 'jal'")),
        },
        "j" => {
            req_count(ops, 1, mnem, line)?;
            Ok(RvInst::Jump {
                rd: 0,
                target: parse_expr(ops[0], line)?,
            })
        }
        "jalr" => match ops.len() {
            1 => Ok(RvInst::Imm12 {
                kind: Imm12Kind::Jalr,
                r1: 1,
                r2: reg(ops[0], line)?,
                imm: Expr::Num(0),
            }),
            2 => {
                let rd = reg(ops[0], line)?;
                if ops[1].trim().ends_with(')') {
                    let (imm, rs1) = parse_mem_operand(ops[1], line)?;
                    Ok(RvInst::Imm12 {
                        kind: Imm12Kind::Jalr,
                        r1: rd,
                        r2: rs1,
                        imm: imm.unwrap_or(Expr::Num(0)),
                    })
                } else {
                    Ok(RvInst::Imm12 {
                        kind: Imm12Kind::Jalr,
                        r1: rd,
                        r2: reg(ops[1], line)?,
                        imm: Expr::Num(0),
                    })
                }
            }
            3 => Ok(RvInst::Imm12 {
                kind: Imm12Kind::Jalr,
                r1: reg(ops[0], line)?,
                r2: reg(ops[1], line)?,
                imm: parse_expr(ops[2], line)?,
            }),
            _ => Err(AsmError::new(line, "wrong operand count for 'jalr'")),
        },
        "beq" | "bne" | "blt" | "bge" | "bltu" | "bgeu" => {
            req_count(ops, 3, mnem, line)?;
            let kind = match mnem {
                "beq" => BranchKind::Beq,
                "bne" => BranchKind::Bne,
                "blt" => BranchKind::Blt,
                "bge" => BranchKind::Bge,
                "bltu" => BranchKind::Bltu,
                _ => BranchKind::Bgeu,
            };
            Ok(RvInst::Branch {
                kind,
                rs1: reg(ops[0], line)?,
                rs2: reg(ops[1], line)?,
                target: parse_expr(ops[2], line)?,
            })
        }
        "bgt" | "ble" | "bgtu" | "bleu" => {
            req_count(ops, 3, mnem, line)?;
            let kind = match mnem {
                "bgt" => BranchKind::Blt,
                "ble" => BranchKind::Bge,
                "bgtu" => BranchKind::Bltu,
                _ => BranchKind::Bgeu,
            };
            Ok(RvInst::Branch {
                kind,
                rs1: reg(ops[1], line)?,
                rs2: reg(ops[0], line)?,
                target: parse_expr(ops[2], line)?,
            })
        }
        "beqz" | "bnez" | "blez" | "bgez" | "bltz" | "bgtz" => {
            req_count(ops, 2, mnem, line)?;
            let r = reg(ops[0], line)?;
            let (kind, rs1, rs2) = match mnem {
                "beqz" => (BranchKind::Beq, r, 0),
                "bnez" => (BranchKind::Bne, r, 0),
                "blez" => (BranchKind::Bge, 0, r),
                "bgez" => (BranchKind::Bge, r, 0),
                "bltz" => (BranchKind::Blt, r, 0),
                _ => (BranchKind::Blt, 0, r),
            };
            Ok(RvInst::Branch {
                kind,
                rs1,
                rs2,
                target: parse_expr(ops[1], line)?,
            })
        }
        "call" => {
            req_count(ops, 1, mnem, line)?;
            Ok(RvInst::PcrelPair {
                rd: 1,
                link_rd: Some(1),
                target: parse_expr(ops[0], line)?,
            })
        }
        "tail" => {
            req_count(ops, 1, mnem, line)?;
            Ok(RvInst::PcrelPair {
                rd: 6,
                link_rd: Some(0),
                target: parse_expr(ops[0], line)?,
            })
        }
        "lla" | "la" => {
            req_count(ops, 2, mnem, line)?;
            Ok(RvInst::PcrelPair {
                rd: reg(ops[0], line)?,
                link_rd: None,
                target: parse_expr(ops[1], line)?,
            })
        }
        "li" => {
            req_count(ops, 2, mnem, line)?;
            let rd = reg(ops[0], line)?;
            let expr = parse_expr(ops[1], line)?;
            let v = eval_expr(&expr, ctx)?;
            if v.has_label {
                return Err(AsmError::new(
                    line,
                    "li operand must be absolute (no labels)",
                ));
            }
            Ok(RvInst::Li {
                words: synth_li(rd, v.val),
            })
        }
        "csrrw" | "csrrs" | "csrrc" => {
            req_count(ops, 3, mnem, line)?;
            let op = match mnem {
                "csrrw" => CsrOp::Rw,
                "csrrs" => CsrOp::Rs,
                _ => CsrOp::Rc,
            };
            Ok(RvInst::CsrReg {
                op,
                rd: reg(ops[0], line)?,
                csr: csr_spec(ops[1], line)?,
                rs1: reg(ops[2], line)?,
            })
        }
        "csrrwi" | "csrrsi" | "csrrci" => {
            req_count(ops, 3, mnem, line)?;
            let op = match mnem {
                "csrrwi" => CsrOp::Rw,
                "csrrsi" => CsrOp::Rs,
                _ => CsrOp::Rc,
            };
            Ok(RvInst::CsrImm {
                op,
                rd: reg(ops[0], line)?,
                csr: csr_spec(ops[1], line)?,
                uimm: parse_expr(ops[2], line)?,
            })
        }
        "csrr" => {
            req_count(ops, 2, mnem, line)?;
            Ok(RvInst::CsrReg {
                op: CsrOp::Rs,
                rd: reg(ops[0], line)?,
                csr: csr_spec(ops[1], line)?,
                rs1: 0,
            })
        }
        "csrw" | "csrs" | "csrc" => {
            req_count(ops, 2, mnem, line)?;
            let op = match mnem {
                "csrw" => CsrOp::Rw,
                "csrs" => CsrOp::Rs,
                _ => CsrOp::Rc,
            };
            Ok(RvInst::CsrReg {
                op,
                rd: 0,
                csr: csr_spec(ops[0], line)?,
                rs1: reg(ops[1], line)?,
            })
        }
        "csrwi" | "csrsi" | "csrci" => {
            req_count(ops, 2, mnem, line)?;
            let op = match mnem {
                "csrwi" => CsrOp::Rw,
                "csrsi" => CsrOp::Rs,
                _ => CsrOp::Rc,
            };
            Ok(RvInst::CsrImm {
                op,
                rd: 0,
                csr: csr_spec(ops[0], line)?,
                uimm: parse_expr(ops[1], line)?,
            })
        }
        "rdcycle" | "rdtime" | "rdinstret" => {
            req_count(ops, 1, mnem, line)?;
            let csr = match mnem {
                "rdcycle" => 0xC00,
                "rdtime" => 0xC01,
                _ => 0xC02,
            };
            Ok(RvInst::CsrReg {
                op: CsrOp::Rs,
                rd: reg(ops[0], line)?,
                rs1: 0,
                csr: CsrSpec::Fixed(csr),
            })
        }
        _ => Err(AsmError::new(line, format!("unknown mnemonic '{mnem}'"))),
    }
}
