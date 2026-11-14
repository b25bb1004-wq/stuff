//! X25519 Diffie-Hellman on Curve25519 (RFC 7748).

use crate::field::Fe;
use crate::Error;

/// RFC 7748 clamping: clear the three low bits, clear bit 255, set bit 254.
pub(crate) fn clamp(k: &mut [u8; 32]) {
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
}

/// Montgomery ladder over the clamped scalar `k` and u-coordinate `u`. Fixed sequence of
/// field operations; secret bits only drive masked swaps.
fn ladder(k: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    let a24 = Fe::from_u64(121_665);
    let x1 = Fe::from_bytes(u);
    let mut x2 = Fe::ONE;
    let mut z2 = Fe::ZERO;
    let mut x3 = x1;
    let mut z3 = Fe::ONE;
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let kt = u64::from((k[t >> 3] >> (t & 7)) & 1);
        swap ^= kt;
        Fe::cswap(&mut x2, &mut x3, swap);
        Fe::cswap(&mut z2, &mut z3, swap);
        swap = kt;

        let a = x2.add(&z2);
        let aa = a.square();
        let b = x2.sub(&z2);
        let bb = b.square();
        let e = aa.sub(&bb);
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        x3 = da.add(&cb).square();
        z3 = x1.mul(&da.sub(&cb).square());
        x2 = aa.mul(&bb);
        z2 = e.mul(&aa.add(&e.mul(&a24)));
    }
    Fe::cswap(&mut x2, &mut x3, swap);
    Fe::cswap(&mut z2, &mut z3, swap);
    x2.mul(&z2.invert()).to_bytes()
}

/// The X25519 public key for a 32-byte secret (the u-coordinate of `[k]9`).
pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
    let mut k = *secret;
    clamp(&mut k);
    let mut base = [0u8; 32];
    base[0] = 9;
    ladder(&k, &base)
}

/// The shared secret for `secret` and the peer's public key. Fails with `WeakKey` when the
/// result is all zero (the peer sent a low-order point).
pub fn shared_secret(secret: &[u8; 32], peer_public: &[u8; 32]) -> Result<[u8; 32], Error> {
    let mut k = *secret;
    clamp(&mut k);
    let out = ladder(&k, peer_public);
    let acc = out.iter().fold(0u8, |acc, b| acc | b);
    if acc == 0 {
        return Err(Error::WeakKey);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::{hex, to_hex};

    /// The RFC 7748 function: clamp the scalar, then run the ladder.
    fn x(k: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
        let mut c = *k;
        clamp(&mut c);
        ladder(&c, u)
    }

    fn arr(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&hex(s));
        out
    }

    #[test]
    fn rfc7748_section_5_2_vectors() {
        let k1 = arr("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let u1 = arr("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        assert_eq!(
            to_hex(&x(&k1, &u1)),
            "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"
        );
        let k2 = arr("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d");
        let u2 = arr("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493");
        assert_eq!(
            to_hex(&x(&k2, &u2)),
            "95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957"
        );
    }

    #[test]
    fn rfc7748_iterated() {
        let mut k = [0u8; 32];
        k[0] = 9;
        let mut u = k;
        let mut after_one = String::new();
        for i in 1..=1000 {
            let next = x(&k, &u);
            u = k;
            k = next;
            if i == 1 {
                after_one = to_hex(&k);
            }
        }
        assert_eq!(
            after_one,
            "422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079"
        );
        assert_eq!(
            to_hex(&k),
            "684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51"
        );
    }

    #[test]
    fn rfc7748_diffie_hellman() {
        let alice = arr("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob = arr("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
        let alice_pub = public_key(&alice);
        let bob_pub = public_key(&bob);
        assert_eq!(
            to_hex(&alice_pub),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
        assert_eq!(
            to_hex(&bob_pub),
            "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
        );
        let want = "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742";
        assert_eq!(to_hex(&shared_secret(&alice, &bob_pub).unwrap()), want);
        assert_eq!(to_hex(&shared_secret(&bob, &alice_pub).unwrap()), want);
    }

    #[test]
    fn shared_secret_is_symmetric() {
        assay::forall(20, 5, |r| {
            let mut a = [0u8; 32];
            let mut b = [0u8; 32];
            r.fill(&mut a);
            r.fill(&mut b);
            let ab = shared_secret(&a, &public_key(&b)).unwrap();
            let ba = shared_secret(&b, &public_key(&a)).unwrap();
            assert_eq!(ab, ba);
        });
    }

    #[test]
    fn all_zero_peer_key_is_weak() {
        let secret = [7u8; 32];
        assert_eq!(shared_secret(&secret, &[0u8; 32]), Err(Error::WeakKey));
    }
}
