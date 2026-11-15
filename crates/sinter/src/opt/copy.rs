//! Copy propagation within basic blocks, restricted to scalar registers.
use alloc::collections::BTreeMap;

use mica::ir::{Inst, Reg, Ty};

use super::{dest_of, for_reads_mut};

/// Rewrites reads of a `Move` copy to its source until either register is written. Returns rewrites.
pub(super) fn copy_prop(regs: &[Ty], insts: &mut [Inst]) -> usize {
    let scalar = |r: Reg| matches!(regs.get(r as usize), Some(Ty::Int | Ty::Byte | Ty::Bool));
    let mut copy: BTreeMap<Reg, Reg> = BTreeMap::new();
    let mut n = 0usize;
    for inst in insts.iter_mut() {
        if matches!(inst, Inst::Label(_)) {
            copy.clear();
            continue;
        }
        for_reads_mut(inst, |r| {
            if let Some(&src) = copy.get(r) {
                *r = src;
                n += 1;
            }
        });
        if let Some(d) = dest_of(inst) {
            copy.remove(&d);
            copy.retain(|_, v| *v != d);
        }
        match *inst {
            Inst::Move(d, s) if d != s && scalar(d) && scalar(s) => {
                copy.insert(d, s);
            }
            Inst::Jump(_) | Inst::Branch(_, _, _) | Inst::Ret(_) => copy.clear(),
            _ => {}
        }
    }
    n
}
