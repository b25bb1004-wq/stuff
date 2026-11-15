//! Load tools and throughput numbers for mercury, iridium, xenon and aurum (T78).
#![forbid(unsafe_code)]

pub mod aurum;
pub mod iridium;
pub mod mercury;
pub mod xenon;

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// Throughput measurement for a single benchmark.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Suite name (for example `"mercury"`).
    pub suite: &'static str,
    /// Benchmark name within the suite.
    pub name: String,
    /// Display unit for the rate column (for example `"req/s"` or `"MB/s"`).
    pub unit: &'static str,
    /// Total completed operations (or megabytes for `"MB/s"`).
    pub ops: u64,
    /// Total elapsed wall-clock seconds.
    pub secs: f64,
}

impl Sample {
    /// Compute throughput in operations per second, returning `0.0` when `secs` is zero or non-finite.
    pub fn rate(&self) -> f64 {
        if self.secs <= 0.0 || !self.secs.is_finite() {
            0.0
        } else {
            self.ops as f64 / self.secs
        }
    }
}

/// Configuration controlling benchmark duration and dataset scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opts {
    /// Minimum wall-clock time to run each benchmark.
    pub budget: Duration,
    /// Dataset size multiplier (`1` for quick, `10` for full).
    pub scale: u32,
}

impl Default for Opts {
    /// Default quick options: 200 ms budget, scale 1.
    fn default() -> Self {
        Self {
            budget: Duration::from_millis(200),
            scale: 1,
        }
    }
}

/// Call `f` at least once until `budget` has elapsed, summing returned ops and timing with `Instant`.
pub fn measure(
    suite: &'static str,
    name: impl Into<String>,
    unit: &'static str,
    budget: Duration,
    mut f: impl FnMut() -> Result<u64, String>,
) -> Result<Sample, String> {
    let start = Instant::now();
    let mut ops = 0u64;
    loop {
        let count = f()?;
        ops = ops.saturating_add(count);
        if start.elapsed() >= budget {
            break;
        }
    }
    let secs = start.elapsed().as_secs_f64();
    Ok(Sample {
        suite,
        name: name.into(),
        unit,
        ops,
        secs,
    })
}

/// Format `samples` as a Markdown table `| suite | benchmark | rate | unit |`.
pub fn table(samples: &[Sample]) -> String {
    let mut out = String::from("| suite | benchmark | rate | unit |\n|---|---|---|---|\n");
    for s in samples {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            s.suite,
            s.name,
            format_rate(s.rate()),
            s.unit
        );
    }
    out
}

fn format_rate(r: f64) -> String {
    if !r.is_finite() || r <= 0.0 {
        return "0".to_string();
    }
    if r >= 999_500.0 {
        return format!("{}M", format_sig3(r / 1_000_000.0));
    }
    if r >= 999.5 {
        return format!("{}k", format_sig3(r / 1_000.0));
    }
    format_sig3(r)
}

fn format_sig3(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        let rounded = (v * 10.0).round() / 10.0;
        if rounded >= 100.0 {
            format!("{rounded:.0}")
        } else {
            format!("{rounded:.1}")
        }
    } else if v >= 1.0 {
        let rounded = (v * 100.0).round() / 100.0;
        if rounded >= 10.0 {
            format!("{rounded:.1}")
        } else {
            format!("{rounded:.2}")
        }
    } else {
        let exp = v.log10().floor() as i32;
        let decimals = (2 - exp).clamp(0, 12) as usize;
        let factor = 10f64.powi(decimals as i32);
        let rounded = (v * factor).round() / factor;
        if rounded >= 1.0 {
            format!("{rounded:.2}")
        } else {
            format!("{rounded:.decimals$}")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use assay::forall;

    #[test]
    fn rate_formatting_cases() {
        assert_eq!(format_rate(0.0), "0");
        assert_eq!(format_rate(-1.0), "0");
        assert_eq!(format_rate(f64::NAN), "0");
        assert_eq!(format_rate(12_340.0), "12.3k");
        assert_eq!(format_rate(4_561_000.0), "4.56M");
        assert_eq!(format_rate(999.5), "1.00k");
        assert_eq!(format_rate(99.96), "100");
        assert_eq!(format_rate(0.1234), "0.123");
    }

    #[test]
    fn sample_rate_zero_and_positive() {
        forall(50, 77, |r| {
            let ops = r.range(0, 1_000_000);
            let zero = Sample {
                suite: "s",
                name: "n".to_string(),
                unit: "ops/s",
                ops,
                secs: 0.0,
            };
            assert_eq!(zero.rate(), 0.0);
            let pos = Sample { secs: 2.0, ..zero };
            assert!((pos.rate() - ops as f64 / 2.0).abs() < 1e-9);
        });
    }
}
