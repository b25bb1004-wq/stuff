//! Property and contract tests for the safe `Sim`, checked against a shadow model of live blocks.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use midas::{Block, BlockState, Handle, Sim, SimError, PAGE};

/// Usable arena per Sim in the property runs.
const ARENA: usize = 1 << 20;
/// Operations per generated case.
const OPS: usize = 300;

/// Request sizes: mostly slab sizes, some page-spanning, a few large buddy blocks.
fn pick_size(rng: &mut Rng) -> usize {
    match rng.below(10) {
        0..=5 => 1 + rng.below(2048),
        6..=8 => 2049 + rng.below(3 * PAGE),
        _ => 1 + rng.below(64 * PAGE),
    }
}

/// Live `(offset, size)` pairs sorted, the form the shadow model and `blocks()` are compared in.
fn live_pairs(blocks: &[Block]) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = blocks
        .iter()
        .filter(|b| b.state == BlockState::Live)
        .map(|b| (b.offset, b.size))
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn sim_matches_shadow_model() {
    forall(48, 0x94_5EED, |rng| {
        let mut sim = Sim::new(ARENA).unwrap();
        let mut other = Sim::new(ARENA).unwrap();
        let foreign: Handle = other.alloc(100).unwrap();
        // Shadow model: (handle, offset, reserved size) for every live block, and every freed handle.
        let mut live: Vec<(Handle, usize, usize)> = Vec::new();
        let mut dead: Vec<Handle> = Vec::new();

        for _ in 0..OPS {
            match rng.below(10) {
                0..=5 => match sim.alloc(pick_size(rng)) {
                    Ok(h) => {
                        let b = sim.block(h).unwrap();
                        assert_eq!(b.state, BlockState::Live);
                        assert!(b.offset + b.size <= ARENA + PAGE, "block past the arena");
                        for &(_, o, s) in &live {
                            assert!(b.offset + b.size <= o || o + s <= b.offset, "overlap");
                        }
                        live.push((h, b.offset, b.size));
                    }
                    Err(e) => assert_eq!(e, SimError::OutOfMemory),
                },
                6..=7 if !live.is_empty() => {
                    let i = rng.below(live.len());
                    let (h, _, _) = live.swap_remove(i);
                    assert_eq!(sim.free(h), Ok(()));
                    dead.push(h);
                }
                8 if !dead.is_empty() => {
                    let h = *rng.pick(&dead);
                    assert_eq!(sim.free(h), Err(SimError::DoubleFree));
                    assert_eq!(sim.block(h), Err(SimError::DoubleFree));
                }
                _ => assert_eq!(sim.free(foreign), Err(SimError::Foreign)),
            }

            assert!(sim.check(), "heap invariant broken");
            let mut model: Vec<(usize, usize)> = live.iter().map(|&(_, o, s)| (o, s)).collect();
            model.sort_unstable();
            assert_eq!(
                live_pairs(&sim.blocks()),
                model,
                "blocks() disagrees with model"
            );
        }

        for (h, _, _) in live.drain(..) {
            assert_eq!(sim.free(h), Ok(()));
        }
        assert_eq!(sim.used_bytes(), 0);
        assert!(sim.check());
        assert!(sim.blocks().iter().all(|b| b.state == BlockState::Freed));
    });
}

#[test]
fn refusals_never_panic_or_touch_the_heap() {
    let mut sim = Sim::new(ARENA).unwrap();
    assert_eq!(sim.alloc(0), Err(SimError::ZeroSize));
    assert_eq!(sim.alloc(usize::MAX), Err(SimError::OutOfMemory));
    assert_eq!(sim.used_bytes(), 0);
    assert!(sim.blocks().is_empty());
    assert!(Sim::new(usize::MAX).is_err());
}

#[test]
fn handles_do_not_cross_between_sims() {
    let mut a = Sim::new(ARENA).unwrap();
    let mut b = Sim::new(ARENA).unwrap();
    let ha = a.alloc(32).unwrap();
    let hb = b.alloc(32).unwrap();
    assert_eq!(b.free(ha), Err(SimError::Foreign));
    assert_eq!(a.free(hb), Err(SimError::Foreign));
    assert_eq!(a.block(hb), Err(SimError::Foreign));
    assert_eq!(a.free(ha), Ok(()));
    assert_eq!(b.free(hb), Ok(()));
}
