//! Ed25519 signatures (RFC 8032 section 5.1): pure mode, no context and no prehash.

use crate::edwards::{self, Point};
use crate::sha512::{self, Sha512};
use crate::{scalar, x25519, Error};

/// The clamped secret scalar and the hash prefix, both derived from the seed.
fn expand(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = sha512::digest(seed);
    let mut a = [0u8; 32];
    a.copy_from_slice(&h[..32]);
    let mut prefix = [0u8; 32];
    prefix.copy_from_slice(&h[32..]);
    x25519::clamp(&mut a);
    (a, prefix)
}

/// The challenge scalar k = SHA-512(R || A || M) mod L.
fn challenge(r: &[u8; 32], a: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut h = Sha512::new();
    h.update(r);
    h.update(a);
    h.update(message);
    scalar::reduce_wide(&h.finalize())
}

/// The Ed25519 public key for a 32-byte seed.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    let (a, _) = expand(seed);
    Point::base().scalar_mul(&a).encode()
}

/// Sign `message` with the key derived from `seed`. Returns R || S.
pub fn sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let (a, prefix) = expand(seed);
    let public = Point::base().scalar_mul(&a).encode();

    let mut h = Sha512::new();
    h.update(&prefix);
    h.update(message);
    let r = scalar::reduce_wide(&h.finalize());
    let big_r = Point::base().scalar_mul(&r).encode();

    let k = challenge(&big_r, &public, message);
    // S = (r + k * a) mod L; a is the clamped scalar, below 2^255, so the product fits.
    let s = scalar::mul_add(&k, &a, &r);

    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&big_r);
    sig[32..].copy_from_slice(&s);
    sig
}

/// Verify a signature: checks [S]B = R + [k]A by comparing encodings. Rejects S >= L,
/// points that do not decode, and non-canonical y encodings. Public data only.
pub fn verify(public: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> Result<(), Error> {
    let mut r_enc = [0u8; 32];
    r_enc.copy_from_slice(&signature[..32]);
    let mut s = [0u8; 32];
    s.copy_from_slice(&signature[32..]);
    if !scalar::is_below_l(&s) {
        return Err(Error::BadSignature);
    }
    let a_point = edwards::decode(public).ok_or(Error::BadSignature)?;
    let r_point = edwards::decode(&r_enc).ok_or(Error::BadSignature)?;
    let k = challenge(&r_enc, public, message);
    let d2 = edwards::curve_d2();
    let lhs = Point::base().scalar_mul(&s).encode();
    let rhs = r_point.add(&a_point.scalar_mul(&k), &d2).encode();
    if lhs == rhs {
        Ok(())
    } else {
        Err(Error::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::{hex, to_hex};

    fn arr32(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&hex(s));
        out
    }

    fn sig_hex(sig: &[u8; 64]) -> String {
        to_hex(sig)
    }

    #[test]
    fn rfc8032_test_1() {
        let seed = arr32("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let public = arr32("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        assert_eq!(public_key(&seed), public);
        let sig = sign(&seed, b"");
        assert_eq!(
            sig_hex(&sig),
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );
        assert!(verify(&public, b"", &sig).is_ok());
    }

    #[test]
    fn rfc8032_test_2() {
        let seed = arr32("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb");
        let public = arr32("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c");
        let msg = hex("72");
        assert_eq!(public_key(&seed), public);
        let sig = sign(&seed, &msg);
        assert_eq!(
            sig_hex(&sig),
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"
        );
        assert!(verify(&public, &msg, &sig).is_ok());
    }

    #[test]
    fn rfc8032_test_3() {
        let seed = arr32("c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7");
        let public = arr32("fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025");
        let msg = hex("af82");
        assert_eq!(public_key(&seed), public);
        let sig = sign(&seed, &msg);
        assert_eq!(
            sig_hex(&sig),
            "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a"
        );
        assert!(verify(&public, &msg, &sig).is_ok());
    }

    #[test]
    fn sign_then_verify_random() {
        assay::forall(12, 6, |r| {
            let mut seed = [0u8; 32];
            r.fill(&mut seed);
            let msg = r.bytes(300);
            let public = public_key(&seed);
            let sig = sign(&seed, &msg);
            assert!(verify(&public, &msg, &sig).is_ok());
        });
    }

    #[test]
    fn any_flipped_bit_fails() {
        let seed = [0x42u8; 32];
        let public = public_key(&seed);
        let msg = b"monolith sphinx".to_vec();
        let sig = sign(&seed, &msg);
        for bit in 0..512 {
            let mut bad = sig;
            bad[bit / 8] ^= 1 << (bit % 8);
            assert!(verify(&public, &msg, &bad).is_err(), "sig bit {bit}");
        }
        for bit in 0..(msg.len() * 8) {
            let mut bad = msg.clone();
            bad[bit / 8] ^= 1 << (bit % 8);
            assert!(verify(&public, &bad, &sig).is_err(), "msg bit {bit}");
        }
        for bit in 0..256 {
            let mut bad = public;
            bad[bit / 8] ^= 1 << (bit % 8);
            assert!(verify(&bad, &msg, &sig).is_err(), "key bit {bit}");
        }
    }

    #[test]
    fn s_plus_l_is_rejected() {
        let seed = [0x17u8; 32];
        let public = public_key(&seed);
        let msg = b"malleability";
        let sig = sign(&seed, msg);
        let l = hex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
        let mut s_plus_l = [0u8; 32];
        let mut carry = 0u16;
        for i in 0..32 {
            let t = u16::from(sig[32 + i]) + u16::from(l[i]) + carry;
            s_plus_l[i] = t as u8;
            carry = t >> 8;
        }
        assert_eq!(carry, 0, "S + L fits in 256 bits");
        let mut bad = sig;
        bad[32..].copy_from_slice(&s_plus_l);
        assert_eq!(verify(&public, msg, &bad), Err(Error::BadSignature));
    }

    #[test]
    fn non_canonical_and_invalid_points_are_rejected() {
        let seed = [0x09u8; 32];
        let public = public_key(&seed);
        let msg = b"points";
        let sig = sign(&seed, msg);
        // y = p for the public key
        let mut bad_key = [0xffu8; 32];
        bad_key[0] = 0xed;
        bad_key[31] = 0x7f;
        assert_eq!(verify(&bad_key, msg, &sig), Err(Error::BadSignature));
        // y = p for R
        let mut bad_r = sig;
        bad_r[..32].copy_from_slice(&bad_key);
        assert_eq!(verify(&public, msg, &bad_r), Err(Error::BadSignature));
    }
}
