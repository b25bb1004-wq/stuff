//! Integers modulo the Ed25519 group order L = 2^252 + 27742317777372353535851937790883648493.

/// L as four little-endian 64-bit limbs.
const L: [u64; 4] = [
    0x5812_631a_5cf5_d3ed,
    0x14de_f9de_a2f7_9cd6,
    0,
    0x1000_0000_0000_0000,
];

fn to_limbs(b: &[u8; 32]) -> [u64; 4] {
    let mut out = [0u64; 4];
    for (limb, chunk) in out.iter_mut().zip(b.as_chunks::<8>().0) {
        *limb = u64::from_le_bytes(*chunk);
    }
    out
}

fn to_bytes(limbs: &[u64; 4]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (chunk, limb) in out.as_chunks_mut::<8>().0.iter_mut().zip(limbs) {
        chunk.copy_from_slice(&limb.to_le_bytes());
    }
    out
}

/// `a - b` over four limbs; returns the difference and 1 if it went negative.
fn sub4(a: &[u64; 4], b: &[u64; 4]) -> ([u64; 4], u64) {
    let mut out = [0u64; 4];
    let mut borrow = 0u64;
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        let (d1, o1) = x.overflowing_sub(*y);
        let (d2, o2) = d1.overflowing_sub(borrow);
        *o = d2;
        borrow = u64::from(o1 | o2);
    }
    (out, borrow)
}

/// True when the little-endian integer `s` is below L. Used on public data (signature S).
pub(crate) fn is_below_l(s: &[u8; 32]) -> bool {
    sub4(&to_limbs(s), &L).1 == 1
}

/// Reduce a 512-bit little-endian integer modulo L.
///
/// Bit by bit from the top: r = 2r + bit, then subtract L once if r >= L. The
/// subtraction is a mask, so the sequence of operations does not depend on the input.
pub(crate) fn reduce_wide(x: &[u8; 64]) -> [u8; 32] {
    let mut r = [0u64; 4];
    for i in (0..512).rev() {
        let mut carry = u64::from((x[i >> 3] >> (i & 7)) & 1);
        for limb in r.iter_mut() {
            let top = *limb >> 63;
            *limb = (*limb << 1) | carry;
            carry = top;
        }
        let (d, borrow) = sub4(&r, &L);
        // borrow == 0 means r >= L, so keep the difference.
        let keep_diff = 0u64.wrapping_sub(1 - borrow);
        for (limb, diff) in r.iter_mut().zip(d) {
            *limb = (diff & keep_diff) | (*limb & !keep_diff);
        }
    }
    to_bytes(&r)
}

/// `(a * b + c) mod L`. Needs `a < L`, `b < 2^256` and `c < 2^256`, so the 512-bit product fits.
pub(crate) fn mul_add(a: &[u8; 32], b: &[u8; 32], c: &[u8; 32]) -> [u8; 32] {
    let al = to_limbs(a);
    let bl = to_limbs(b);
    let cl = to_limbs(c);
    let mut p = [0u64; 8];
    for (i, ai) in al.iter().enumerate() {
        let mut carry = 0u128;
        for (j, bj) in bl.iter().enumerate() {
            let t = u128::from(*ai) * u128::from(*bj) + u128::from(p[i + j]) + carry;
            p[i + j] = t as u64;
            carry = t >> 64;
        }
        p[i + 4] = carry as u64;
    }
    let mut carry = 0u128;
    for (k, slot) in p.iter_mut().enumerate() {
        let add = cl.get(k).copied().unwrap_or(0);
        let t = u128::from(*slot) + u128::from(add) + carry;
        *slot = t as u64;
        carry = t >> 64;
    }
    let mut wide = [0u8; 64];
    for (chunk, limb) in wide.as_chunks_mut::<8>().0.iter_mut().zip(p) {
        chunk.copy_from_slice(&limb.to_le_bytes());
    }
    reduce_wide(&wide)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::Rng;

    fn l_bytes() -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&assay::hex(
            "edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010",
        ));
        out
    }

    fn random_below_l(r: &mut Rng) -> [u8; 32] {
        loop {
            let mut b = [0u8; 32];
            r.fill(&mut b);
            b[31] &= 0x1f;
            if is_below_l(&b) {
                return b;
            }
        }
    }

    #[test]
    fn order_bounds() {
        let mut below = l_bytes();
        below[0] -= 1;
        assert!(is_below_l(&below));
        assert!(!is_below_l(&l_bytes()));
        let mut above = l_bytes();
        above[0] += 1;
        assert!(!is_below_l(&above));
        assert!(is_below_l(&[0u8; 32]));
    }

    #[test]
    fn reduce_wide_fixes_small_and_kills_l() {
        let mut x = [0u8; 64];
        x[..32].copy_from_slice(&l_bytes());
        assert_eq!(reduce_wide(&x), [0u8; 32]);
        // L + 5 reduces to 5
        x[0] = 0xf2;
        let mut five = [0u8; 32];
        five[0] = 5;
        assert_eq!(reduce_wide(&x), five);
    }

    #[test]
    fn mul_add_identities() {
        assay::forall(40, 4, |r| {
            let a = random_below_l(r);
            let b = random_below_l(r);
            let c = random_below_l(r);
            let mut one = [0u8; 32];
            one[0] = 1;
            let zero = [0u8; 32];
            assert_eq!(mul_add(&a, &one, &zero), a);
            assert_eq!(mul_add(&one, &a, &zero), a);
            assert_eq!(mul_add(&a, &b, &zero), mul_add(&b, &a, &zero));
            assert_eq!(mul_add(&a, &zero, &c), c);
            assert_eq!(mul_add(&zero, &a, &zero), zero);
        });
    }
}
