#![allow(clippy::unwrap_used)]
use super::*;
use crate::fx::{Fx, Vec2};
use alloc::vec;
use alloc::vec::Vec;
use assay::Rng;

#[derive(Clone, Hash, Debug, PartialEq)]
struct Pos(Vec2);

#[derive(Clone, Hash, Debug, PartialEq)]
struct Vel(Vec2);

#[derive(Clone, Hash, Debug, PartialEq)]
struct Tag(u32);

fn v(x: i64, y: i64) -> Vec2 {
    Vec2::new(Fx(x), Fx(y))
}

#[test]
fn spawn_reuses_lowest_index_with_next_generation() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn();
    let c = w.spawn();
    assert_eq!((a.index, a.generation), (0, 0));
    assert_eq!((b.index, b.generation), (1, 0));
    assert_eq!((c.index, c.generation), (2, 0));
    assert!(w.despawn(b));
    assert!(!w.despawn(b));
    assert!(!w.alive(b));
    let d = w.spawn();
    assert_eq!(
        d,
        Entity {
            index: 1,
            generation: 1
        }
    );
    let e = w.spawn();
    assert_eq!(
        e,
        Entity {
            index: 3,
            generation: 0
        }
    );
    assert_eq!(w.entities(), vec![a, d, c, e]);
}

#[test]
fn lowest_free_index_is_chosen_first() {
    let mut w = World::new();
    let es: Vec<Entity> = (0..4).map(|_| w.spawn()).collect();
    assert!(w.despawn(es[3]));
    assert!(w.despawn(es[1]));
    assert_eq!(w.spawn().index, 1);
    assert_eq!(w.spawn().index, 3);
}

#[test]
fn stale_handles_see_nothing() {
    let mut w = World::new();
    let old = w.spawn();
    assert!(w.insert(old, Tag(7)));
    assert!(w.despawn(old));
    let new = w.spawn();
    assert_eq!(new.index, old.index);
    assert!(w.get::<Tag>(new).is_none());
    assert!(!w.insert(old, Tag(9)));
    assert!(w.get::<Tag>(old).is_none());
    assert!(w.get_mut::<Tag>(old).is_none());
    assert!(w.remove::<Tag>(old).is_none());
    assert!(!w.despawn(old));
    assert!(!w.alive(old));
    assert!(w.get::<Tag>(new).is_none());
    assert!(w.insert(new, Tag(1)));
    assert_eq!(w.get::<Tag>(new), Some(&Tag(1)));
    assert!(w.get::<Tag>(old).is_none());
}

#[test]
fn components_insert_get_replace_remove() {
    let mut w = World::new();
    let e = w.spawn();
    assert!(w.get::<Tag>(e).is_none());
    assert!(w.insert(e, Tag(1)));
    assert!(w.insert(e, Tag(2)));
    assert_eq!(w.get::<Tag>(e), Some(&Tag(2)));
    if let Some(t) = w.get_mut::<Tag>(e) {
        t.0 = 5;
    }
    assert_eq!(w.remove::<Tag>(e), Some(Tag(5)));
    assert_eq!(w.remove::<Tag>(e), None);
    assert!(w.get::<Tag>(e).is_none());
}

#[test]
fn despawn_clears_all_components() {
    let mut w = World::new();
    let e = w.spawn();
    assert!(w.insert(e, Tag(3)));
    assert!(w.insert(e, Pos(v(1, 2))));
    assert!(w.despawn(e));
    let f = w.spawn();
    assert_eq!(f.index, e.index);
    assert!(w.get::<Tag>(f).is_none());
    assert!(w.get::<Pos>(f).is_none());
}

#[test]
fn each_visits_ascending_and_each2_filters_to_both() {
    let mut w = World::new();
    let es: Vec<Entity> = (0..6).map(|_| w.spawn()).collect();
    for &e in es.iter().rev() {
        assert!(w.insert(e, Tag(e.index)));
    }
    for &e in es.iter() {
        if e.index.is_multiple_of(2) || e.index == 3 {
            assert!(w.insert(e, Vel(v(e.index as i64, 0))));
        }
    }
    let mut tags = Vec::new();
    w.each::<Tag>(|e, t| tags.push((e.index, t.0)));
    assert_eq!(tags, vec![(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)]);

    let mut both = Vec::new();
    w.each2::<Tag, Vel>(|e, t, vel| {
        both.push((e.index, t.0));
        vel.0 = vel.0 + v(1, 1);
    });
    assert_eq!(both, vec![(0, 0), (2, 2), (3, 3), (4, 4)]);
    assert_eq!(w.get::<Vel>(es[3]), Some(&Vel(v(4, 1))));
    assert_eq!(w.get::<Vel>(es[1]), None);
}

#[test]
fn each2_with_unknown_type_visits_nothing() {
    let mut w = World::new();
    let e = w.spawn();
    assert!(w.insert(e, Tag(1)));
    let mut n = 0;
    w.each2::<Tag, Vel>(|_, _, _| n += 1);
    assert_eq!(n, 0);
}

#[test]
fn replace_keeps_one_value_and_entities_list_is_live_only() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn();
    assert!(w.insert(a, Tag(1)));
    assert!(w.insert(a, Tag(2)));
    assert!(w.despawn(b));
    assert_eq!(w.entities(), vec![a]);
    let mut seen = Vec::new();
    w.each::<Tag>(|_, t| seen.push(t.0));
    assert_eq!(seen, vec![2]);
}

#[test]
fn hash_depends_on_contents_not_insertion_order_per_entity() {
    let build = |reverse: bool| {
        let mut w = World::new();
        let es: Vec<Entity> = (0..10).map(|_| w.spawn()).collect();
        for &e in &es {
            assert!(w.insert(e, Pos(v(e.index as i64, 0))));
        }
        let order: Vec<Entity> = if reverse {
            es.iter().rev().copied().collect()
        } else {
            es.clone()
        };
        for e in order {
            assert!(w.insert(e, Vel(v(0, e.index as i64))));
        }
        w
    };
    assert_eq!(build(false).hash(), build(true).hash());
    let mut changed = build(false);
    let e = changed.entities()[4];
    if let Some(p) = changed.get_mut::<Pos>(e) {
        p.0 = v(99, 99);
    }
    assert_ne!(changed.hash(), build(false).hash());
}

const PINNED: u64 = 0x22fe_28ab_ebee_a91f;

fn rand_vec(rng: &mut Rng) -> Vec2 {
    let x = rng.range(0, 2_000_000) as i64 - 1_000_000;
    let y = rng.range(0, 2_000_000) as i64 - 1_000_000;
    v(x, y)
}

/// Seeded simulation: 1000 entities, random spawn/despawn/insert/remove, movement on Vec2, 600 steps.
fn simulate(seed: u64, trace: &mut Vec<u64>) -> u64 {
    let mut w = World::new();
    let mut rng = Rng::new(seed);
    let mut ents: Vec<Entity> = Vec::new();
    for _ in 0..1000 {
        let e = w.spawn();
        assert!(w.insert(e, Pos(rand_vec(&mut rng))));
        ents.push(e);
    }
    for _ in 0..600 {
        for _ in 0..50 {
            let i = rng.below(ents.len());
            let Some(&e) = ents.get(i) else {
                continue;
            };
            match rng.below(5) {
                0 => {
                    w.despawn(e);
                    let n = w.spawn();
                    assert!(w.insert(n, Pos(rand_vec(&mut rng))));
                    if let Some(slot) = ents.get_mut(i) {
                        *slot = n;
                    }
                }
                1 => {
                    w.insert(e, Vel(rand_vec(&mut rng)));
                }
                2 => {
                    w.remove::<Vel>(e);
                }
                3 => {
                    w.insert(e, Pos(rand_vec(&mut rng)));
                }
                _ => {
                    w.insert(e, Tag(rng.u64() as u32));
                }
            }
        }
        w.each2::<Pos, Vel>(|_, p, vel| {
            p.0 = p.0 + vel.0;
        });
        trace.push(w.hash());
    }
    w.hash()
}

#[test]
fn seeded_simulation_is_deterministic_and_pinned() {
    let mut t1 = Vec::new();
    let h1 = simulate(1234, &mut t1);
    let mut t2 = Vec::new();
    let h2 = simulate(1234, &mut t2);
    assert_eq!(t1.len(), 600);
    assert_eq!(t1, t2);
    assert_eq!(h1, h2);
    let mut t3 = Vec::new();
    let h3 = simulate(999, &mut t3);
    assert_ne!(h1, h3);
    assert_eq!(h1, PINNED, "final hash is {h1:#x}");
}
