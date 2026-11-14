//! ChaCha20-Poly1305 AEAD (RFC 8439 section 2.8).

use crate::{chacha20, ct, poly1305, Error};
use alloc::vec::Vec;

/// Poly1305 input: AAD and ciphertext, each zero-padded to 16 bytes, then both lengths as u64 LE.
fn mac_input(aad: &[u8], ct: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(aad.len() + ct.len() + 48);
    m.extend_from_slice(aad);
    m.resize(m.len() + (16 - aad.len() % 16) % 16, 0);
    m.extend_from_slice(ct);
    m.resize(m.len() + (16 - ct.len() % 16) % 16, 0);
    m.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    m.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    m
}

fn one_time_key(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
    let mut k = [0u8; 32];
    chacha20::xor(key, nonce, 0, &mut k);
    k
}

/// Encrypt and authenticate. Returns `ciphertext || 16-byte tag`.
pub fn seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(plaintext.len() + 16);
    out.extend_from_slice(plaintext);
    chacha20::xor(key, nonce, 1, &mut out);
    let tag = poly1305::mac(&one_time_key(key, nonce), &mac_input(aad, &out));
    out.extend_from_slice(&tag);
    out
}

/// Verify and decrypt `ciphertext || tag`. `BadLength` if shorter than 16 bytes, `BadTag` on mismatch.
pub fn open(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, Error> {
    let split = sealed.len().checked_sub(16).ok_or(Error::BadLength)?;
    let (ct, tag) = sealed.split_at(split);
    let expected = poly1305::mac(&one_time_key(key, nonce), &mac_input(aad, ct));
    if !ct::eq(&expected, tag) {
        return Err(Error::BadTag);
    }
    let mut pt = ct.to_vec();
    chacha20::xor(key, nonce, 1, &mut pt);
    Ok(pt)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use strata_testkit::{forall, to_hex};

    const PLAIN: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

    const AAD: &[u8] = &[
        0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    ];

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = 0x80 + i as u8;
        }
        k
    }

    fn nonce() -> [u8; 12] {
        [7, 0, 0, 0, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47]
    }

    #[test]
    fn rfc8439_aead_vector() {
        let sealed = seal(&key(), &nonce(), AAD, PLAIN);
        let want = concat!(
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116",
            "1ae10b594f09e26a7e902ecbd0600691"
        );
        assert_eq!(sealed.len(), PLAIN.len() + 16);
        assert_eq!(to_hex(&sealed), want);
        assert_eq!(open(&key(), &nonce(), AAD, &sealed).unwrap(), PLAIN);
    }

    #[test]
    fn rejects_short_input() {
        assert_eq!(
            open(&key(), &nonce(), b"", &[0u8; 15]),
            Err(Error::BadLength)
        );
    }

    #[test]
    fn round_trip_and_bit_flips() {
        forall(200, 31, |r| {
            let mut k = [0u8; 32];
            r.fill(&mut k);
            let mut n = [0u8; 12];
            r.fill(&mut n);
            let aad = r.bytes(300);
            let msg = r.bytes(300);
            let sealed = seal(&k, &n, &aad, &msg);
            assert_eq!(open(&k, &n, &aad, &sealed).unwrap(), msg);
            let bit = r.below(sealed.len() * 8);
            let mut bad = sealed.clone();
            bad[bit / 8] ^= 1 << (bit % 8);
            assert_eq!(open(&k, &n, &aad, &bad), Err(Error::BadTag));
            if !aad.is_empty() {
                let mut other = aad.clone();
                let abit = r.below(other.len() * 8);
                other[abit / 8] ^= 1 << (abit % 8);
                assert_eq!(open(&k, &n, &other, &sealed), Err(Error::BadTag));
            }
        });
    }
}
