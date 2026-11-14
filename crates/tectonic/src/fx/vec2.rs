//! Two-component fixed-point vector.
use super::{isqrt, Fx};
use core::ops::{Add, Neg, Sub};

/// 2-D vector of fixed-point components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Vec2 {
    /// X component.
    pub x: Fx,
    /// Y component.
    pub y: Fx,
}

impl Vec2 {
    /// Vector from its components.
    pub fn new(x: Fx, y: Fx) -> Vec2 {
        Vec2 { x, y }
    }

    /// Dot product.
    pub fn dot(self, o: Vec2) -> Fx {
        self.x * o.x + self.y * o.y
    }

    /// 2-D cross product (z component).
    pub fn cross(self, o: Vec2) -> Fx {
        self.x * o.y - self.y * o.x
    }

    /// Euclidean length, floor-rounded, computed without intermediate overflow.
    pub fn len(self) -> Fx {
        let x = u128::from(self.x.0.unsigned_abs());
        let y = u128::from(self.y.0.unsigned_abs());
        Fx(isqrt(x * x + y * y).min(i64::MAX as u128) as i64)
    }

    /// Unit vector in the same direction; the zero vector stays zero.
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l == Fx::ZERO {
            return Vec2::new(Fx::ZERO, Fx::ZERO);
        }
        Vec2::new(self.x / l, self.y / l)
    }

    /// Each component multiplied by `k`.
    pub fn scale(self, k: Fx) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
