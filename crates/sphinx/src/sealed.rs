//! Sealed box: encrypt to an X25519 public key (ephemeral DH, HKDF-SHA-256, ChaCha20-Poly1305).

use crate::{aead, hkdf, x25519, Error};
use alloc::vec::Vec;

/// Bytes added to a message by `seal`: the ephemeral public key and the Poly1305 tag.
pub const OVERHEAD: usize = 32 + 16;

const INFO: &[u8] = b"monolith sealed v1";
const ZERO_NONCE: [u8; 12] = [0; 12];

/// AEAD key from the shared secret; `ctx` is `eph_pub || recipient`, used as HKDF salt.
fn box_key(ctx: &[u8], shared: &[u8; 32]) -> Result<[u8; 32], Error> {
    let mut key = [0u8; 32];
    hkdf::expand(&hkdf::extract(ctx, shared), INFO, &mut key)?;
    Ok(key)
}

/// Encrypt `msg` to `recipient`. `eph_secret` is a fresh random 32-byte value from the caller.
/// Returns an empty vector when `recipient` has low order (no key can be derived); `open` rejects it.
pub fn seal(recipient: &[u8; 32], eph_secret: &[u8; 32], msg: &[u8]) -> Vec<u8> {
    let eph_pub = x25519::public_key(eph_secret);
    let ctx = [eph_pub.as_slice(), recipient.as_slice()].concat();
    let key = match x25519::shared_secret(eph_secret, recipient).and_then(|s| box_key(&ctx, &s)) {
        Ok(key) => key,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::with_capacity(OVERHEAD + msg.len());
    out.extend_from_slice(&eph_pub);
    out.extend_from_slice(&aead::seal(&key, &ZERO_NONCE, &ctx, msg));
    out
}

/// Decrypt a box made by `seal` for the public key of `recipient_secret`. Any tampering, a wrong
/// recipient, a low-order ephemeral key or input shorter than `OVERHEAD` is an error.
pub fn open(recipient_secret: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, Error> {
    if sealed.len() < OVERHEAD {
        return Err(Error::BadLength);
    }
    let (eph_pub, body) = sealed.split_first_chunk::<32>().ok_or(Error::BadLength)?;
    let recipient = x25519::public_key(recipient_secret);
    let ctx = [eph_pub.as_slice(), recipient.as_slice()].concat();
    let shared = x25519::shared_secret(recipient_secret, eph_pub)?;
    let key = box_key(&ctx, &shared)?;
    aead::open(&key, &ZERO_NONCE, &ctx, body)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::forall;

    /// Low-order u-coordinates from RFC 7748 section 6.1 (order 1 and order 8).
    const LOW_ORDER: [&str; 3] = [
        "0000000000000000000000000000000000000000000000000000000000000000",
        "e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800",
        "5f9c95bca3508c24b1d0b1559c83ef5b04445cc4581c8e86d8224eddd09f1157",
    ];

    fn low(i: usize) -> [u8; 32] {
        let v = assay::hex(LOW_ORDER[i]);
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        out
    }

    #[test]
    fn round_trip_random_sizes() {
        forall(300, 41, |r| {
            let mut eph = [0u8; 32];
            r.fill(&mut eph);
            let mut secret = [0u8; 32];
            r.fill(&mut secret);
            let public = x25519::public_key(&secret);
            let mut msg = vec![0u8; r.below(4097)];
            r.fill(&mut msg);
            let boxed = seal(&public, &eph, &msg);
            assert_eq!(boxed.len(), msg.len() + OVERHEAD);
            assert_eq!(open(&secret, &boxed).unwrap(), msg);
        });
    }

    #[test]
    fn any_flipped_byte_fails() {
        let secret = [9u8; 32];
        let public = x25519::public_key(&secret);
        let msg = [0x33u8; 50];
        let boxed = seal(&public, &[5u8; 32], &msg);
        assert_eq!(open(&secret, &boxed).unwrap(), msg);
        for i in 0..boxed.len() {
            let mut bad = boxed.clone();
            bad[i] ^= 0xff;
            assert!(open(&secret, &bad).is_err(), "byte {i} not caught");
        }
    }

    #[test]
    fn wrong_recipient_fails() {
        let public = x25519::public_key(&[9u8; 32]);
        let boxed = seal(&public, &[5u8; 32], b"hello");
        assert_eq!(open(&[10u8; 32], &boxed), Err(Error::BadTag));
    }

    #[test]
    fn short_input_fails() {
        assert_eq!(
            open(&[1u8; 32], &[0u8; OVERHEAD - 1]),
            Err(Error::BadLength)
        );
        assert_eq!(open(&[1u8; 32], &[]), Err(Error::BadLength));
    }

    #[test]
    fn low_order_points_are_rejected() {
        for i in 0..LOW_ORDER.len() {
            let point = low(i);
            assert_eq!(
                x25519::shared_secret(&[3u8; 32], &point),
                Err(Error::WeakKey),
                "vector {i} is not low order"
            );
            // Recipient of low order: seal yields nothing that open accepts.
            let boxed = seal(&point, &[3u8; 32], b"msg");
            assert!(open(&[4u8; 32], &boxed).is_err());
            // Ephemeral key of low order: open rejects it.
            let mut forged = point.to_vec();
            forged.extend_from_slice(&[0u8; 60]);
            assert_eq!(open(&[4u8; 32], &forged), Err(Error::WeakKey));
        }
    }
}
