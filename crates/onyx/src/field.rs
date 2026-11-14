//! Arithmetic in GF(2^255 - 19) with five 51-bit limbs. Shared by X25519 and Ed25519.

const MASK: u64 = (1 << 51) - 1;
/// 2p in limbs, added before a subtraction so that no limb goes negative.
const TWO_P0: u64 = (1 << 52) - 38;
const TWO_PN: u64 = (1 << 52) - 2;

/// Exponent p - 2, for inversion by Fermat.
pub(crate) const EXP_INVERT: [u8; 32] = exp(0xeb, 0x7f);
/// Exponent (p - 5) / 8, for square roots in point decoding.
pub(crate) const EXP_P58: [u8; 32] = exp(0xfd, 0x0f);
/// Exponent (p - 1) / 4, which gives a square root of -1 from 2.
pub(crate) const EXP_P14: [u8; 32] = exp(0xfb, 0x1f);

/// Little-endian bytes of `first, 0xff * 30, last`.
const fn exp(first: u8, last: u8) -> [u8; 32] {
    let mut e = [0xffu8; 32];
    e[0] = first;
    e[31] = last;
    e
}

/// A field element. Limbs stay below about 2^52 between operations.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fe(pub(crate) [u64; 5]);

/// Carry chain that leaves every limb below 2^51 except a tiny excess in the low limbs.
fn carry(h: [u64; 5]) -> [u64; 5] {
    let [mut h0, mut h1, mut h2, mut h3, mut h4] = h;
    h1 += h0 >> 51;
    h0 &= MASK;
    h2 += h1 >> 51;
    h1 &= MASK;
    h3 += h2 >> 51;
    h2 &= MASK;
    h4 += h3 >> 51;
    h3 &= MASK;
    h0 += (h4 >> 51) * 19;
    h4 &= MASK;
    h1 += h0 >> 51;
    h0 &= MASK;
    [h0, h1, h2, h3, h4]
}

/// Load up to eight little-endian bytes starting at `at`; bytes past the end count as zero.
fn load8(b: &[u8; 32], at: usize) -> u64 {
    let mut v = 0u64;
    for i in 0..8 {
        if let Some(x) = b.get(at + i) {
            v |= u64::from(*x) << (8 * i);
        }
    }
    v
}

impl Fe {
    /// The element 0.
    pub(crate) const ZERO: Fe = Fe([0; 5]);
    /// The element 1.
    pub(crate) const ONE: Fe = Fe([1, 0, 0, 0, 0]);

    /// The element for a small constant.
    pub(crate) fn from_u64(v: u64) -> Fe {
        Fe([v & MASK, v >> 51, 0, 0, 0])
    }

    /// Decode 32 little-endian bytes. The top bit is ignored; values from 0 to 2^255 - 1 are accepted.
    pub(crate) fn from_bytes(b: &[u8; 32]) -> Fe {
        Fe([
            load8(b, 0) & MASK,
            (load8(b, 6) >> 3) & MASK,
            (load8(b, 12) >> 6) & MASK,
            (load8(b, 19) >> 1) & MASK,
            (load8(b, 24) >> 12) & MASK,
        ])
    }

    /// Canonical 32-byte encoding (the value reduced below p).
    pub(crate) fn to_bytes(self) -> [u8; 32] {
        let mut h = carry(carry(self.0));
        // q = 1 exactly when h >= p, computed by carrying h + 19 through all limbs.
        let mut q = (h[0] + 19) >> 51;
        q = (h[1] + q) >> 51;
        q = (h[2] + q) >> 51;
        q = (h[3] + q) >> 51;
        q = (h[4] + q) >> 51;
        h[0] += 19 * q;
        // Propagate without folding: the carry out of bit 255 is dropped, which subtracts p when q = 1.
        for i in 0..4 {
            h[i + 1] += h[i] >> 51;
            h[i] &= MASK;
        }
        h[4] &= MASK;
        let [h0, h1, h2, h3, h4] = h;
        let words = [
            h0 | (h1 << 51),
            (h1 >> 13) | (h2 << 38),
            (h2 >> 26) | (h3 << 25),
            (h3 >> 39) | (h4 << 12),
        ];
        let mut out = [0u8; 32];
        for (chunk, w) in out.as_chunks_mut::<8>().0.iter_mut().zip(words) {
            chunk.copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    /// Field addition.
    pub(crate) fn add(&self, o: &Fe) -> Fe {
        let a = self.0;
        let b = o.0;
        Fe(carry([
            a[0] + b[0],
            a[1] + b[1],
            a[2] + b[2],
            a[3] + b[3],
            a[4] + b[4],
        ]))
    }

    /// Field subtraction. Operands must have limbs below 2^52 - 38, which every output here satisfies.
    pub(crate) fn sub(&self, o: &Fe) -> Fe {
        let a = self.0;
        let b = o.0;
        Fe(carry([
            (a[0] + TWO_P0).wrapping_sub(b[0]),
            (a[1] + TWO_PN).wrapping_sub(b[1]),
            (a[2] + TWO_PN).wrapping_sub(b[2]),
            (a[3] + TWO_PN).wrapping_sub(b[3]),
            (a[4] + TWO_PN).wrapping_sub(b[4]),
        ]))
    }

    /// Additive inverse.
    pub(crate) fn neg(&self) -> Fe {
        Fe::ZERO.sub(self)
    }

    /// Field multiplication.
    pub(crate) fn mul(&self, o: &Fe) -> Fe {
        let [a0, a1, a2, a3, a4] = self.0.map(u128::from);
        let [b0, b1, b2, b3, b4] = o.0.map(u128::from);
        let (b1x, b2x, b3x, b4x) = (19 * b1, 19 * b2, 19 * b3, 19 * b4);
        let r0 = a0 * b0 + a1 * b4x + a2 * b3x + a3 * b2x + a4 * b1x;
        let r1 = a0 * b1 + a1 * b0 + a2 * b4x + a3 * b3x + a4 * b2x;
        let r2 = a0 * b2 + a1 * b1 + a2 * b0 + a3 * b4x + a4 * b3x;
        let r3 = a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + a4 * b4x;
        let r4 = a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0;
        // Carry in 128 bits first; every limb then fits in 64 bits.
        let c0 = r0 >> 51;
        let r1 = r1 + c0;
        let c1 = r1 >> 51;
        let r2 = r2 + c1;
        let c2 = r2 >> 51;
        let r3 = r3 + c2;
        let c3 = r3 >> 51;
        let r4 = r4 + c3;
        let c4 = r4 >> 51;
        let low = [
            (r0 as u64) & MASK,
            (r1 as u64) & MASK,
            (r2 as u64) & MASK,
            (r3 as u64) & MASK,
            (r4 as u64) & MASK,
        ];
        let top = (c4 as u64) * 19;
        Fe(carry([low[0] + top, low[1], low[2], low[3], low[4]]))
    }

    /// Field squaring.
    pub(crate) fn square(&self) -> Fe {
        self.mul(self)
    }

    /// `self` raised to the little-endian exponent `e`. The sequence of operations does not depend on `self`.
    pub(crate) fn pow(&self, e: &[u8; 32]) -> Fe {
        let mut r = Fe::ONE;
        for byte in e.iter().rev() {
            for bit in (0..8).rev() {
                r = r.square();
                let t = r.mul(self);
                r = Fe::cmov(&r, &t, u64::from((byte >> bit) & 1));
            }
        }
        r
    }

    /// Multiplicative inverse (0 maps to 0).
    pub(crate) fn invert(&self) -> Fe {
        self.pow(&EXP_INVERT)
    }

    /// `self` if `bit` is 0, `b` if `bit` is 1. `bit` must be 0 or 1.
    pub(crate) fn cmov(a: &Fe, b: &Fe, bit: u64) -> Fe {
        let mask = 0u64.wrapping_sub(bit);
        let mut r = a.0;
        for (x, y) in r.iter_mut().zip(b.0) {
            *x ^= (*x ^ y) & mask;
        }
        Fe(r)
    }

    /// Swap `a` and `b` when `swap` is 1; leave them alone when it is 0.
    pub(crate) fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
        let mask = 0u64.wrapping_sub(swap);
        for (x, y) in a.0.iter_mut().zip(b.0.iter_mut()) {
            let t = (*x ^ *y) & mask;
            *x ^= t;
            *y ^= t;
        }
    }

    /// True when the canonical encodings are equal (public data only).
    pub(crate) fn eq(&self, o: &Fe) -> bool {
        self.to_bytes() == o.to_bytes()
    }

    /// True when the element is 0 (public data only).
    pub(crate) fn is_zero(&self) -> bool {
        self.to_bytes() == [0u8; 32]
    }

    /// The low bit of the canonical encoding.
    pub(crate) fn is_odd(&self) -> u8 {
        self.to_bytes()[0] & 1
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::Rng;

    fn p_bytes() -> [u8; 32] {
        exp(0xed, 0x7f)
    }

    fn random_bytes(r: &mut Rng) -> [u8; 32] {
        let mut b = [0u8; 32];
        r.fill(&mut b);
        b[31] &= 0x7f;
        b
    }

    #[test]
    fn inverse_of_random_is_one() {
        assay::forall(100, 1, |r| {
            let a = Fe::from_bytes(&random_bytes(r));
            if a.is_zero() {
                return;
            }
            let one = a.mul(&a.invert());
            assert_eq!(one.to_bytes(), Fe::ONE.to_bytes());
        });
    }

    #[test]
    fn encode_decode_round_trip() {
        assay::forall(100, 2, |r| {
            let canonical = Fe::from_bytes(&random_bytes(r)).to_bytes();
            assert_eq!(Fe::from_bytes(&canonical).to_bytes(), canonical);
        });
    }

    #[test]
    fn values_at_and_above_p_reduce() {
        assert_eq!(Fe::from_bytes(&p_bytes()).to_bytes(), [0u8; 32]);
        let mut pm1 = p_bytes();
        pm1[0] = 0xec;
        assert_eq!(Fe::from_bytes(&pm1).to_bytes(), pm1);
        for k in 1..19u8 {
            let mut above = p_bytes();
            above[0] += k;
            let mut want = [0u8; 32];
            want[0] = k;
            assert_eq!(Fe::from_bytes(&above).to_bytes(), want);
        }
    }

    #[test]
    fn ring_identities() {
        assay::forall(60, 3, |r| {
            let a = Fe::from_bytes(&random_bytes(r));
            let b = Fe::from_bytes(&random_bytes(r));
            let c = Fe::from_bytes(&random_bytes(r));
            assert!(a.mul(&b).mul(&c).eq(&a.mul(&b.mul(&c))));
            assert!(a.mul(&b.add(&c)).eq(&a.mul(&b).add(&a.mul(&c))));
            assert!(a.sub(&a).is_zero());
            assert!(a.add(&a.neg()).is_zero());
            assert!(a.sub(&b).add(&b).eq(&a));
        });
    }

    #[test]
    fn swap_and_select() {
        let a = Fe::from_u64(5);
        let b = Fe::from_u64(7);
        assert!(Fe::cmov(&a, &b, 0).eq(&a));
        assert!(Fe::cmov(&a, &b, 1).eq(&b));
        let (mut x, mut y) = (a, b);
        Fe::cswap(&mut x, &mut y, 1);
        assert!(x.eq(&b) && y.eq(&a));
        Fe::cswap(&mut x, &mut y, 0);
        assert!(x.eq(&b) && y.eq(&a));
    }
}
