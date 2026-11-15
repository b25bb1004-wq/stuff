//! Poly1305 one-time authenticator (RFC 8439), 26-bit limb arithmetic.
//!
//! Every step is straight-line code with masks, so no branch depends on the key or message.

const M26: u32 = 0x3ff_ffff;

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Poly1305 tag of `msg` under the one-time `key` (r || s).
pub fn mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    let t0 = le32(&key[0..4]);
    let t1 = le32(&key[4..8]);
    let t2 = le32(&key[8..12]);
    let t3 = le32(&key[12..16]);
    let r = [
        t0 & 0x3ff_ffff,
        ((t0 >> 26) | (t1 << 6)) & 0x3ff_ff03,
        ((t1 >> 20) | (t2 << 12)) & 0x3ff_c0ff,
        ((t2 >> 14) | (t3 << 18)) & 0x3f0_3fff,
        (t3 >> 8) & 0x00f_ffff,
    ];
    let s = [r[1] * 5, r[2] * 5, r[3] * 5, r[4] * 5];
    let mut h = [0u32; 5];
    for chunk in msg.chunks(16) {
        let mut block = [0u8; 16];
        let hibit = if chunk.len() == 16 {
            block.copy_from_slice(chunk);
            1 << 24
        } else {
            block[..chunk.len()].copy_from_slice(chunk);
            block[chunk.len()] = 1;
            0
        };
        absorb(&mut h, &r, &s, &block, hibit);
    }
    finish(h, &key[16..32])
}

/// h = (h + block) * r mod 2^130 - 5, with `hibit` set for full blocks only.
fn absorb(h: &mut [u32; 5], r: &[u32; 5], s: &[u32; 4], m: &[u8; 16], hibit: u32) {
    let t0 = le32(&m[0..4]);
    let t1 = le32(&m[4..8]);
    let t2 = le32(&m[8..12]);
    let t3 = le32(&m[12..16]);
    let h0 = (h[0] + (t0 & M26)) as u64;
    let h1 = (h[1] + (((t0 >> 26) | (t1 << 6)) & M26)) as u64;
    let h2 = (h[2] + (((t1 >> 20) | (t2 << 12)) & M26)) as u64;
    let h3 = (h[3] + (((t2 >> 14) | (t3 << 18)) & M26)) as u64;
    let h4 = (h[4] + ((t3 >> 8) | hibit)) as u64;
    let [r0, r1, r2, r3, r4] = r.map(|x| x as u64);
    let [s1, s2, s3, s4] = s.map(|x| x as u64);

    let d0 = h0 * r0 + h1 * s4 + h2 * s3 + h3 * s2 + h4 * s1;
    let d1 = h0 * r1 + h1 * r0 + h2 * s4 + h3 * s3 + h4 * s2;
    let d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * s4 + h4 * s3;
    let d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * s4;
    let d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;

    let m26 = M26 as u64;
    let mut c = d0 >> 26;
    let g0 = d0 & m26;
    let d1 = d1 + c;
    c = d1 >> 26;
    let g1 = d1 & m26;
    let d2 = d2 + c;
    c = d2 >> 26;
    let g2 = d2 & m26;
    let d3 = d3 + c;
    c = d3 >> 26;
    let g3 = d3 & m26;
    let d4 = d4 + c;
    c = d4 >> 26;
    let g4 = d4 & m26;
    let g0 = g0 + c * 5;
    c = g0 >> 26;
    h[0] = (g0 & m26) as u32;
    h[1] = (g1 + c) as u32;
    h[2] = g2 as u32;
    h[3] = g3 as u32;
    h[4] = g4 as u32;
}

/// Final carry, conditional subtraction of p, then add s modulo 2^128.
fn finish(mut h: [u32; 5], pad: &[u8]) -> [u8; 16] {
    let p = [
        le32(&pad[0..4]),
        le32(&pad[4..8]),
        le32(&pad[8..12]),
        le32(&pad[12..16]),
    ];
    let mut c = h[1] >> 26;
    h[1] &= M26;
    h[2] += c;
    c = h[2] >> 26;
    h[2] &= M26;
    h[3] += c;
    c = h[3] >> 26;
    h[3] &= M26;
    h[4] += c;
    c = h[4] >> 26;
    h[4] &= M26;
    h[0] += c * 5;
    c = h[0] >> 26;
    h[0] &= M26;
    h[1] += c;

    // g = h + 5 - 2^130; keep g when h >= p, otherwise keep h.
    let mut g = [0u32; 5];
    g[0] = h[0].wrapping_add(5);
    c = g[0] >> 26;
    g[0] &= M26;
    g[1] = h[1].wrapping_add(c);
    c = g[1] >> 26;
    g[1] &= M26;
    g[2] = h[2].wrapping_add(c);
    c = g[2] >> 26;
    g[2] &= M26;
    g[3] = h[3].wrapping_add(c);
    c = g[3] >> 26;
    g[3] &= M26;
    g[4] = h[4].wrapping_add(c).wrapping_sub(1 << 26);
    let mask = (g[4] >> 31).wrapping_sub(1);
    for (hi, gi) in h.iter_mut().zip(g.iter()) {
        *hi = (*hi & !mask) | (gi & mask);
    }

    let w0 = h[0] | (h[1] << 26);
    let w1 = (h[1] >> 6) | (h[2] << 20);
    let w2 = (h[2] >> 12) | (h[3] << 14);
    let w3 = (h[3] >> 18) | (h[4] << 8);
    let words = [w0, w1, w2, w3];
    let mut out = [0u8; 16];
    let mut carry = 0u64;
    for (i, (w, pw)) in words.iter().zip(p.iter()).enumerate() {
        let sum = *w as u64 + *pw as u64 + carry;
        out[i * 4..i * 4 + 4].copy_from_slice(&(sum as u32).to_le_bytes());
        carry = sum >> 32;
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::{hex, to_hex};

    #[test]
    fn rfc8439_mac() {
        let key: [u8; 32] = hex("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b")
            .try_into()
            .unwrap();
        let tag = mac(&key, b"Cryptographic Forum Research Group");
        assert_eq!(to_hex(&tag), "a8061dc1305136c6c22b8baf0c0127a9");
    }

    #[test]
    fn empty_message_tag_is_s() {
        let mut key = [0u8; 32];
        key[16..].copy_from_slice(&[7u8; 16]);
        assert_eq!(mac(&key, b""), [7u8; 16]);
    }
}
