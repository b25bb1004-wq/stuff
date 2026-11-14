//! ChaCha20 stream cipher (RFC 8439, 32-bit counter).

/// Keystream block for `counter`, as 64 bytes.
fn block(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> [u8; 64] {
    let mut init = [0u32; 16];
    init[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for (slot, bytes) in init[4..12].iter_mut().zip(key.as_chunks::<4>().0) {
        *slot = le32(bytes);
    }
    init[12] = counter;
    for (slot, bytes) in init[13..16].iter_mut().zip(nonce.as_chunks::<4>().0) {
        *slot = le32(bytes);
    }
    let mut s = init;
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for ((chunk, w), i) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(s.iter())
        .zip(init.iter())
    {
        *chunk = w.wrapping_add(*i).to_le_bytes();
    }
    out
}

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// XOR `data` in place with the keystream starting at block `counter`. Applying it twice is identity.
pub fn xor(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    let mut ctr = counter;
    for chunk in data.chunks_mut(64) {
        let ks = block(key, nonce, ctr);
        for (d, k) in chunk.iter_mut().zip(ks.iter()) {
            *d ^= k;
        }
        ctr = ctr.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::{forall, to_hex};

    fn rfc_key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    #[test]
    fn rfc8439_block() {
        let nonce = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0];
        let ks = block(&rfc_key(), &nonce, 1);
        assert_eq!(
            to_hex(&ks),
            "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4ed2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
        );
    }

    #[test]
    fn rfc8439_encryption() {
        let nonce = [0, 0, 0, 0, 0, 0, 0, 0x4a, 0, 0, 0, 0];
        let plain = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let mut data = plain.to_vec();
        xor(&rfc_key(), &nonce, 1, &mut data);
        assert_eq!(
            to_hex(&data),
            "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0bf91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d807ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab77937365af90bbf74a35be6b40b8eedf2785e42874d"
        );
        xor(&rfc_key(), &nonce, 1, &mut data);
        assert_eq!(&data[..], &plain[..]);
    }

    #[test]
    fn xor_twice_is_identity() {
        forall(100, 21, |r| {
            let mut key = [0u8; 32];
            r.fill(&mut key);
            let mut nonce = [0u8; 12];
            r.fill(&mut nonce);
            let counter = r.u64() as u32;
            let original = r.bytes(300);
            let mut data = original.clone();
            xor(&key, &nonce, counter, &mut data);
            xor(&key, &nonce, counter, &mut data);
            assert_eq!(data, original);
        });
    }
}
