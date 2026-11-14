//! Leader check for T68: fixed-point error sweep, exhaustive sqrt over a range, PCG reference,
//! determinism with per-entity insertion order permuted, stale handles.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use tectonic::ecs::{Entity, World};
use tectonic::fx::{Fx, Vec2};
use tectonic::rng::Pcg32;
use tectonic::Clock;

const S: f64 = 65536.0;

fn f(x: Fx) -> f64 {
    x.0 as f64 / S
}

#[test]
fn leader_fixed_point_sweep() {
    let mut r = Rng::new(0x68);
    for _ in 0..200_000 {
        let a = Fx((r.u64() as i64) >> 30);
        let b = Fx(((r.u64() as i64) >> 34) | 1);
        let m = (a.0 as i128 * b.0 as i128) as f64 / S;
        if m.abs() < 1e12 {
            assert!((a.mul(b).0 as f64 - m).abs() <= 0.5 + 1e-6, "{a:?}*{b:?}");
        }
        let d = a.0 as f64 * S / b.0 as f64;
        if d.abs() < 1e12 {
            assert!((a.div(b).0 as f64 - d).abs() <= 0.5 + 1e-3, "{a:?}/{b:?}");
        }
    }
    assert_eq!(Fx(i64::MAX).mul(Fx::from_int(2)), Fx(i64::MAX));
    assert_eq!(Fx(i64::MIN).mul(Fx::from_int(2)), Fx(i64::MIN));
    assert_eq!(Fx::ONE.div(Fx::ZERO), Fx(i64::MAX));
    assert_eq!(Fx::ZERO.div(Fx::ZERO), Fx::ZERO);
    assert_eq!(Fx::from_ratio(1, 3).0, 21845);
    assert_eq!(Fx::from_ratio(-1, 2), -Fx::HALF);
    assert_eq!(
        Fx::from_ratio(1, 131072).0,
        1,
        "half of one unit rounds away from zero"
    );
    assert_eq!(Fx::from_ratio(-1, 131072).0, -1);
    assert_eq!(Fx::PI.0, 205887);
    assert_eq!(Fx(-1).to_int(), -1);

    for raw in (-1000 * 65536..=1000 * 65536).step_by(977) {
        let x = Fx(raw);
        let (s, c) = (f(x).sin(), f(x).cos());
        assert!((f(x.sin()) - s).abs() <= 2.0 / S + 1e-9, "sin {raw}");
        assert!((f(x.cos()) - c).abs() <= 2.0 / S + 1e-9, "cos {raw}");
    }
    for _ in 0..20_000 {
        let y = Fx((r.u64() as i64) >> 40);
        let x = Fx((r.u64() as i64) >> 40);
        let got = f(Fx::atan2(y, x));
        let want = f(y).atan2(f(x));
        if !(y == Fx::ZERO && x == Fx::ZERO) {
            let diff = (got - want).abs();
            let wrap = (diff - 2.0 * std::f64::consts::PI).abs();
            assert!(
                diff.min(wrap) <= 4.0 / S + 1e-9,
                "atan2({y:?},{x:?}) {got} vs {want}"
            );
            assert!(got > -std::f64::consts::PI - 1e-9 && got <= std::f64::consts::PI + 4.0 / S);
        }
    }
    assert_eq!(Fx::atan2(Fx::ZERO, Fx::ZERO), Fx::ZERO);
    let v = Vec2::new(Fx::from_int(3), Fx::from_int(4));
    assert_eq!(v.len(), Fx::from_int(5));
    assert_eq!(Vec2::default().normalized(), Vec2::default());
}

#[test]
fn leader_sqrt_exhaustive_range() {
    // Every raw value in [0, 2^22) plus a sparse sweep to the top: floor of the exact root.
    let check = |raw: i64| {
        let r = Fx(raw).sqrt().0 as i128;
        let x = raw as i128 * 65536; // root of raw/2^16, scaled by 2^16, squared = raw * 2^16
        assert!(
            r * r <= x && (r + 1) * (r + 1) > x,
            "sqrt raw {raw} gave {r}"
        );
    };
    for raw in 0..(1 << 22) {
        check(raw);
    }
    let mut x: i64 = 1 << 22;
    while x < i64::MAX / 3 {
        check(x);
        check(x - 1);
        check(x + 1);
        x += x / 7 + 1;
    }
    check(i64::MAX);
    assert_eq!(Fx(-5).sqrt(), Fx::ZERO);
}

#[test]
fn leader_pcg_reference_and_bound() {
    let mut p = Pcg32::new(42, 54);
    let want = [
        0xa15c02b7u32,
        0x7b47f409,
        0xba1d3330,
        0x83d2f293,
        0xbfa4784b,
        0xcbed606e,
    ];
    for w in want {
        assert_eq!(p.next_u32(), w);
    }
    // n = 3 * 2^30: naive `% n` puts half the draws below 2^30 instead of a third.
    let n = 3u32 << 30;
    let low = (0..30_000).filter(|_| p.below(n) < 1 << 30).count() as f64 / 30_000.0;
    assert!((low - 1.0 / 3.0).abs() < 0.02, "bias: {low}");
    assert!((0..1000).all(|_| p.below(1) == 0));
}

#[derive(Clone, Hash, PartialEq, Debug)]
struct Pos(Vec2);
#[derive(Clone, Hash, PartialEq, Debug)]
struct Vel(Vec2);
#[derive(Clone, Hash, PartialEq, Debug)]
struct Tag(u32);

fn sim(seed: u64, permute: bool) -> Vec<u64> {
    let mut w = World::new();
    let mut r = Rng::new(seed);
    let mut order = Rng::new(seed ^ 0xabc);
    let mut live: Vec<Entity> = Vec::new();
    // Register types in a fixed order once, so later per-entity order may vary.
    let e0 = w.spawn();
    w.insert(e0, Pos(Vec2::default()));
    w.insert(e0, Vel(Vec2::default()));
    w.insert(e0, Tag(0));
    live.push(e0);
    let mut hashes = Vec::new();
    for step in 0..400u32 {
        for _ in 0..r.range(0, 6) {
            let e = w.spawn();
            let p = Pos(Vec2::new(
                Fx(r.u64() as i64 >> 40),
                Fx(r.u64() as i64 >> 40),
            ));
            let v = Vel(Vec2::new(
                Fx(r.u64() as i64 >> 48),
                Fx(r.u64() as i64 >> 48),
            ));
            let t = Tag(step);
            if permute && order.chance(1, 2) {
                w.insert(e, t);
                w.insert(e, v);
                w.insert(e, p);
            } else {
                w.insert(e, p);
                w.insert(e, v);
                w.insert(e, t);
            }
            live.push(e);
        }
        if live.len() > 3 && r.chance(1, 3) {
            let i = r.below(live.len());
            assert!(w.despawn(live.swap_remove(i)));
        }
        if !live.is_empty() && r.chance(1, 4) {
            let e = live[r.below(live.len())];
            let _ = w.remove::<Tag>(e);
        }
        w.each2::<Pos, Vel>(|_, p, v| {
            p.0 = p.0 + v.0;
            v.0 = v.0.scale(Fx::from_ratio(99, 100));
        });
        hashes.push(w.hash());
    }
    hashes
}

#[test]
fn leader_determinism_and_stale_handles() {
    let a = sim(1, false);
    assert_eq!(a, sim(1, false));
    assert_eq!(
        a,
        sim(1, true),
        "per-entity insertion order changed the hash"
    );
    assert_ne!(a.last(), sim(2, false).last());

    let mut w = World::new();
    let e = w.spawn();
    w.insert(e, Tag(7));
    assert!(w.despawn(e));
    assert!(!w.despawn(e));
    let e2 = w.spawn();
    assert_eq!(e2.index, e.index);
    assert_ne!(e2.generation, e.generation);
    assert!(w.get::<Tag>(e2).is_none(), "components survived despawn");
    assert!(!w.insert(e, Tag(1)), "insert through a stale handle");
    assert!(w.get::<Tag>(e).is_none() && !w.alive(e));
    w.insert(e2, Tag(2));
    assert!(
        w.get::<Tag>(e).is_none(),
        "stale handle sees the new entity"
    );
    assert_eq!(w.remove::<Tag>(e2), Some(Tag(2)));
    let order: Vec<u32> = {
        let mut w = World::new();
        let es: Vec<Entity> = (0..10).map(|_| w.spawn()).collect();
        for &x in es.iter().rev() {
            w.insert(x, Tag(x.index));
        }
        w.despawn(es[3]);
        let mut seen = Vec::new();
        w.each::<Tag>(|_, t| seen.push(t.0));
        seen
    };
    assert_eq!(order, vec![0, 1, 2, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn leader_clock() {
    let mut c = Clock::new(16_667, 5);
    let mut steps = 0u32;
    let mut r = Rng::new(3);
    let mut total = 0u64;
    for _ in 0..3000 {
        let dt = 10_000 + r.below(13_000) as u64;
        total += dt;
        steps += c.advance(dt);
        let a = c.alpha();
        assert!(a >= Fx::ZERO && a < Fx::ONE);
    }
    assert_eq!(u64::from(steps), total / 16_667);
    assert_eq!(c.advance(10_000_000), 5, "cap");
    assert!(c.advance(0) == 0 && c.alpha() < Fx::ONE);
}
