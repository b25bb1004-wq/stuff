//! Dead-code removal: unreachable tails, self-moves and pure instructions with unread results.
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use logos::ast::{BinOp, UnOp};
use logos::ir::{Inst, Reg};

use super::{dest_of, for_reads_mut, retain_flagged};

/// Removes dead instructions and their spans. Returns how many were removed.
pub(super) fn dce(insts: &mut Vec<Inst>, spans: &mut Vec<logos::Span>) -> usize {
    // Instructions after Jump/Ret are unreachable up to the next Label.
    let mut keep = Vec::with_capacity(insts.len());
    let mut dead = false;
    for inst in insts.iter() {
        match inst {
            Inst::Label(_) => {
                dead = false;
                keep.push(true);
            }
            _ if dead => keep.push(false),
            Inst::Jump(_) | Inst::Ret(_) => {
                dead = true;
                keep.push(true);
            }
            _ => keep.push(true),
        }
    }
    let mut removed = retain_flagged(insts, spans, &keep);

    // Registers read anywhere in the function.
    let mut reads: BTreeSet<Reg> = BTreeSet::new();
    for inst in insts.iter_mut() {
        for_reads_mut(inst, |r| {
            reads.insert(*r);
        });
    }

    let keep: Vec<bool> = insts
        .iter()
        .map(|inst| {
            if let Inst::Move(d, s) = inst {
                if d == s {
                    return false;
                }
            }
            match dest_of(inst) {
                Some(d) => !(is_pure(inst) && !reads.contains(&d)),
                None => true,
            }
        })
        .collect();
    removed += retain_flagged(insts, spans, &keep);
    removed
}

/// Instructions with no effect besides writing their destination (cannot trap).
fn is_pure(inst: &Inst) -> bool {
    match inst {
        Inst::Lit(_, _)
        | Inst::Move(_, _)
        | Inst::NewArray(_, _)
        | Inst::NewTuple(_, _)
        | Inst::NewRecord(_, _, _)
        | Inst::NewVariant(_, _, _, _)
        | Inst::NewSome(_, _)
        | Inst::NewNone(_)
        | Inst::Tag(_, _) => true,
        Inst::Un(_, op, _) => *op == UnOp::Not,
        Inst::Bin(_, op, _, _) => matches!(
            op,
            BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::BitAnd
                | BinOp::AddWrap
                | BinOp::SubWrap
                | BinOp::MulWrap
        ),
        _ => false,
    }
}
