//! Constant folding within basic blocks.
use alloc::collections::BTreeMap;

use logos::ast::{BinOp, UnOp};
use logos::ir::{Inst, Lit, Reg};

use super::dest_of;

/// Folds known-constant `Un`/`Bin` ops and `Branch` on known bools. Returns the number of folds.
pub(super) fn fold(insts: &mut [Inst]) -> usize {
    let mut known: BTreeMap<Reg, Lit> = BTreeMap::new();
    let mut n = 0usize;
    for inst in insts.iter_mut() {
        if let Inst::Branch(c, t, f) = *inst {
            if let Some(Lit::Bool(b)) = known.get(&c) {
                *inst = Inst::Jump(if *b { t } else { f });
                n += 1;
            }
            known.clear();
            continue;
        }
        if matches!(inst, Inst::Label(_) | Inst::Jump(_) | Inst::Ret(_)) {
            known.clear();
            continue;
        }
        if let Some(d) = dest_of(inst) {
            if let Some(value) = fold_value(inst, &known) {
                *inst = Inst::Lit(d, value);
                n += 1;
            }
        }
        match inst {
            Inst::Lit(d, l) => {
                if matches!(l, Lit::Int(_) | Lit::Byte(_) | Lit::Bool(_)) {
                    known.insert(*d, l.clone());
                } else {
                    known.remove(d);
                }
            }
            Inst::Move(d, s) => match known.get(s).cloned() {
                Some(v) => {
                    known.insert(*d, v);
                }
                None => {
                    known.remove(d);
                }
            },
            other => {
                if let Some(d) = dest_of(other) {
                    known.remove(&d);
                }
            }
        }
    }
    n
}

/// Value of a `Un`/`Bin` whose operands are known; `None` when it would trap or is not foldable.
fn fold_value(inst: &Inst, known: &BTreeMap<Reg, Lit>) -> Option<Lit> {
    match inst {
        Inst::Un(_, op, s) => match (*op, known.get(s)?) {
            (UnOp::Not, Lit::Bool(b)) => Some(Lit::Bool(!b)),
            (UnOp::Neg, Lit::Int(i)) => i.checked_neg().map(Lit::Int),
            _ => None,
        },
        Inst::Bin(_, op, a, b) => bin(*op, known.get(a)?, known.get(b)?),
        _ => None,
    }
}

fn bin(op: BinOp, x: &Lit, y: &Lit) -> Option<Lit> {
    match (x, y) {
        (Lit::Int(a), Lit::Int(b)) => int_bin(op, *a, *b),
        (Lit::Byte(a), Lit::Byte(b)) => byte_bin(op, *a, *b),
        (Lit::Bool(a), Lit::Bool(b)) => match op {
            BinOp::Eq => Some(Lit::Bool(a == b)),
            BinOp::Ne => Some(Lit::Bool(a != b)),
            _ => None,
        },
        _ => None,
    }
}

/// Integer ops; overflow, division by zero and `MIN / -1` return `None` so the trap survives.
fn int_bin(op: BinOp, a: i64, b: i64) -> Option<Lit> {
    Some(match op {
        BinOp::Add => Lit::Int(a.checked_add(b)?),
        BinOp::Sub => Lit::Int(a.checked_sub(b)?),
        BinOp::Mul => Lit::Int(a.checked_mul(b)?),
        BinOp::Div => Lit::Int(a.checked_div(b)?),
        BinOp::Rem => Lit::Int(a.checked_rem(b)?),
        BinOp::AddWrap => Lit::Int(a.wrapping_add(b)),
        BinOp::SubWrap => Lit::Int(a.wrapping_sub(b)),
        BinOp::MulWrap => Lit::Int(a.wrapping_mul(b)),
        BinOp::BitAnd => Lit::Int(a & b),
        BinOp::BitOr => Lit::Int(a | b),
        BinOp::BitXor => Lit::Int(a ^ b),
        BinOp::Eq => Lit::Bool(a == b),
        BinOp::Ne => Lit::Bool(a != b),
        BinOp::Lt => Lit::Bool(a < b),
        BinOp::Le => Lit::Bool(a <= b),
        BinOp::Gt => Lit::Bool(a > b),
        BinOp::Ge => Lit::Bool(a >= b),
        _ => return None,
    })
}

/// Byte ops: checked arithmetic (overflow traps), no folding of shifts or wrapping forms.
fn byte_bin(op: BinOp, a: u8, b: u8) -> Option<Lit> {
    Some(match op {
        BinOp::Add => Lit::Byte(a.checked_add(b)?),
        BinOp::Sub => Lit::Byte(a.checked_sub(b)?),
        BinOp::Mul => Lit::Byte(a.checked_mul(b)?),
        BinOp::Div => Lit::Byte(a.checked_div(b)?),
        BinOp::Rem => Lit::Byte(a.checked_rem(b)?),
        BinOp::BitAnd => Lit::Byte(a & b),
        BinOp::BitOr => Lit::Byte(a | b),
        BinOp::BitXor => Lit::Byte(a ^ b),
        BinOp::Eq => Lit::Bool(a == b),
        BinOp::Ne => Lit::Bool(a != b),
        BinOp::Lt => Lit::Bool(a < b),
        BinOp::Le => Lit::Bool(a <= b),
        BinOp::Gt => Lit::Bool(a > b),
        BinOp::Ge => Lit::Bool(a >= b),
        _ => return None,
    })
}
