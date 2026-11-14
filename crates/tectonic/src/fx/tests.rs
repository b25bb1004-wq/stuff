#![allow(clippy::unwrap_used)]
use super::*;
use assay::{forall, Rng};

#[test]
fn constants_and_pi() {
    assert_eq!(Fx::ZERO, Fx(0));
    assert_eq!(Fx::ONE, Fx(65_536));
    assert_eq!(Fx::HALF, Fx(32_768));
    assert_eq!(Fx::from_int(-3), Fx(-3 * 65_536));
    let exact = core::f64::consts::PI * 65_536.0;
    assert!((Fx::PI.0 as f64 - exact).abs() <= 0.5);
}

#[test]
fn mul_table() {
    let cases: &[(i64, i64, i64)] = &[
        (65_536, 65_536, 65_536),
        (32_768, 32_768, 16_384),
        (3 * 65_536, 4 * 65_536, 12 * 65_536),
        (1, 1, 0),
        (32_768, 1, 1),
        (-32_768, 1, -1),
        (32_767, 1, 0),
        (1, -32_768, -1),
        (-1, -1, 0),
        (98_304, 98_304, 147_456),
        (65_537, 65_536, 65_537),
        (0, 7 * 65_536, 0),
        (65_536, 0, 0),
        (3, 21_845, 1),
        (i64::MIN, 65_536, i64::MIN),
        (i64::MIN, -65_536, i64::MAX),
        (i64::MAX, 131_072, i64::MAX),
        (i64::MIN, i64::MIN, i64::MAX),
        (i64::MAX, i64::MIN, i64::MIN),
        (-5 * 65_536, 3 * 65_536, -15 * 65_536),
        (-5 * 65_536, -3 * 65_536, 15 * 65_536),
        (1, i64::MAX, 1 << 47),
    ];
    for &(a, b, want) in cases {
        assert_eq!(Fx(a).mul(Fx(b)), Fx(want), "mul {a} {b}");
        assert_eq!(Fx(a) * Fx(b), Fx(want), "operator mul {a} {b}");
    }
}

#[test]
fn div_table() {
    let cases: &[(i64, i64, i64)] = &[
        (65_536, 65_536, 65_536),
        (65_536, 131_072, 32_768),
        (65_536, 196_608, 21_845),
        (-65_536, 196_608, -21_845),
        (65_536, -196_608, -21_845),
        (-65_536, -196_608, 21_845),
        (1, 131_072, 1),
        (1, 196_608, 0),
        (2, 4, 32_768),
        (65_536, 262_144, 16_384),
        (131_072, 65_536, 131_072),
        (0, 65_536, 0),
        (0, 0, 0),
        (65_536, 0, i64::MAX),
        (-65_536, 0, i64::MIN),
        (i64::MAX, 32_768, i64::MAX),
        (i64::MIN, -1, i64::MAX),
        (1000 * 65_536, 7 * 65_536, 9_362_286),
        (65_536, 1, 1 << 32),
        (i64::MAX, 1, i64::MAX),
        (196_608, 131_072, 98_304),
        (65_536, 3, 1_431_655_765),
    ];
    for &(a, b, want) in cases {
        assert_eq!(Fx(a).div(Fx(b)), Fx(want), "div {a} {b}");
        assert_eq!(Fx(a) / Fx(b), Fx(want), "operator div {a} {b}");
    }
}

#[test]
fn from_ratio_table() {
    let cases: &[(i64, i64, i64)] = &[
        (1, 2, 32_768),
        (1, 3, 21_845),
        (2, 3, 43_691),
        (-1, 3, -21_845),
        (1, -3, -21_845),
        (-1, -3, 21_845),
        (1, 131_072, 1),
        (-1, 131_072, -1),
        (1, 131_073, 0),
        (0, 5, 0),
        (5, 1, 5 * 65_536),
        (i64::MAX, 1, i64::MAX),
        (i64::MIN, 1, i64::MIN),
        (i64::MAX, -1, i64::MIN),
        (7, 0, i64::MAX),
        (-7, 0, i64::MIN),
        (0, 0, 0),
        (3, 4, 49_152),
        (1, 65_536, 1),
    ];
    for &(n, d, want) in cases {
        assert_eq!(Fx::from_ratio(n, d), Fx(want), "from_ratio {n} {d}");
    }
}

#[test]
fn add_sub_neg_saturate() {
    assert_eq!(Fx(i64::MAX) + Fx::ONE, Fx(i64::MAX));
    assert_eq!(Fx(i64::MIN) - Fx::ONE, Fx(i64::MIN));
    assert_eq!(-Fx(i64::MIN), Fx(i64::MAX));
    assert_eq!(Fx::from_int(2) - Fx::from_int(5), Fx::from_int(-3));
}

#[test]
fn floor_round_abs_int() {
    assert_eq!(Fx(-32_768).floor(), Fx(-65_536));
    assert_eq!(Fx(98_303).floor(), Fx(65_536));
    assert_eq!(Fx(32_768).round(), Fx(65_536));
    assert_eq!(Fx(-32_768).round(), Fx(-65_536));
    assert_eq!(Fx(32_767).round(), Fx::ZERO);
    assert_eq!(Fx(98_304).round(), Fx(131_072));
    assert_eq!(Fx(i64::MIN).abs(), Fx(i64::MAX));
    assert_eq!(Fx(-5).abs(), Fx(5));
    assert_eq!(Fx(-32_768).to_int(), -1);
    assert_eq!(Fx(98_304).to_int(), 1);
}

#[test]
fn sqrt_small_exact_values() {
    assert_eq!(Fx::from_int(4).sqrt(), Fx::from_int(2));
    assert_eq!(Fx::ONE.sqrt(), Fx::ONE);
    assert_eq!(Fx::HALF.sqrt(), Fx(46_340));
    assert_eq!(Fx::ZERO.sqrt(), Fx::ZERO);
    assert_eq!(Fx(-5).sqrt(), Fx::ZERO);
    assert_eq!(Fx(1).sqrt(), Fx(256));
}

#[test]
fn sqrt_is_exact_floor_on_random_values() {
    forall(10_000, 11, |rng: &mut Rng| {
        let raw = (rng.u64() >> rng.range(1, 64)) as i64;
        let r = i128::from(Fx(raw).sqrt().0);
        let x = i128::from(raw) << 16;
        assert!(r * r <= x && x < (r + 1) * (r + 1), "raw {raw}");
    });
}

#[test]
fn sqrt_of_max_is_floor() {
    let r = i128::from(Fx(i64::MAX).sqrt().0);
    let x = i128::from(i64::MAX) << 16;
    assert!(r * r <= x && x < (r + 1) * (r + 1));
}

#[test]
fn sin_cos_error_bounds_over_20000_points() {
    let bound = 2.0 / 65_536.0;
    forall(20_000, 7, |rng: &mut Rng| {
        let raw = (rng.range(0, 2_000 * 65_536) as i64) - 1_000 * 65_536;
        let xf = raw as f64 / 65_536.0;
        let s = Fx(raw).sin().0 as f64 / 65_536.0;
        let c = Fx(raw).cos().0 as f64 / 65_536.0;
        assert!((s - f64::sin(xf)).abs() <= bound, "sin raw {raw}");
        assert!((c - f64::cos(xf)).abs() <= bound, "cos raw {raw}");
    });
}

#[test]
fn trig_exact_points() {
    assert_eq!(Fx::ZERO.sin(), Fx::ZERO);
    assert_eq!(Fx::ZERO.cos(), Fx::ONE);
    assert_eq!(Fx::atan2(Fx::ZERO, Fx::ZERO), Fx::ZERO);
    assert_eq!(Fx::atan2(Fx::ZERO, Fx::ONE), Fx::ZERO);
    assert_eq!(Fx::atan2(Fx::ZERO, Fx(-65_536)), Fx::PI);
    let half_pi = core::f64::consts::FRAC_PI_2 * 65_536.0;
    assert!((Fx::atan2(Fx::ONE, Fx::ZERO).0 as f64 - half_pi).abs() <= 1.0);
    assert!((Fx::atan2(Fx(-65_536), Fx::ZERO).0 as f64 + half_pi).abs() <= 1.0);
}

#[test]
fn atan2_error_bound_over_20000_points() {
    let bound = 4.0 / 65_536.0;
    forall(20_000, 9, |rng: &mut Rng| {
        let y = (rng.range(0, 2_000 * 65_536) as i64) - 1_000 * 65_536;
        let x = (rng.range(0, 2_000 * 65_536) as i64) - 1_000 * 65_536;
        let want = f64::atan2(y as f64 / 65_536.0, x as f64 / 65_536.0);
        let got = Fx::atan2(Fx(y), Fx(x)).0 as f64 / 65_536.0;
        assert!((got - want).abs() <= bound, "atan2 y {y} x {x}");
    });
}

#[test]
fn vec2_basics() {
    let a = Vec2::new(Fx::from_int(3), Fx::from_int(4));
    assert_eq!(a.len(), Fx::from_int(5));
    assert_eq!(a.dot(Vec2::new(Fx::ONE, Fx::ONE)), Fx::from_int(7));
    assert_eq!(a.cross(Vec2::new(Fx::ONE, Fx::ZERO)), Fx::from_int(-4));
    assert_eq!(a.scale(Fx::HALF), Vec2::new(Fx(98_304), Fx::from_int(2)));
    assert_eq!(Vec2::default().normalized(), Vec2::default());
    let n = a.normalized();
    assert!((n.len().0 - 65_536).abs() <= 2);
    assert_eq!(a + a - a, a);
    assert_eq!(-a, Vec2::new(Fx::from_int(-3), Fx::from_int(-4)));
}
