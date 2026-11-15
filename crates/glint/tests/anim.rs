//! Unit and property tests for `glint::anim`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use glint::anim::{ease, tween, Ease, Spring};

const CURVES: [Ease; 5] = [
    Ease::Linear,
    Ease::InQuad,
    Ease::OutQuad,
    Ease::InOutQuad,
    Ease::OutCubic,
];

#[test]
fn ease_hand_worked_values() {
    assert_eq!(ease(Ease::Linear, 300), 300);
    assert_eq!(ease(Ease::InQuad, 500), 250);
    assert_eq!(ease(Ease::OutQuad, 500), 750);
    assert_eq!(ease(Ease::InOutQuad, 250), 125);
    assert_eq!(ease(Ease::InOutQuad, 750), 875);
    assert_eq!(ease(Ease::InOutQuad, 500), 500);
    assert_eq!(ease(Ease::OutCubic, 500), 875);
}

#[test]
fn ease_endpoints_and_clamp() {
    for e in CURVES {
        assert_eq!(ease(e, 0), 0, "{e:?} at 0");
        assert_eq!(ease(e, 1000), 1000, "{e:?} at 1000");
        assert_eq!(ease(e, 5000), 1000, "{e:?} clamps high");
    }
}

#[test]
fn ease_stays_in_range_and_is_monotone() {
    for e in CURVES {
        let mut prev = 0;
        for t in 0..=1000 {
            let v = ease(e, t);
            assert!(v <= 1000);
            assert!(v >= prev, "{e:?} not monotone at {t}");
            prev = v;
        }
    }
}

#[test]
fn tween_hand_worked_values() {
    // Linear 0 -> 100 over [1000, 1500).
    assert_eq!(tween(0, 100, 1000, 500, 900, Ease::Linear), 0);
    assert_eq!(tween(0, 100, 1000, 500, 1000, Ease::Linear), 0);
    assert_eq!(tween(0, 100, 1000, 500, 1250, Ease::Linear), 50);
    assert_eq!(tween(0, 100, 1000, 500, 1500, Ease::Linear), 100);
    assert_eq!(tween(0, 100, 1000, 500, 2000, Ease::Linear), 100);
    // OutQuad at the midpoint is 75.
    assert_eq!(tween(0, 100, 1000, 500, 1250, Ease::OutQuad), 75);
    // Downward tween and truncation toward zero: -7 * 500 / 1000 = -3.5 -> -3.
    assert_eq!(tween(100, 0, 0, 1000, 500, Ease::Linear), 50);
    assert_eq!(tween(0, -7, 0, 1000, 500, Ease::Linear), -3);
    // now <= start is checked first, so zero duration only jumps once time has passed.
    assert_eq!(tween(5, 9, 10, 0, 10, Ease::InQuad), 5);
    assert_eq!(tween(5, 9, 10, 0, 11, Ease::InQuad), 9);
}

#[test]
fn tween_bounds_and_extremes() {
    forall(2000, 0x7001, |rng| {
        let from = rng.u64() as i32;
        let to = rng.u64() as i32;
        let start = rng.u64();
        let dur = rng.u64();
        let now = rng.u64();
        let e = *rng.pick(&CURVES);
        let v = i64::from(tween(from, to, start, dur, now, e));
        let lo = i64::from(from.min(to));
        let hi = i64::from(from.max(to));
        assert!(v >= lo && v <= hi, "{from} {to} -> {v}");
    });
}

#[test]
fn spring_hand_worked_steps() {
    let mut s = Spring {
        pos: 0,
        vel: 0,
        target: 1000,
        stiffness: 100,
        damping: 0,
    };
    s.step(1000);
    assert_eq!((s.pos, s.vel), (100, 100));
    s.step(1000);
    assert_eq!((s.pos, s.vel), (290, 190));
    s.step(0);
    assert_eq!((s.pos, s.vel), (290, 190));
}

#[test]
fn spring_settled_rule() {
    let base = Spring {
        pos: 1000,
        vel: 0,
        target: 1000,
        stiffness: 0,
        damping: 0,
    };
    assert!(base.settled());
    assert!(Spring { pos: 999, ..base }.settled());
    assert!(!Spring { pos: 998, ..base }.settled());
    assert!(!Spring { vel: 2, ..base }.settled());
    assert!(Spring { vel: -1, ..base }.settled());
}

#[test]
fn spring_step_never_panics() {
    forall(1000, 0x7002, |rng| {
        let mut s = Spring {
            pos: rng.u64() as i64,
            vel: rng.u64() as i64,
            target: rng.u64() as i64,
            stiffness: rng.u64() as i64,
            damping: rng.u64() as i64,
        };
        for _ in 0..4 {
            s.step(rng.u64() as u32);
        }
        let _ = s.settled();
    });
}
