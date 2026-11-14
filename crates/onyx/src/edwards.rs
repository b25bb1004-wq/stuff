//! Points on Edwards25519 in extended coordinates (RFC 8032 section 5.1).

use crate::field::{Fe, EXP_P14, EXP_P58};

/// A point (X:Y:Z:T) with x = X/Z, y = Y/Z and x*y = T/Z.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

/// Encoding of the base point: y = 4/5 with a positive x.
const BASE_ENC: [u8; 32] = {
    let mut b = [0x66u8; 32];
    b[0] = 0x58;
    b
};

/// The curve constant d = -121665/121666.
pub(crate) fn curve_d() -> Fe {
    Fe::from_u64(121_665)
        .neg()
        .mul(&Fe::from_u64(121_666).invert())
}

/// 2d, the constant the addition formula takes. Compute once per scalar multiplication.
pub(crate) fn curve_d2() -> Fe {
    let d = curve_d();
    d.add(&d)
}

impl Point {
    /// The neutral element (0, 1).
    pub(crate) fn identity() -> Point {
        Point {
            x: Fe::ZERO,
            y: Fe::ONE,
            z: Fe::ONE,
            t: Fe::ZERO,
        }
    }

    /// The Ed25519 base point B.
    pub(crate) fn base() -> Point {
        decode(&BASE_ENC).unwrap_or_else(Point::identity)
    }

    /// Unified addition (valid for every pair of points, including doubling and the identity).
    pub(crate) fn add(&self, o: &Point, d2: &Fe) -> Point {
        let a = self.y.sub(&self.x).mul(&o.y.sub(&o.x));
        let b = self.y.add(&self.x).mul(&o.y.add(&o.x));
        let c = self.t.mul(d2).mul(&o.t);
        let dd = self.z.mul(&o.z);
        let dd = dd.add(&dd);
        let e = b.sub(&a);
        let f = dd.sub(&c);
        let g = dd.add(&c);
        let h = b.add(&a);
        Point {
            x: e.mul(&f),
            y: g.mul(&h),
            t: e.mul(&h),
            z: f.mul(&g),
        }
    }

    /// Doubling.
    pub(crate) fn double(&self) -> Point {
        let a = self.x.square();
        let b = self.y.square();
        let zz = self.z.square();
        let c = zz.add(&zz);
        let d = a.neg();
        let e = self.x.add(&self.y).square().sub(&a).sub(&b);
        let g = d.add(&b);
        let f = g.sub(&c);
        let h = d.sub(&b);
        Point {
            x: e.mul(&f),
            y: g.mul(&h),
            t: e.mul(&h),
            z: f.mul(&g),
        }
    }

    /// `a` when `bit` is 0, `b` when `bit` is 1. `bit` must be 0 or 1.
    pub(crate) fn select(a: &Point, b: &Point, bit: u64) -> Point {
        Point {
            x: Fe::cmov(&a.x, &b.x, bit),
            y: Fe::cmov(&a.y, &b.y, bit),
            z: Fe::cmov(&a.z, &b.z, bit),
            t: Fe::cmov(&a.t, &b.t, bit),
        }
    }

    /// `[k]self` for a 256-bit little-endian scalar. Double-and-add-always with a masked
    /// select: the same operations run for every bit, and no branch or index uses a bit of `k`.
    pub(crate) fn scalar_mul(&self, k: &[u8; 32]) -> Point {
        let d2 = curve_d2();
        let mut r = Point::identity();
        for byte in k.iter().rev() {
            for bit in (0..8).rev() {
                r = r.double();
                let t = r.add(self, &d2);
                r = Point::select(&r, &t, u64::from((byte >> bit) & 1));
            }
        }
        r
    }

    /// Canonical 32-byte encoding: y little-endian, with the sign of x in the top bit.
    pub(crate) fn encode(&self) -> [u8; 32] {
        let zinv = self.z.invert();
        let x = self.x.mul(&zinv);
        let y = self.y.mul(&zinv);
        let mut out = y.to_bytes();
        out[31] |= x.is_odd() << 7;
        out
    }
}

/// Decode a point (RFC 8032 section 5.1.3). Public data only, so branches are allowed.
/// Rejects a non-canonical y, a y with no matching x, and the sign bit set on x = 0.
pub(crate) fn decode(enc: &[u8; 32]) -> Option<Point> {
    let sign = enc[31] >> 7;
    let y = Fe::from_bytes(enc);
    let mut canonical = y.to_bytes();
    canonical[31] |= sign << 7;
    if canonical != *enc {
        return None;
    }
    let one = Fe::ONE;
    let d = curve_d();
    let y2 = y.square();
    let u = y2.sub(&one);
    let v = d.mul(&y2).add(&one);
    let v3 = v.square().mul(&v);
    let v7 = v3.square().mul(&v);
    let mut x = u.mul(&v3).mul(&u.mul(&v7).pow(&EXP_P58));
    let vx2 = v.mul(&x.square());
    if !vx2.eq(&u) {
        if !vx2.eq(&u.neg()) {
            return None;
        }
        let sqrt_m1 = Fe::from_u64(2).pow(&EXP_P14);
        x = x.mul(&sqrt_m1);
    }
    if x.is_zero() && sign == 1 {
        return None;
    }
    if x.is_odd() != sign {
        x = x.neg();
    }
    Some(Point {
        x,
        y,
        z: one,
        t: x.mul(&y),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn identity_enc() -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = 1;
        b
    }

    #[test]
    fn base_point_round_trips() {
        let b = decode(&BASE_ENC).unwrap();
        assert_eq!(b.encode(), BASE_ENC);
    }

    #[test]
    fn base_point_has_order_l() {
        let l = assay::hex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
        let mut lb = [0u8; 32];
        lb.copy_from_slice(&l);
        assert_eq!(Point::base().scalar_mul(&lb).encode(), identity_enc());
    }

    #[test]
    fn add_and_double_agree() {
        let d2 = curve_d2();
        let b = Point::base();
        let two = b.double();
        assert_eq!(two.encode(), b.add(&b, &d2).encode());
        let three = two.add(&b, &d2);
        let mut k = [0u8; 32];
        k[0] = 3;
        assert_eq!(three.encode(), b.scalar_mul(&k).encode());
        assert_eq!(b.add(&Point::identity(), &d2).encode(), BASE_ENC);
    }

    #[test]
    fn decode_rejects_bad_points() {
        // y = p (not canonical)
        let mut p = [0xffu8; 32];
        p[0] = 0xed;
        p[31] = 0x7f;
        assert!(decode(&p).is_none());
        // x = 0 with the sign bit set: y = 1, sign = 1
        let mut neg_zero = identity_enc();
        neg_zero[31] |= 0x80;
        assert!(decode(&neg_zero).is_none());
    }
}
