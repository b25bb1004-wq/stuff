//! Parser for explicit RV64C (`c.*`) compressed instructions.

use crate::encode_c::CInst;
use crate::expr::parse_expr;
use crate::regs::{c_reg, parse_mem_operand, parse_reg};
use crate::AsmError;

fn req_count(ops: &[&str], n: usize, mnem: &str, line: usize) -> Result<(), AsmError> {
    if ops.len() != n {
        return Err(AsmError::new(
            line,
            format!("'{mnem}' expects {n} operand(s), got {}", ops.len()),
        ));
    }
    Ok(())
}

fn req_reg(s: &str, line: usize) -> Result<u8, AsmError> {
    parse_reg(s).ok_or_else(|| AsmError::new(line, format!("unknown register '{s}'")))
}

fn req_c_reg(s: &str, line: usize) -> Result<u8, AsmError> {
    let r = req_reg(s, line)?;
    c_reg(r).ok_or_else(|| {
        AsmError::new(
            line,
            format!("compressed instruction requires x8..x15 register, got '{s}'"),
        )
    })
}

fn req_nonzero_reg(s: &str, line: usize) -> Result<u8, AsmError> {
    let r = req_reg(s, line)?;
    if r == 0 {
        return Err(AsmError::new(line, "compressed register cannot be x0"));
    }
    Ok(r)
}

/// Parse a `c.*` mnemonic and operands into a [`CInst`].
pub(crate) fn parse_c(mnem: &str, ops: &[&str], line: usize) -> Result<Option<CInst>, AsmError> {
    if !mnem.starts_with("c.") {
        return Ok(None);
    }
    let inst = match mnem {
        "c.addi4spn" => {
            req_count(ops, 3, mnem, line)?;
            let rd_p = req_c_reg(ops[0], line)?;
            let sp = req_reg(ops[1], line)?;
            if sp != 2 {
                return Err(AsmError::new(line, "c.addi4spn second operand must be sp"));
            }
            CInst::Addi4spn {
                rd_p,
                imm: parse_expr(ops[2], line)?,
            }
        }
        "c.lw" | "c.ld" | "c.sw" | "c.sd" => {
            req_count(ops, 2, mnem, line)?;
            let r_p = req_c_reg(ops[0], line)?;
            let (imm, base) = parse_mem_operand(ops[1], line)?;
            let rs1_p = c_reg(base)
                .ok_or_else(|| AsmError::new(line, "compressed load/store base must be x8..x15"))?;
            match mnem {
                "c.lw" => CInst::Lw {
                    rd_p: r_p,
                    rs1_p,
                    imm,
                },
                "c.ld" => CInst::Ld {
                    rd_p: r_p,
                    rs1_p,
                    imm,
                },
                "c.sw" => CInst::Sw {
                    rs2_p: r_p,
                    rs1_p,
                    imm,
                },
                _ => CInst::Sd {
                    rs2_p: r_p,
                    rs1_p,
                    imm,
                },
            }
        }
        "c.nop" => {
            req_count(ops, 0, mnem, line)?;
            CInst::Nop
        }
        "c.addi" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Addi {
                rd: req_nonzero_reg(ops[0], line)?,
                imm: parse_expr(ops[1], line)?,
            }
        }
        "c.addiw" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Addiw {
                rd: req_nonzero_reg(ops[0], line)?,
                imm: parse_expr(ops[1], line)?,
            }
        }
        "c.li" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Li {
                rd: req_nonzero_reg(ops[0], line)?,
                imm: parse_expr(ops[1], line)?,
            }
        }
        "c.addi16sp" => {
            req_count(ops, 2, mnem, line)?;
            if req_reg(ops[0], line)? != 2 {
                return Err(AsmError::new(line, "c.addi16sp first operand must be sp"));
            }
            CInst::Addi16sp {
                imm: parse_expr(ops[1], line)?,
            }
        }
        "c.lui" => {
            req_count(ops, 2, mnem, line)?;
            let rd = req_nonzero_reg(ops[0], line)?;
            if rd == 2 {
                return Err(AsmError::new(line, "c.lui rd cannot be sp"));
            }
            CInst::Lui {
                rd,
                imm: parse_expr(ops[1], line)?,
            }
        }
        "c.srli" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Srli {
                rd_p: req_c_reg(ops[0], line)?,
                shamt: parse_expr(ops[1], line)?,
            }
        }
        "c.srai" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Srai {
                rd_p: req_c_reg(ops[0], line)?,
                shamt: parse_expr(ops[1], line)?,
            }
        }
        "c.andi" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Andi {
                rd_p: req_c_reg(ops[0], line)?,
                imm: parse_expr(ops[1], line)?,
            }
        }
        "c.sub" | "c.xor" | "c.or" | "c.and" | "c.subw" | "c.addw" => {
            req_count(ops, 2, mnem, line)?;
            let rd_p = req_c_reg(ops[0], line)?;
            let rs2_p = req_c_reg(ops[1], line)?;
            let (funct6, funct2) = match mnem {
                "c.sub" => (0b100_011, 0b00),
                "c.xor" => (0b100_011, 0b01),
                "c.or" => (0b100_011, 0b10),
                "c.and" => (0b100_011, 0b11),
                "c.subw" => (0b100_111, 0b00),
                _ => (0b100_111, 0b01),
            };
            CInst::Arith {
                funct6,
                funct2,
                rd_p,
                rs2_p,
            }
        }
        "c.j" => {
            req_count(ops, 1, mnem, line)?;
            CInst::J {
                target: parse_expr(ops[0], line)?,
            }
        }
        "c.beqz" | "c.bnez" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Branch {
                funct3: if mnem == "c.beqz" { 0b110 } else { 0b111 },
                rs1_p: req_c_reg(ops[0], line)?,
                target: parse_expr(ops[1], line)?,
            }
        }
        "c.slli" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Slli {
                rd: req_nonzero_reg(ops[0], line)?,
                shamt: parse_expr(ops[1], line)?,
            }
        }
        "c.lwsp" | "c.ldsp" => {
            req_count(ops, 2, mnem, line)?;
            let rd = req_nonzero_reg(ops[0], line)?;
            let (imm, base) = parse_mem_operand(ops[1], line)?;
            if base != 2 {
                return Err(AsmError::new(
                    line,
                    format!("{mnem} base register must be sp"),
                ));
            }
            if mnem == "c.lwsp" {
                CInst::Lwsp { rd, imm }
            } else {
                CInst::Ldsp { rd, imm }
            }
        }
        "c.jr" => {
            req_count(ops, 1, mnem, line)?;
            CInst::Jr {
                rs1: req_nonzero_reg(ops[0], line)?,
            }
        }
        "c.mv" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Mv {
                rd: req_nonzero_reg(ops[0], line)?,
                rs2: req_nonzero_reg(ops[1], line)?,
            }
        }
        "c.ebreak" => {
            req_count(ops, 0, mnem, line)?;
            CInst::Ebreak
        }
        "c.jalr" => {
            req_count(ops, 1, mnem, line)?;
            CInst::Jalr {
                rs1: req_nonzero_reg(ops[0], line)?,
            }
        }
        "c.add" => {
            req_count(ops, 2, mnem, line)?;
            CInst::Add {
                rd: req_nonzero_reg(ops[0], line)?,
                rs2: req_nonzero_reg(ops[1], line)?,
            }
        }
        "c.swsp" | "c.sdsp" => {
            req_count(ops, 2, mnem, line)?;
            let rs2 = req_reg(ops[0], line)?;
            let (imm, base) = parse_mem_operand(ops[1], line)?;
            if base != 2 {
                return Err(AsmError::new(
                    line,
                    format!("{mnem} base register must be sp"),
                ));
            }
            if mnem == "c.swsp" {
                CInst::Swsp { rs2, imm }
            } else {
                CInst::Sdsp { rs2, imm }
            }
        }
        _ => return Err(AsmError::new(line, format!("unknown mnemonic '{mnem}'"))),
    };
    Ok(Some(inst))
}
