//! EXPERIMENTAL: Medusa, Monolith's educational 128-bit block cipher (no outside cryptanalysis).

use core::fmt;

/// Number of rounds in Medusa.
pub const ROUNDS: usize = 24;

/// 256-bit key, 128-bit block experimental ARX block cipher.
pub struct Medusa {
    rk: [[u32; 4]; ROUNDS + 1],
}

impl fmt::Debug for Medusa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Medusa").finish_non_exhaustive()
    }
}

impl Medusa {
    /// Expand a 256-bit key into `ROUNDS + 1` 128-bit round keys.
    pub fn new(key: &[u8; 32]) -> Medusa {
        let mut k = [0u32; 8];
        for (slot, chunk) in k.iter_mut().zip(key.as_chunks::<4>().0) {
            *slot = u32::from_le_bytes(*chunk);
        }

        let mut rk = [[0u32; 4]; ROUNDS + 1];
        for (r, round_key) in rk.iter_mut().enumerate() {
            for (i, slot) in round_key.iter_mut().enumerate() {
                let c = 0x9E37_79B9u32.wrapping_mul((4 * r + i + 1) as u32);
                *slot = k[i] ^ c;
            }
            qr(&mut k[0..4]);
            qr(&mut k[4..8]);
            k.rotate_left(4);
            k[0] ^= k[4];
        }

        Medusa { rk }
    }

    /// Encrypt a 16-byte block in place.
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        let mut s = load_block(block);
        xor_words(&mut s, &self.rk[0]);
        for round_key in &self.rk[1..] {
            qr(&mut s);
            s.rotate_left(1);
            xor_words(&mut s, round_key);
        }
        store_block(block, &s);
    }

    /// Decrypt a 16-byte block in place.
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut s = load_block(block);
        for round_key in self.rk[1..].iter().rev() {
            xor_words(&mut s, round_key);
            s.rotate_right(1);
            qr_inv(&mut s);
        }
        xor_words(&mut s, &self.rk[0]);
        store_block(block, &s);
    }
}

/// Counter mode as in the doc: xors the keystream starting at block `counter` into `data`.
pub fn ctr_xor(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    let cipher = Medusa::new(key);
    let mut ctr = counter;
    for chunk in data.chunks_mut(16) {
        let mut block = [0u8; 16];
        let (n_part, c_part) = block.split_at_mut(12);
        n_part.copy_from_slice(nonce);
        c_part.copy_from_slice(&ctr.to_le_bytes());
        cipher.encrypt_block(&mut block);
        for (d, k) in chunk.iter_mut().zip(block.iter()) {
            *d ^= *k;
        }
        ctr = ctr.wrapping_add(1);
    }
}

fn load_block(block: &[u8; 16]) -> [u32; 4] {
    let mut s = [0u32; 4];
    for (slot, chunk) in s.iter_mut().zip(block.as_chunks::<4>().0) {
        *slot = u32::from_le_bytes(*chunk);
    }
    s
}

fn store_block(block: &mut [u8; 16], s: &[u32; 4]) {
    for (chunk, word) in block.as_chunks_mut::<4>().0.iter_mut().zip(s.iter()) {
        *chunk = word.to_le_bytes();
    }
}

fn xor_words(s: &mut [u32; 4], rk: &[u32; 4]) {
    for (a, b) in s.iter_mut().zip(rk.iter()) {
        *a ^= *b;
    }
}

fn qr(s: &mut [u32]) {
    let [a, b, c, d] = s else { return };
    *a = a.wrapping_add(*b);
    *d = (*d ^ *a).rotate_left(16);
    *c = c.wrapping_add(*d);
    *b = (*b ^ *c).rotate_left(12);
    *a = a.wrapping_add(*b);
    *d = (*d ^ *a).rotate_left(8);
    *c = c.wrapping_add(*d);
    *b = (*b ^ *c).rotate_left(7);
}

fn qr_inv(s: &mut [u32; 4]) {
    let [a, b, c, d] = s;
    *b = b.rotate_right(7) ^ *c;
    *c = c.wrapping_sub(*d);
    *d = d.rotate_right(8) ^ *a;
    *a = a.wrapping_sub(*b);
    *b = b.rotate_right(12) ^ *c;
    *c = c.wrapping_sub(*d);
    *d = d.rotate_right(16) ^ *a;
    *a = a.wrapping_sub(*b);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::forall;

    #[test]
    fn debug_hides_keys() {
        let c = Medusa::new(&[0u8; 32]);
        let s = format!("{c:?}");
        assert_eq!(s, "Medusa { .. }");
    }

    #[test]
    fn qr_round_trip() {
        forall(200, 11, |r| {
            let orig = [
                r.u64() as u32,
                r.u64() as u32,
                r.u64() as u32,
                r.u64() as u32,
            ];
            let mut s = orig;
            qr(&mut s);
            qr_inv(&mut s);
            assert_eq!(s, orig);
        });
    }

    #[test]
    fn block_round_trip() {
        forall(200, 12, |r| {
            let mut key = [0u8; 32];
            r.fill(&mut key);
            let mut block = [0u8; 16];
            r.fill(&mut block);
            let orig = block;
            let c = Medusa::new(&key);
            c.encrypt_block(&mut block);
            assert_ne!(block, orig);
            c.decrypt_block(&mut block);
            assert_eq!(block, orig);
        });
    }

    #[test]
    fn ctr_round_trip_and_split() {
        forall(100, 13, |r| {
            let mut key = [0u8; 32];
            r.fill(&mut key);
            let mut nonce = [0u8; 12];
            r.fill(&mut nonce);
            let counter = r.u64() as u32;
            let plain = r.bytes(200);
            let mut ct = plain.clone();
            ctr_xor(&key, &nonce, counter, &mut ct);

            // Round trip
            let mut pt = ct.clone();
            ctr_xor(&key, &nonce, counter, &mut pt);
            assert_eq!(pt, plain);

            // Split at a block boundary
            if plain.len() >= 16 {
                let split_blocks = r.below(plain.len() / 16 + 1);
                let split = split_blocks * 16;
                let mut ct_split = plain.clone();
                ctr_xor(&key, &nonce, counter, &mut ct_split[..split]);
                ctr_xor(
                    &key,
                    &nonce,
                    counter.wrapping_add(split_blocks as u32),
                    &mut ct_split[split..],
                );
                assert_eq!(ct_split, ct);
            }
        });
    }
}
