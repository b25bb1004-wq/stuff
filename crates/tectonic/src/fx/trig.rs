//! Sine, cosine and arctangent. Internal work is at scale 2^30; the result is rounded to Q47.16.
use super::{isqrt, round_div, Fx};

/// Pi times 2^60, rounded (computed to 99 digits, then rounded).
const PI60: i128 = 3_622_009_729_038_561_421;
/// Two pi times 2^60, rounded.
const TAU60: i128 = 7_244_019_458_077_122_842;
/// Pi times 2^30, truncated.
const PI30: i128 = PI60 >> 30;
/// Pi over two at scale 2^30.
const HALF_PI30: i128 = PI30 / 2;
/// One at scale 2^30.
const ONE30: i128 = 1 << 30;

/// Product of two scale-2^30 values, result at scale 2^30.
fn mul30(a: i128, b: i128) -> i128 {
    (a * b) >> 30
}

/// Scale-2^30 value to Fx, rounded.
fn to_fx(v: i128) -> Fx {
    Fx(round_div(v, 1 << 14) as i64)
}

/// Reduce `x` modulo 2 pi to [-pi, pi], at scale 2^30.
fn reduce(x: Fx) -> i128 {
    let a = i128::from(x.0) << 44; // scale 2^60
    let k = round_div(a, TAU60);
    round_div(a - k * TAU60, 1 << 30)
}

/// Taylor series for sin (or cos) of `theta`, scale 2^30. Converges well within 16 terms for |theta| <= pi.
fn taylor(theta: i128, cos: bool) -> i128 {
    let t2 = mul30(theta, theta);
    let (mut term, mut sum) = if cos { (ONE30, ONE30) } else { (theta, theta) };
    for n in 1..=16i128 {
        let (a, b) = if cos {
            (2 * n - 1, 2 * n)
        } else {
            (2 * n, 2 * n + 1)
        };
        term = -(mul30(term, t2) / (a * b));
        sum += term;
    }
    sum
}

/// Arctangent series for small `u` (|u| <= 0.1), scale 2^30.
fn atan_series(u: i128) -> i128 {
    let u2 = mul30(u, u);
    let (mut term, mut sum) = (u, u);
    for n in 1..=12i128 {
        term = -mul30(term, u2);
        sum += term / (2 * n + 1);
    }
    sum
}

/// Half-angle step: atan(t) = 2 atan(t / (1 + sqrt(1 + t^2))), scale 2^30.
fn half_angle(t: i128) -> i128 {
    let raw = ONE30 + mul30(t, t);
    let root = isqrt((raw as u128) << 30) as i128;
    round_div(t << 30, ONE30 + root)
}

/// Arctangent of `t` in [0, 1], scale 2^30. Three half-angle steps bring it under 0.1.
fn atan01(t: i128) -> i128 {
    let mut u = t;
    for _ in 0..3 {
        u = half_angle(u);
    }
    atan_series(u) << 3
}

impl Fx {
    /// Sine of `self` in radians. Error at most 2/65536 for |x| <= 1000.
    pub fn sin(self) -> Fx {
        to_fx(taylor(reduce(self), false))
    }

    /// Cosine of `self` in radians. Error at most 2/65536 for |x| <= 1000.
    pub fn cos(self) -> Fx {
        to_fx(taylor(reduce(self), true))
    }

    /// Angle of the vector (x, y) in (-PI, PI]; `atan2(0, 0)` is zero. Error at most 4/65536.
    pub fn atan2(y: Fx, x: Fx) -> Fx {
        if x == Fx::ZERO && y == Fx::ZERO {
            return Fx::ZERO;
        }
        let ax = i128::from(x.0).abs();
        let ay = i128::from(y.0).abs();
        let angle = if ay <= ax {
            atan01(round_div(ay << 30, ax))
        } else {
            HALF_PI30 - atan01(round_div(ax << 30, ay))
        };
        let angle = if x.0 < 0 { PI30 - angle } else { angle };
        let angle = if y.0 < 0 { -angle } else { angle };
        to_fx(angle)
    }
}
