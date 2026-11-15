//! SHA-1, Base64, and WebSocket `Sec-WebSocket-Accept` key derivation (RFC 6455).

use alloc::string::String;
use alloc::vec::Vec;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Compute `Sec-WebSocket-Accept` from a client's `Sec-WebSocket-Key` header value.
pub fn accept_key(client_key: &str) -> String {
    let mut input = Vec::with_capacity(client_key.len() + WS_GUID.len());
    input.extend_from_slice(client_key.as_bytes());
    input.extend_from_slice(WS_GUID.as_bytes());
    let digest = sha1(&input);
    base64_encode(&digest)
}

/// Compute the SHA-1 digest of `input`.
pub(crate) fn sha1(input: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    let (blocks, rem) = input.as_chunks::<64>();
    for block in blocks {
        compress(&mut state, block);
    }

    let mut block = [0u8; 64];
    block[..rem.len()].copy_from_slice(rem);
    block[rem.len()] = 0x80;

    let bit_len = (input.len() as u64).wrapping_mul(8);
    if rem.len() < 56 {
        block[56..64].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut state, &block);
    } else {
        compress(&mut state, &block);
        let mut last = [0u8; 64];
        last[56..64].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut state, &last);
    }

    let mut out = [0u8; 20];
    for (i, word) in state.iter().enumerate() {
        let bytes = word.to_be_bytes();
        out[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }
    out
}

fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    let (words, _) = block.as_chunks::<4>();
    for (i, &arr) in words.iter().enumerate() {
        w[i] = u32::from_be_bytes(arr);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let temp = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

/// Encode `data` as standard Base64 with `=` padding.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode standard Base64 with strict padding validation.
#[cfg(feature = "std")]
pub(crate) fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    let num_quads = bytes.len() / 4;
    let (quads, _) = bytes.as_chunks::<4>();
    for (qi, quad) in quads.iter().enumerate() {
        let is_last = qi + 1 == num_quads;
        let v0 = b64_val(quad[0])?;
        let v1 = b64_val(quad[1])?;
        let pad2 = quad[2] == b'=';
        let pad3 = quad[3] == b'=';

        if (pad2 || pad3) && !is_last {
            return None;
        }
        if pad2 && !pad3 {
            return None;
        }

        if pad2 && pad3 {
            if (v1 & 0x0F) != 0 {
                return None;
            }
            out.push((v0 << 2) | (v1 >> 4));
        } else if pad3 {
            let v2 = b64_val(quad[2])?;
            if (v2 & 0x03) != 0 {
                return None;
            }
            out.push((v0 << 2) | (v1 >> 4));
            out.push((v1 << 4) | (v2 >> 2));
        } else {
            let v2 = b64_val(quad[2])?;
            let v3 = b64_val(quad[3])?;
            out.push((v0 << 2) | (v1 >> 4));
            out.push((v1 << 4) | (v2 >> 2));
            out.push((v2 << 6) | v3);
        }
    }
    Some(out)
}

#[cfg(feature = "std")]
fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use assay::to_hex;

    #[test]
    fn rfc6455_accept_key_vector() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn sha1_published_vectors() {
        assert_eq!(
            to_hex(&sha1(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            to_hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            to_hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        let million_a = alloc::vec![b'a'; 1_000_000];
        assert_eq!(
            to_hex(&sha1(&million_a)),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }
}
