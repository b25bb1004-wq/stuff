//! assay: the shared test helpers for every Monolith crate.
//!
//! Monolith takes no external crates, so this is our own small property-testing kit:
//! a deterministic random generator, a `forall` runner that prints the failing seed,
//! and hex helpers for published test vectors.
#![forbid(unsafe_code)]

/// Deterministic pseudo-random generator (SplitMix64). Same seed, same stream, on every machine.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Start a stream from `seed`.
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    /// Next 64 random bits.
    pub fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `lo..hi`. Returns `lo` when the range is empty.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.u64() % (hi - lo)
    }

    /// Uniform index in `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        self.range(0, n as u64) as usize
    }

    /// True with probability `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.range(0, den) < num
    }

    /// Fill `buf` with random bytes.
    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let v = self.u64().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }

    /// Random byte vector whose length is in `0..=max_len`.
    pub fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        let mut v = vec![0u8; self.below(max_len + 1)];
        self.fill(&mut v);
        v
    }

    /// Pick one item from a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Run `prop` on `cases` independent random streams derived from `seed`.
///
/// If a case panics, the case seed is printed first, so the failure can be replayed with
/// `forall(1, <that seed>, ..)` after temporarily using the printed seed directly via [`replay`].
pub fn forall(cases: u32, seed: u64, mut prop: impl FnMut(&mut Rng)) {
    let mut master = Rng::new(seed);
    for case in 0..cases {
        let case_seed = master.u64();
        let guard = SeedReport { case, case_seed };
        prop(&mut Rng::new(case_seed));
        std::mem::forget(guard);
    }
}

/// Re-run one failing case from the seed that [`forall`] printed.
pub fn replay(case_seed: u64, mut prop: impl FnMut(&mut Rng)) {
    prop(&mut Rng::new(case_seed));
}

struct SeedReport {
    case: u32,
    case_seed: u64,
}

impl Drop for SeedReport {
    fn drop(&mut self) {
        eprintln!(
            "testkit: property failed on case {} (replay seed {:#018x})",
            self.case, self.case_seed
        );
    }
}

/// Decode a hex string (spaces, newlines and `:` are ignored). Panics on bad input: test-only.
pub fn hex(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b':')
        .map(|b| match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("testkit::hex: bad hex digit {:?}", b as char),
        })
        .collect();
    assert!(
        digits.len().is_multiple_of(2),
        "testkit::hex: odd number of digits"
    );
    digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect()
}

/// Encode bytes as lowercase hex.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..100 {
            assert_eq!(a.u64(), b.u64());
        }
    }

    #[test]
    fn range_stays_in_bounds() {
        forall(200, 1, |r| {
            let lo = r.range(0, 1000);
            let hi = lo + r.range(1, 1000);
            let v = r.range(lo, hi);
            assert!(v >= lo && v < hi);
        });
        assert_eq!(Rng::new(1).range(5, 5), 5);
    }

    #[test]
    fn hex_round_trips() {
        forall(100, 2, |r| {
            let b = r.bytes(64);
            assert_eq!(hex(&to_hex(&b)), b);
        });
        assert_eq!(hex("de ad:BE\nef"), vec![0xde, 0xad, 0xbe, 0xef]);
    }
}
