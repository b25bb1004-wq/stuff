//! Q47.16 fixed-point scalar. Integer-only; rounding is to nearest, ties away from zero.
use core::cmp::Ordering;
use core::ops::{Add, Div, Mul, Neg, Sub};

#[cfg(test)]
mod tests;
mod trig;
mod vec2;

pub use vec2::Vec2;

/// Fixed-point number in Q47.16: the value is `raw / 65536`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fx(
    /// Raw value, scaled by 65536.
    pub i64,
);

impl Fx {
    /// Zero.
    pub const ZERO: Fx = Fx(0);
    /// One.
    pub const ONE: Fx = Fx(1 << 16);
    /// One half.
    pub const HALF: Fx = Fx(1 << 15);
    /// Pi, `round(pi * 65536)`; checked against core::f64::consts in the tests.
    pub const PI: Fx = Fx(205_887);

    /// The whole number `n`.
    pub const fn from_int(n: i32) -> Fx {
        Fx((n as i64) << 16)
    }

    /// `num / den` rounded to nearest, ties away from zero. Saturates; `den == 0` saturates by the sign of `num`.
    pub fn from_ratio(num: i64, den: i64) -> Fx {
        if den == 0 {
            return by_sign(num);
        }
        sat(round_div(i128::from(num) << 16, i128::from(den)))
    }

    /// Product rounded to nearest (ties away from zero), saturating.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, o: Fx) -> Fx {
        sat(round_div(i128::from(self.0) * i128::from(o.0), 1 << 16))
    }

    /// Quotient rounded to nearest (ties away from zero), saturating. `x / 0` saturates by the sign of `x`; `0 / 0` is zero.
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, o: Fx) -> Fx {
        if o.0 == 0 {
            return by_sign(self.0);
        }
        sat(round_div(i128::from(self.0) << 16, i128::from(o.0)))
    }

    /// Square root rounded down (floor of the exact root); negative input gives zero.
    pub fn sqrt(self) -> Fx {
        if self.0 <= 0 {
            return Fx::ZERO;
        }
        let root = isqrt((self.0 as u128) << 16);
        Fx(root.min(i64::MAX as u128) as i64)
    }

    /// Absolute value, saturating at the maximum.
    pub fn abs(self) -> Fx {
        Fx(self.0.saturating_abs())
    }

    /// Largest whole number not above `self`.
    pub fn floor(self) -> Fx {
        Fx((self.0 >> 16) << 16)
    }

    /// Nearest whole number; halves go away from zero.
    pub fn round(self) -> Fx {
        let base = self.0 >> 16;
        let frac = self.0 & 0xFFFF;
        let up = frac > 0x8000 || (frac == 0x8000 && self.0 >= 0);
        Fx((base + i64::from(up)) << 16)
    }

    /// Whole part, rounded down.
    pub fn to_int(self) -> i64 {
        self.0 >> 16
    }
}

impl Add for Fx {
    type Output = Fx;
    fn add(self, o: Fx) -> Fx {
        Fx(self.0.saturating_add(o.0))
    }
}

impl Sub for Fx {
    type Output = Fx;
    fn sub(self, o: Fx) -> Fx {
        Fx(self.0.saturating_sub(o.0))
    }
}

impl Neg for Fx {
    type Output = Fx;
    fn neg(self) -> Fx {
        Fx(self.0.saturating_neg())
    }
}

impl Mul for Fx {
    type Output = Fx;
    fn mul(self, o: Fx) -> Fx {
        Fx::mul(self, o)
    }
}

impl Div for Fx {
    type Output = Fx;
    fn div(self, o: Fx) -> Fx {
        Fx::div(self, o)
    }
}

/// Clamp a wide intermediate into the Fx range.
fn sat(v: i128) -> Fx {
    Fx(v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

/// Saturated sign-only value: +MAX, -MIN, or zero.
fn by_sign(v: i64) -> Fx {
    match v.cmp(&0) {
        Ordering::Greater => Fx(i64::MAX),
        Ordering::Less => Fx(i64::MIN),
        Ordering::Equal => Fx::ZERO,
    }
}

/// `n / d` rounded to nearest, ties away from zero. Returns 0 when `d` is 0 (callers guard that).
pub(crate) fn round_div(n: i128, d: i128) -> i128 {
    if d == 0 {
        return 0;
    }
    let q = n / d;
    let r = n % d;
    if r == 0 {
        return q;
    }
    let negative = (n < 0) != (d < 0);
    if r.unsigned_abs() * 2 >= d.unsigned_abs() {
        if negative {
            q - 1
        } else {
            q + 1
        }
    } else {
        q
    }
}

/// Floor of the square root of `n` (digit-by-digit method, no floats).
pub(crate) fn isqrt(n: u128) -> u128 {
    let mut rem = n;
    let mut res: u128 = 0;
    let mut bit: u128 = 1 << 126;
    while bit > rem {
        bit >>= 2;
    }
    while bit != 0 {
        if rem >= res + bit {
            rem -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}
