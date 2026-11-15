//! Motion presets: the integer [`Spring`] sampled into CSS `linear()` easing curves.

use crate::anim::Spring;
use std::fmt::Write;

/// Spring settings and length of one motion preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Pull towards the target, per mille (see [`Spring::stiffness`]).
    pub stiffness: i64,
    /// Drag on the velocity, per mille (see [`Spring::damping`]).
    pub damping: i64,
    /// Time after which the curve has settled, in milliseconds.
    pub duration_ms: u32,
}

/// Firm: critically damped, reaches its target without overshoot.
pub const FIRM: Preset = Preset {
    stiffness: 625_000,
    damping: 50_000,
    duration_ms: 800,
};

/// Live: lightly underdamped, overshoots its target by 4 to 8 percent.
pub const LIVE: Preset = Preset {
    stiffness: 625_000,
    damping: 33_000,
    duration_ms: 1000,
};

/// Heavy: slow and critically damped, reaches its target without overshoot.
pub const HEAVY: Preset = Preset {
    stiffness: 100_000,
    damping: 20_000,
    duration_ms: 2000,
};

/// Full scale of the simulated position; sampled values are rescaled to per mille.
const SCALE: i64 = 1_000_000;

/// Most stops a string from [`css_linear`] carries.
pub const MAX_STOPS: usize = 32;

/// Simulate `p` from 0 to 1000 in 1 ms steps and return `n` evenly spaced values in per mille.
///
/// The first value is 0 and the last is 1000 (the settled target) when `n >= 2`.
pub fn sample(p: Preset, n: usize) -> Vec<i32> {
    let dur = u64::from(p.duration_ms);
    let mut spring = Spring {
        pos: 0,
        vel: 0,
        target: SCALE,
        stiffness: p.stiffness,
        damping: p.damping,
    };
    let mut track = Vec::with_capacity(usize::try_from(dur).unwrap_or(0) + 1);
    track.push(0);
    for _ in 0..dur {
        spring.step(1);
        track.push(spring.pos);
    }
    let last = u64::try_from(n.saturating_sub(1).max(1)).unwrap_or(1);
    (0..n)
        .map(|k| {
            let k = u64::try_from(k).unwrap_or(0);
            let t = k.saturating_mul(dur) / last;
            let pos = usize::try_from(t).ok().and_then(|i| track.get(i).copied());
            let pos = pos.unwrap_or(SCALE);
            i32::try_from(pos.saturating_mul(1000) / SCALE).unwrap_or(i32::MAX)
        })
        .collect()
}

/// Build a `linear(...)` CSS easing from per-mille `points`, using at most [`MAX_STOPS`] evenly chosen stops.
///
/// Empty input gives `linear(0, 1)`. Stops are spaced evenly by CSS, so the curve keeps its shape.
pub fn css_linear(points: &[i32]) -> String {
    if points.is_empty() {
        return String::from("linear(0, 1)");
    }
    let len = points.len();
    let stops = len.min(MAX_STOPS);
    let mut out = String::from("linear(");
    for k in 0..stops {
        let idx = if stops == 1 {
            0
        } else {
            k.saturating_mul(len - 1) / (stops - 1)
        };
        if k > 0 {
            out.push_str(", ");
        }
        let v = points.get(idx).copied().unwrap_or(0);
        push_unit(&mut out, v);
    }
    out.push(')');
    out
}

/// Append per-mille `v` as a decimal in units with three places (for example `-0.040`).
fn push_unit(out: &mut String, v: i32) {
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    // writing to a String cannot fail
    let _ = write!(out, "{sign}{}.{:03}", a / 1000, a % 1000);
}
