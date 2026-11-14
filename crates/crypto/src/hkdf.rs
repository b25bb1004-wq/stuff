//! HKDF with HMAC-SHA-256 (RFC 5869).

use crate::hmac::hmac_sha256;
use crate::Error;
use alloc::vec::Vec;

/// Extract a 32-byte pseudo-random key from `ikm` with `salt`. An empty salt means zeros.
pub fn extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    hmac_sha256(salt, ikm)
}

/// Fill `out` with output keying material derived from `prk` and `info`.
/// Returns `BadLength` when `out` is longer than 255 * 32 bytes.
pub fn expand(prk: &[u8; 32], info: &[u8], out: &mut [u8]) -> Result<(), Error> {
    if out.len() > 255 * 32 {
        return Err(Error::BadLength);
    }
    let mut t = [0u8; 32];
    for (i, chunk) in out.chunks_mut(32).enumerate() {
        let mut msg: Vec<u8> = Vec::with_capacity(32 + info.len() + 1);
        if i > 0 {
            msg.extend_from_slice(&t);
        }
        msg.extend_from_slice(info);
        msg.push((i + 1) as u8);
        t = hmac_sha256(prk, &msg);
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use strata_testkit::{hex, to_hex};

    #[test]
    fn rfc5869_case1() {
        let ikm = [0x0b; 22];
        let salt = hex("000102030405060708090a0b0c");
        let info = hex("f0f1f2f3f4f5f6f7f8f9");
        let prk = extract(&salt, &ikm);
        assert_eq!(
            to_hex(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );
        let mut okm = [0u8; 42];
        expand(&prk, &info, &mut okm).unwrap();
        assert_eq!(
            to_hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn rfc5869_case3() {
        let ikm = [0x0b; 22];
        let prk = extract(&[], &ikm);
        assert_eq!(
            to_hex(&prk),
            "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04"
        );
        let mut okm = [0u8; 42];
        expand(&prk, &[], &mut okm).unwrap();
        assert_eq!(
            to_hex(&okm),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
        );
    }

    #[test]
    fn rejects_too_long_output() {
        let prk = [0u8; 32];
        let mut big = alloc::vec![0u8; 255 * 32 + 1];
        assert_eq!(expand(&prk, b"", &mut big), Err(Error::BadLength));
        let mut max = alloc::vec![0u8; 255 * 32];
        assert_eq!(expand(&prk, b"", &mut max), Ok(()));
    }
}
