//! IR optimiser: constant folding, copy propagation and dead-code removal over Mica IR.
use alloc::vec::Vec;

use mica::ir::{Body, Func, Inst, Program, Ty};

mod copy;
mod dce;
mod fold;

/// Maximum optimisation rounds per function.
const MAX_ROUNDS: usize = 8;

/// Counts of what one run changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Operations replaced by constants or branches turned into jumps.
    pub folded: usize,
    /// Register reads rewritten to a copy source.
    pub copies: usize,
    /// Instructions removed.
    pub removed: usize,
}

/// Optimise every `Body::Code` function in place; returns what changed. Idempotent in effect.
pub fn optimize(p: &mut Program) -> Stats {
    let mut total = Stats::default();
    for f in p.funcs.iter_mut() {
        let s = optimize_func(f);
        total.folded += s.folded;
        total.copies += s.copies;
        total.removed += s.removed;
    }
    total
}

fn optimize_func(f: &mut Func) -> Stats {
    let regs: &[Ty] = &f.regs;
    let Body::Code { insts, spans } = &mut f.body else {
        return Stats::default();
    };
    // Spans must stay parallel to instructions; leave malformed bodies alone.
    if insts.len() != spans.len() {
        return Stats::default();
    }
    let mut total = Stats::default();
    for _ in 0..MAX_ROUNDS {
        let round = Stats {
            folded: fold::fold(insts),
            copies: copy::copy_prop(regs, insts),
            removed: dce::dce(insts, spans),
        };
        if round == Stats::default() {
            break;
        }
        total.folded += round.folded;
        total.copies += round.copies;
        total.removed += round.removed;
    }
    total
}

/// Keeps the instructions and spans whose flag is true, in order.
fn retain_flagged(insts: &mut Vec<Inst>, spans: &mut Vec<mica::Span>, keep: &[bool]) -> usize {
    let mut i = 0usize;
    insts.retain(|_| {
        let k = keep.get(i).copied().unwrap_or(true);
        i += 1;
        k
    });
    let mut j = 0usize;
    spans.retain(|_| {
        let k = keep.get(j).copied().unwrap_or(true);
        j += 1;
        k
    });
    keep.iter().filter(|k| !**k).count()
}

/// Destination register written by an instruction, if it writes a register.
fn dest_of(inst: &Inst) -> Option<mica::ir::Reg> {
    match inst {
        Inst::Lit(d, _)
        | Inst::Move(d, _)
        | Inst::Un(d, _, _)
        | Inst::Bin(d, _, _, _)
        | Inst::Call(d, _, _)
        | Inst::CallValue(d, _, _)
        | Inst::Builtin(d, _, _)
        | Inst::NewArray(d, _)
        | Inst::NewFill(d, _, _)
        | Inst::NewTuple(d, _)
        | Inst::NewRecord(d, _, _)
        | Inst::NewVariant(d, _, _, _)
        | Inst::NewSome(d, _)
        | Inst::NewNone(d)
        | Inst::GetField(d, _, _)
        | Inst::Tag(d, _)
        | Inst::GetIndex(d, _, _) => Some(*d),
        _ => None,
    }
}

/// Calls `f` on every register an instruction reads (not its destination).
fn for_reads_mut(inst: &mut Inst, mut f: impl FnMut(&mut mica::ir::Reg)) {
    match inst {
        Inst::Lit(_, _) | Inst::NewNone(_) | Inst::Label(_) | Inst::Jump(_) => {}
        Inst::Move(_, s)
        | Inst::Un(_, _, s)
        | Inst::NewSome(_, s)
        | Inst::GetField(_, s, _)
        | Inst::Tag(_, s)
        | Inst::Branch(s, _, _)
        | Inst::Ret(s)
        | Inst::Assert(s, _) => f(s),
        Inst::Bin(_, _, a, b)
        | Inst::NewFill(_, a, b)
        | Inst::GetIndex(_, a, b)
        | Inst::SetField(a, _, b) => {
            f(a);
            f(b);
        }
        Inst::SetIndex(a, b, c) => {
            f(a);
            f(b);
            f(c);
        }
        Inst::Call(_, _, args) | Inst::Builtin(_, _, args) => args.iter_mut().for_each(f),
        Inst::CallValue(_, callee, args) => {
            f(callee);
            args.iter_mut().for_each(f);
        }
        Inst::NewArray(_, args) | Inst::NewTuple(_, args) | Inst::NewRecord(_, _, args) => {
            args.iter_mut().for_each(f)
        }
        Inst::NewVariant(_, _, _, args) => args.iter_mut().for_each(f),
    }
}
