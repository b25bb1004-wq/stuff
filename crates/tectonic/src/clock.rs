//! Fixed time step: turns elapsed microseconds into whole simulation steps.
use crate::fx::Fx;

/// Converts elapsed real time into fixed simulation steps, carrying the sub-step remainder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    step_us: u64,
    accumulator: u64,
    max_steps: u32,
}

impl Clock {
    /// Clock with `step_us` microseconds per step (at least 1) and at most `max_steps` steps per call.
    pub fn new(step_us: u64, max_steps: u32) -> Clock {
        Clock {
            step_us: step_us.max(1),
            accumulator: 0,
            max_steps,
        }
    }

    /// Add `elapsed_us` and return the whole steps to run now. Whole steps beyond the cap are dropped; the sub-step remainder is kept.
    pub fn advance(&mut self, elapsed_us: u64) -> u32 {
        self.accumulator = self.accumulator.saturating_add(elapsed_us);
        let whole = self.accumulator / self.step_us;
        self.accumulator %= self.step_us;
        whole.min(u64::from(self.max_steps)) as u32
    }

    /// Leftover time as a fraction of a step, in [0, 1).
    pub fn alpha(&self) -> Fx {
        Fx(((u128::from(self.accumulator) << 16) / u128::from(self.step_us)) as i64)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::forall;

    #[test]
    fn sixty_hz_with_jitter_sums_to_the_right_count() {
        let mut clock = Clock::new(16_667, 8);
        let mut total = 0;
        for i in 0..60u64 {
            let frame = if i % 2 == 0 {
                16_667 - 700
            } else {
                16_667 + 700
            };
            let steps = clock.advance(frame);
            assert!(steps <= 2);
            total += steps;
        }
        assert_eq!(total, 60);
        assert!(clock.alpha() < Fx::ONE);
    }

    #[test]
    fn cap_drops_excess_time() {
        let mut clock = Clock::new(1_000, 5);
        assert_eq!(clock.advance(1_000_000), 5);
        assert_eq!(clock.advance(0), 0);
        assert_eq!(clock.alpha(), Fx::ZERO);
    }

    #[test]
    fn alpha_is_the_leftover_fraction() {
        let mut clock = Clock::new(1_000, 10);
        assert_eq!(clock.advance(1_500), 1);
        assert_eq!(clock.alpha(), Fx::HALF);
        assert_eq!(clock.advance(0), 0);
        assert_eq!(clock.alpha(), Fx::HALF);
    }

    #[test]
    fn zero_step_is_clamped_and_alpha_stays_in_range() {
        let mut zero = Clock::new(0, 3);
        assert_eq!(zero.advance(2), 2);
        forall(2_000, 5, |rng| {
            let mut c = Clock::new(rng.range(1, 100_000), rng.range(1, 10) as u32);
            for _ in 0..5 {
                c.advance(rng.range(0, 300_000));
                let a = c.alpha();
                assert!(a >= Fx::ZERO && a < Fx::ONE);
            }
        });
    }
}
