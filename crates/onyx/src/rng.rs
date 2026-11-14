//! ChaCha20 keystream as a deterministic random generator.

use crate::{chacha20, hkdf};

const ZERO_NONCE: [u8; 12] = [0; 12];

/// ChaCha20 keystream used as a random generator: key = seed, nonce = 0, counter from 0.
pub struct ChaChaRng {
    key: [u8; 32],
    counter: u32,
    buf: [u8; 64],
    pos: usize,
}

impl ChaChaRng {
    /// A generator whose output is the ChaCha20 keystream for `seed`.
    pub fn from_seed(seed: [u8; 32]) -> ChaChaRng {
        ChaChaRng {
            key: seed,
            counter: 0,
            buf: [0; 64],
            pos: 64,
        }
    }

    /// Load the next 64-byte keystream block into the buffer.
    fn refill(&mut self) {
        self.buf = [0; 64];
        chacha20::xor(&self.key, &ZERO_NONCE, self.counter, &mut self.buf);
        // shortcut: the 32-bit block counter repeats after 256 GiB of output, reseed before that
        self.counter = self.counter.wrapping_add(1);
        self.pos = 0;
    }

    /// Fill `out` with the next keystream bytes, in order.
    pub fn fill(&mut self, out: &mut [u8]) {
        for byte in out.iter_mut() {
            if self.pos == 64 {
                self.refill();
            }
            if let Some(&b) = self.buf.get(self.pos) {
                *byte = b;
            }
            self.pos += 1;
        }
    }

    /// The next 8 keystream bytes as a little-endian `u64`.
    pub fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill(&mut b);
        u64::from_le_bytes(b)
    }

    /// A uniform value in `0..n` by rejection sampling. Returns 0 when `n` is 0.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        // 2^64 mod n: draws below this would make some residues more likely than others.
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next_u64();
            if x >= threshold {
                return x % n;
            }
        }
    }

    /// Mix `extra` into the key: key = HKDF-Extract(salt = old key, ikm = extra). Restarts the stream.
    pub fn reseed(&mut self, extra: &[u8]) {
        self.key = hkdf::extract(&self.key, extra);
        self.counter = 0;
        self.pos = 64;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::{forall, to_hex};

    /// RFC 8439 appendix A.1 test vectors #1 and #2: zero key, zero nonce, counters 0 and 1.
    const A1_BLOCKS: &str = concat!(
        "76b8e0ada0f13d90405d6ae55386bd28bdd219b8a08ded1aa836efcc8b770dc7da41597c5157488d7724e03fb8d84a37",
        "6a43b8f41518a11cc387b669b2ee6586",
        "9f07e7be5551387a98ba977c732d080dcb0f29a048e3656912c6533e32ee7aed29b721769ce64e43d57133b074d839d531",
        "ed1f28510afb45ace10a1f4b794d6f"
    );

    #[test]
    fn rfc8439_a1_keystream() {
        let mut rng = ChaChaRng::from_seed([0; 32]);
        let mut out = [0u8; 128];
        rng.fill(&mut out);
        assert_eq!(to_hex(&out), A1_BLOCKS);
        let mut derived = [0u8; 128];
        chacha20::xor(&[0; 32], &[0; 12], 0, &mut derived);
        assert_eq!(out, derived);
    }

    #[test]
    fn split_reads_match() {
        let seed = [0x42u8; 32];
        let mut whole = [0u8; 128];
        ChaChaRng::from_seed(seed).fill(&mut whole);
        let mut bytes = ChaChaRng::from_seed(seed);
        let mut single = [0u8; 128];
        for b in single.iter_mut() {
            bytes.fill(core::slice::from_mut(b));
        }
        assert_eq!(single, whole);
        let mut words = ChaChaRng::from_seed(seed);
        let mut from_u64 = [0u8; 128];
        for chunk in from_u64.chunks_mut(8) {
            chunk.copy_from_slice(&words.next_u64().to_le_bytes());
        }
        assert_eq!(from_u64, whole);
    }

    #[test]
    fn below_stays_in_range() {
        for n in [1u64, 2, 3, 7, 1000, (1u64 << 63) + 1] {
            let mut rng = ChaChaRng::from_seed([n as u8; 32]);
            for _ in 0..10_000 {
                assert!(rng.below(n) < n);
            }
        }
        assert_eq!(ChaChaRng::from_seed([0; 32]).below(0), 0);
    }

    #[test]
    fn below_is_roughly_uniform() {
        let mut rng = ChaChaRng::from_seed([7; 32]);
        let mut counts = [0u64; 6];
        for _ in 0..60_000 {
            counts[rng.below(6) as usize] += 1;
        }
        // Expected 10 000 per bucket; chi-square with 5 degrees of freedom, loose bound.
        let chi2: f64 = counts
            .iter()
            .map(|&o| {
                let d = o as f64 - 10_000.0;
                d * d / 10_000.0
            })
            .sum();
        assert!(chi2 < 30.0, "chi-square {chi2}");
    }

    #[test]
    fn reseed_changes_and_repeats() {
        let mut a = ChaChaRng::from_seed([1; 32]);
        let mut b = ChaChaRng::from_seed([1; 32]);
        let mut c = ChaChaRng::from_seed([1; 32]);
        a.reseed(b"extra");
        b.reseed(b"extra");
        c.reseed(b"other");
        let (x, y, z) = (a.next_u64(), b.next_u64(), c.next_u64());
        assert_eq!(x, y);
        assert_ne!(x, z);
        assert_ne!(x, ChaChaRng::from_seed([1; 32]).next_u64());
    }

    #[test]
    fn fill_splits_are_independent_of_length() {
        forall(100, 61, |r| {
            let mut seed = [0u8; 32];
            r.fill(&mut seed);
            let n = r.below(300);
            let mut whole = vec![0u8; n];
            ChaChaRng::from_seed(seed).fill(&mut whole);
            let mut rng = ChaChaRng::from_seed(seed);
            let mut parts = vec![0u8; n];
            let cut = r.below(n + 1);
            let (head, tail) = parts.split_at_mut(cut);
            rng.fill(head);
            rng.fill(tail);
            assert_eq!(parts, whole);
        });
    }
}
