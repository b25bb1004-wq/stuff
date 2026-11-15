//! Expression evaluator with relocations and label balance tracking.

use std::collections::HashMap;

use crate::expr::{BinOp, Expr, RelocKind, UnaryOp};
use crate::AsmError;

/// Result of evaluating an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvalVal {
    pub val: i64,
    pub label_balance: i32,
    pub has_label: bool,
}

/// Evaluation environment for expressions.
pub(crate) struct EvalCtx<'a> {
    pub line: usize,
    pub stmt_idx: usize,
    pub pc: u64,
    pub equs: &'a HashMap<String, EvalVal>,
    pub labels: &'a HashMap<String, u64>,
    /// Numeric label definitions: `num -> sorted list of (stmt_idx, addr)`.
    pub num_labels: &'a HashMap<u64, Vec<(usize, u64)>>,
    /// Recorded `%pcrel_hi` auipc PCs -> `target - auipc_pc`.
    pub pcrel_auipcs: Option<&'a HashMap<u64, i64>>,
    pub allow_pcrel_hi: bool,
}

/// Evaluate a parsed expression under `ctx`.
pub(crate) fn eval_expr(expr: &Expr, ctx: &EvalCtx<'_>) -> Result<EvalVal, AsmError> {
    match expr {
        Expr::Num(v) => Ok(EvalVal {
            val: *v,
            label_balance: 0,
            has_label: false,
        }),
        Expr::Ident(name) => {
            if let Some(&v) = ctx.equs.get(name) {
                return Ok(v);
            }
            if let Some(&addr) = ctx.labels.get(name) {
                return Ok(EvalVal {
                    val: addr as i64,
                    label_balance: 1,
                    has_label: true,
                });
            }
            Err(AsmError::new(
                ctx.line,
                format!("undefined symbol '{name}'"),
            ))
        }
        Expr::NumLabel { num, forward } => {
            let defs = ctx.num_labels.get(num).ok_or_else(|| {
                let dir = if *forward { 'f' } else { 'b' };
                AsmError::new(ctx.line, format!("undefined numeric label '{num}{dir}'"))
            })?;
            let found = if *forward {
                defs.iter().find(|&&(idx, _)| idx > ctx.stmt_idx)
            } else {
                defs.iter().rfind(|&&(idx, _)| idx <= ctx.stmt_idx)
            };
            let &(_, addr) = found.ok_or_else(|| {
                let dir = if *forward { 'f' } else { 'b' };
                AsmError::new(ctx.line, format!("no matching numeric label '{num}{dir}'"))
            })?;
            Ok(EvalVal {
                val: addr as i64,
                label_balance: 1,
                has_label: true,
            })
        }
        Expr::Unary { op, inner } => {
            let v = eval_expr(inner, ctx)?;
            match op {
                UnaryOp::Plus => Ok(v),
                UnaryOp::Neg => Ok(EvalVal {
                    val: v.val.wrapping_neg(),
                    label_balance: -v.label_balance,
                    has_label: v.has_label,
                }),
                UnaryOp::BitNot => {
                    if v.label_balance != 0 {
                        return Err(AsmError::new(ctx.line, "cannot apply '~' to a label"));
                    }
                    Ok(EvalVal {
                        val: !v.val,
                        label_balance: 0,
                        has_label: v.has_label,
                    })
                }
            }
        }
        Expr::Binary { op, lhs, rhs } => eval_binary(*op, lhs, rhs, ctx),
        Expr::Reloc { kind, inner } => eval_reloc(*kind, inner, ctx),
    }
}

fn eval_binary(op: BinOp, lhs: &Expr, rhs: &Expr, ctx: &EvalCtx<'_>) -> Result<EvalVal, AsmError> {
    let a = eval_expr(lhs, ctx)?;
    let b = eval_expr(rhs, ctx)?;
    match op {
        BinOp::Add => Ok(EvalVal {
            val: a.val.wrapping_add(b.val),
            label_balance: a.label_balance + b.label_balance,
            has_label: a.has_label || b.has_label,
        }),
        BinOp::Sub => Ok(EvalVal {
            val: a.val.wrapping_sub(b.val),
            label_balance: a.label_balance - b.label_balance,
            has_label: a.has_label || b.has_label,
        }),
        _ => {
            if a.label_balance != 0 || b.label_balance != 0 {
                return Err(AsmError::new(
                    ctx.line,
                    "invalid arithmetic on relocatable label",
                ));
            }
            let val = match op {
                BinOp::Mul => a.val.wrapping_mul(b.val),
                BinOp::Div => {
                    if b.val == 0 {
                        return Err(AsmError::new(ctx.line, "division by zero"));
                    }
                    a.val.wrapping_div(b.val)
                }
                BinOp::Rem => {
                    if b.val == 0 {
                        return Err(AsmError::new(ctx.line, "division by zero"));
                    }
                    a.val.wrapping_rem(b.val)
                }
                BinOp::Shl => {
                    if (b.val as u64) >= 64 {
                        0
                    } else {
                        ((a.val as u64) << (b.val as u32)) as i64
                    }
                }
                BinOp::Shr => {
                    if (b.val as u64) >= 64 {
                        0
                    } else {
                        ((a.val as u64) >> (b.val as u32)) as i64
                    }
                }
                BinOp::BitOr => a.val | b.val,
                BinOp::BitAnd => a.val & b.val,
                BinOp::BitXor => a.val ^ b.val,
                BinOp::Add | BinOp::Sub => unreachable!(),
            };
            Ok(EvalVal {
                val,
                label_balance: 0,
                has_label: a.has_label || b.has_label,
            })
        }
    }
}

fn eval_reloc(kind: RelocKind, inner: &Expr, ctx: &EvalCtx<'_>) -> Result<EvalVal, AsmError> {
    let v = eval_expr(inner, ctx)?;
    match kind {
        RelocKind::Hi => Ok(EvalVal {
            val: (v.val.wrapping_add(0x800) >> 12) & 0xfffff,
            label_balance: 0,
            has_label: v.has_label,
        }),
        RelocKind::Lo => Ok(EvalVal {
            val: (v.val << 52) >> 52,
            label_balance: 0,
            has_label: v.has_label,
        }),
        RelocKind::PcrelHi => {
            if !ctx.allow_pcrel_hi {
                return Err(AsmError::new(ctx.line, "%pcrel_hi is only valid in auipc"));
            }
            let diff = v.val.wrapping_sub(ctx.pc as i64);
            Ok(EvalVal {
                val: (diff.wrapping_add(0x800) >> 12) & 0xfffff,
                label_balance: 0,
                has_label: v.has_label,
            })
        }
        RelocKind::PcrelLo => {
            if v.label_balance != 1 {
                return Err(AsmError::new(ctx.line, "bad %pcrel_lo target"));
            }
            let map = ctx
                .pcrel_auipcs
                .ok_or_else(|| AsmError::new(ctx.line, "bad %pcrel_lo target"))?;
            let diff = map
                .get(&(v.val as u64))
                .ok_or_else(|| AsmError::new(ctx.line, "bad %pcrel_lo target"))?;
            Ok(EvalVal {
                val: (*diff << 52) >> 52,
                label_balance: 0,
                has_label: true,
            })
        }
    }
}
