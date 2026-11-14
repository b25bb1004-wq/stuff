//! PCG-XSH-RR 64/32 generator with the reference constants. Same seed gives the same stream everywhere.

/// Permuted congruential generator (PCG-XSH-RR 64/32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Generator for `seed` on `stream`, seeded as the reference `pcg32_srandom_r`.
    pub fn new(seed: u64, stream: u64) -> Pcg32 {
        let mut rng = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    /// Next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform value in `0..n` without modulo bias. Returns 0 when `n` is 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let threshold = n.wrapping_neg() % n;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::forall;

    #[test]
    fn reference_outputs_for_42_54() {
        let mut rng = Pcg32::new(42, 54);
        let got: [u32; 6] = core::array::from_fn(|_| rng.next_u32());
        assert_eq!(
            got,
            [
                0xa15c_02b7,
                0x7b47_f409,
                0xba1d_3330,
                0x83d2_f293,
                0xbfa4_784b,
                0xcbed_606e
            ]
        );
    }

    #[test]
    fn below_is_in_range_and_zero_bound_is_safe() {
        assert_eq!(Pcg32::new(1, 1).below(0), 0);
        forall(200, 3, |rng| {
            let n = rng.range(1, 1000) as u32;
            let mut pcg = Pcg32::new(rng.u64(), rng.u64());
            for _ in 0..50 {
                assert!(pcg.below(n) < n);
            }
        });
    }

    #[test]
    fn below_one_is_always_zero() {
        let mut rng = Pcg32::new(9, 9);
        for _ in 0..100 {
            assert_eq!(rng.below(1), 0);
        }
    }
}
