//! Encoder for RV64IMA, Zicsr, Zifencei, privileged, and pseudo instructions.

use quartz::inst::{encode, Inst};

use crate::encode_c::eval_rel_target;
use crate::expr::{Expr, RelocKind};
use crate::expr_eval::{eval_expr, EvalCtx};
use crate::AsmError;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Imm12Kind {
    Addi,
    Slti,
    Sltiu,
    Xori,
    Ori,
    Andi,
    Addiw,
    Lb,
    Lh,
    Lw,
    Ld,
    Lbu,
    Lhu,
    Lwu,
    Sb,
    Sh,
    Sw,
    Sd,
    Jalr,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ShiftKind {
    Slli,
    Srli,
    Srai,
    Slliw,
    Srliw,
    Sraiw,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BranchKind {
    Beq,
    Bne,
    Blt,
    Bge,
    Bltu,
    Bgeu,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CsrOp {
    Rw,
    Rs,
    Rc,
}

#[derive(Debug, Clone)]
pub(crate) enum CsrSpec {
    Fixed(u16),
    Expr(Expr),
}

/// Parsed 32-bit or multi-word pseudo instruction.
#[derive(Debug, Clone)]
pub(crate) enum RvInst {
    Fixed(u32),
    Amo {
        word: u32,
        imm: Option<Expr>,
    },
    Imm12 {
        kind: Imm12Kind,
        r1: u8,
        r2: u8,
        imm: Expr,
    },
    Shift {
        kind: ShiftKind,
        rd: u8,
        rs1: u8,
        shamt: Expr,
        max: i64,
    },
    UImm20 {
        is_auipc: bool,
        rd: u8,
        imm: Expr,
    },
    Jump {
        rd: u8,
        target: Expr,
    },
    Branch {
        kind: BranchKind,
        rs1: u8,
        rs2: u8,
        target: Expr,
    },
    CsrReg {
        op: CsrOp,
        rd: u8,
        rs1: u8,
        csr: CsrSpec,
    },
    CsrImm {
        op: CsrOp,
        rd: u8,
        uimm: Expr,
        csr: CsrSpec,
    },
    PcrelPair {
        rd: u8,
        link_rd: Option<u8>,
        target: Expr,
    },
    Li {
        words: Vec<u32>,
    },
}

impl RvInst {
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            RvInst::PcrelPair { .. } => 8,
            RvInst::Li { words } => words.len() * 4,
            _ => 4,
        }
    }

    /// If this is `auipc rd, %pcrel_hi(inner)`, return `Some(&inner)`.
    pub(crate) fn pcrel_hi_inner(&self) -> Option<&Expr> {
        if let RvInst::UImm20 {
            is_auipc: true,
            imm:
                Expr::Reloc {
                    kind: RelocKind::PcrelHi,
                    inner,
                },
            ..
        } = self
        {
            Some(inner)
        } else {
            None
        }
    }
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

fn resolve_csr(spec: &CsrSpec, ctx: &EvalCtx<'_>) -> Result<u16, AsmError> {
    match spec {
        CsrSpec::Fixed(c) => Ok(*c),
        CsrSpec::Expr(e) => {
            let v = eval_abs(e, ctx)?;
            if !(0..=4095).contains(&v) {
                return Err(AsmError::new(ctx.line, "CSR address out of range 0..4095"));
            }
            Ok(v as u16)
        }
    }
}

/// Encode a parsed [`RvInst`] into one or more 32-bit instruction words.
pub(crate) fn encode_rv(inst: &RvInst, ctx: &EvalCtx<'_>) -> Result<Vec<u32>, AsmError> {
    let line = ctx.line;
    match inst {
        RvInst::Fixed(w) => Ok(vec![*w]),
        RvInst::Amo { word, imm } => {
            if let Some(e) = imm {
                if eval_abs(e, ctx)? != 0 {
                    return Err(AsmError::new(line, "atomic memory offset must be 0"));
                }
            }
            Ok(vec![*word])
        }
        RvInst::Li { words } => Ok(words.clone()),
        RvInst::Imm12 { kind, r1, r2, imm } => {
            let v = eval_abs(imm, ctx)?;
            if !(-2048..=2047).contains(&v) {
                return Err(AsmError::new(line, "12-bit immediate out of range"));
            }
            let (rd, rs1, rs2) = (*r1, *r2, *r1);
            let encoded = match kind {
                Imm12Kind::Addi => Inst::Addi { rd, rs1, imm: v },
                Imm12Kind::Slti => Inst::Slti { rd, rs1, imm: v },
                Imm12Kind::Sltiu => Inst::Sltiu { rd, rs1, imm: v },
                Imm12Kind::Xori => Inst::Xori { rd, rs1, imm: v },
                Imm12Kind::Ori => Inst::Ori { rd, rs1, imm: v },
                Imm12Kind::Andi => Inst::Andi { rd, rs1, imm: v },
                Imm12Kind::Addiw => Inst::Addiw { rd, rs1, imm: v },
                Imm12Kind::Lb => Inst::Lb { rd, rs1, imm: v },
                Imm12Kind::Lh => Inst::Lh { rd, rs1, imm: v },
                Imm12Kind::Lw => Inst::Lw { rd, rs1, imm: v },
                Imm12Kind::Ld => Inst::Ld { rd, rs1, imm: v },
                Imm12Kind::Lbu => Inst::Lbu { rd, rs1, imm: v },
                Imm12Kind::Lhu => Inst::Lhu { rd, rs1, imm: v },
                Imm12Kind::Lwu => Inst::Lwu { rd, rs1, imm: v },
                Imm12Kind::Sb => Inst::Sb { rs1, rs2, imm: v },
                Imm12Kind::Sh => Inst::Sh { rs1, rs2, imm: v },
                Imm12Kind::Sw => Inst::Sw { rs1, rs2, imm: v },
                Imm12Kind::Sd => Inst::Sd { rs1, rs2, imm: v },
                Imm12Kind::Jalr => Inst::Jalr { rd, rs1, imm: v },
            };
            Ok(vec![encode(encoded)])
        }
        RvInst::Shift {
            kind,
            rd,
            rs1,
            shamt,
            max,
        } => {
            let v = eval_abs(shamt, ctx)?;
            if v < 0 || v > *max {
                return Err(AsmError::new(line, "shift amount out of range"));
            }
            let (rd, rs1) = (*rd, *rs1);
            let encoded = match kind {
                ShiftKind::Slli => Inst::Slli { rd, rs1, imm: v },
                ShiftKind::Srli => Inst::Srli { rd, rs1, imm: v },
                ShiftKind::Srai => Inst::Srai { rd, rs1, imm: v },
                ShiftKind::Slliw => Inst::Slliw { rd, rs1, imm: v },
                ShiftKind::Srliw => Inst::Srliw { rd, rs1, imm: v },
                ShiftKind::Sraiw => Inst::Sraiw { rd, rs1, imm: v },
            };
            Ok(vec![encode(encoded)])
        }
        RvInst::UImm20 { is_auipc, rd, imm } => {
            let v = eval_abs(imm, ctx)?;
            if !(0..=0xfffff).contains(&v) {
                return Err(AsmError::new(line, "20-bit upper immediate out of range"));
            }
            let shifted = v << 12;
            let encoded = if *is_auipc {
                Inst::Auipc {
                    rd: *rd,
                    imm: shifted,
                }
            } else {
                Inst::Lui {
                    rd: *rd,
                    imm: shifted,
                }
            };
            Ok(vec![encode(encoded)])
        }
        RvInst::Jump { rd, target } => {
            let off = eval_rel_target(target, ctx)?;
            if !(-(1 << 20)..=(1 << 20) - 2).contains(&off) || (off & 1) != 0 {
                return Err(AsmError::new(line, "jump target out of range or unaligned"));
            }
            Ok(vec![encode(Inst::Jal { rd: *rd, imm: off })])
        }
        RvInst::Branch {
            kind,
            rs1,
            rs2,
            target,
        } => {
            let off = eval_rel_target(target, ctx)?;
            if !(-4096..=4094).contains(&off) || (off & 1) != 0 {
                return Err(AsmError::new(
                    line,
                    "branch target out of range or unaligned",
                ));
            }
            let (rs1, rs2) = (*rs1, *rs2);
            let encoded = match kind {
                BranchKind::Beq => Inst::Beq { rs1, rs2, imm: off },
                BranchKind::Bne => Inst::Bne { rs1, rs2, imm: off },
                BranchKind::Blt => Inst::Blt { rs1, rs2, imm: off },
                BranchKind::Bge => Inst::Bge { rs1, rs2, imm: off },
                BranchKind::Bltu => Inst::Bltu { rs1, rs2, imm: off },
                BranchKind::Bgeu => Inst::Bgeu { rs1, rs2, imm: off },
            };
            Ok(vec![encode(encoded)])
        }
        RvInst::CsrReg { op, rd, rs1, csr } => {
            let csr = resolve_csr(csr, ctx)?;
            let (rd, rs1) = (*rd, *rs1);
            let encoded = match op {
                CsrOp::Rw => Inst::Csrrw { rd, rs1, csr },
                CsrOp::Rs => Inst::Csrrs { rd, rs1, csr },
                CsrOp::Rc => Inst::Csrrc { rd, rs1, csr },
            };
            Ok(vec![encode(encoded)])
        }
        RvInst::CsrImm { op, rd, uimm, csr } => {
            let csr = resolve_csr(csr, ctx)?;
            let v = eval_abs(uimm, ctx)?;
            if !(0..=31).contains(&v) {
                return Err(AsmError::new(line, "CSR immediate out of range 0..31"));
            }
            let (rd, uimm) = (*rd, v as u8);
            let encoded = match op {
                CsrOp::Rw => Inst::Csrrwi { rd, uimm, csr },
                CsrOp::Rs => Inst::Csrrsi { rd, uimm, csr },
                CsrOp::Rc => Inst::Csrrci { rd, uimm, csr },
            };
            Ok(vec![encode(encoded)])
        }
        RvInst::PcrelPair {
            rd,
            link_rd,
            target,
        } => {
            let diff = eval_rel_target(target, ctx)?;
            let hi = ((diff.wrapping_add(0x800) >> 12) & 0xfffff) << 12;
            let lo = (diff << 52) >> 52;
            let w1 = encode(Inst::Auipc { rd: *rd, imm: hi });
            let w2 = match link_rd {
                Some(lrd) => encode(Inst::Jalr {
                    rd: *lrd,
                    rs1: *rd,
                    imm: lo,
                }),
                None => encode(Inst::Addi {
                    rd: *rd,
                    rs1: *rd,
                    imm: lo,
                }),
            };
            Ok(vec![w1, w2])
        }
    }
}
