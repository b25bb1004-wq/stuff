//! HMAC-SHA-256 (RFC 2104).

use crate::sha256::{self, Sha256};

/// HMAC-SHA-256 tag of `msg` under `key`. Keys longer than the block size are hashed first.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > k.len() {
        k[..32].copy_from_slice(&sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for ((i, o), kb) in ipad.iter_mut().zip(opad.iter_mut()).zip(k.iter()) {
        *i ^= kb;
        *o ^= kb;
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner_digest);
    outer.finalize()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use strata_testkit::to_hex;

    #[test]
    fn rfc4231_cases() {
        let case1 = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            to_hex(&case1),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        let case2 = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            to_hex(&case2),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let case3 = hmac_sha256(&[0xaa; 20], &[0xdd; 50]);
        assert_eq!(
            to_hex(&case3),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
        let case6 = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        assert_eq!(
            to_hex(&case6),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }
}
