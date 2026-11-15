//! Integer easing, tweening and spring motion. No floats: all values are fixed-point.

/// Easing curve used by [`ease`] and [`tween`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ease {
    /// Constant speed.
    Linear,
    /// Accelerating quadratic.
    InQuad,
    /// Decelerating quadratic.
    OutQuad,
    /// Accelerating then decelerating quadratic.
    InOutQuad,
    /// Decelerating cubic.
    OutCubic,
}

/// Apply curve `e` at `t` (clamped to 0..=1000); the result is in 0..=1000.
pub fn ease(e: Ease, t: u32) -> u32 {
    let t = u64::from(t.min(1000));
    let u = 1000 - t;
    let r = match e {
        Ease::Linear => t,
        Ease::InQuad => t * t / 1000,
        Ease::OutQuad => 1000 - u * u / 1000,
        Ease::InOutQuad => {
            if t < 500 {
                2 * t * t / 1000
            } else {
                1000 - 2 * u * u / 1000
            }
        }
        Ease::OutCubic => 1000 - u * u * u / 1_000_000,
    };
    u32::try_from(r).unwrap_or(1000)
}

/// Value at time `now` of a tween from `from` to `to` over `dur` ms starting at `start`.
pub fn tween(from: i32, to: i32, start: u64, dur: u64, now: u64, e: Ease) -> i32 {
    if now <= start {
        return from;
    }
    if dur == 0 || now >= start.saturating_add(dur) {
        return to;
    }
    let t = u128::from(now - start) * 1000 / u128::from(dur);
    let p = i64::from(ease(e, u32::try_from(t).unwrap_or(1000)));
    let d = i64::from(to) - i64::from(from);
    let v = i64::from(from) + d * p / 1000;
    i32::try_from(v).unwrap_or(to)
}

/// Return `q`, or one unit in the direction of `src` when `q` truncated to zero but `src` is non-zero.
fn nudge(q: i64, src: i64) -> i64 {
    if q == 0 && src != 0 {
        src.signum()
    } else {
        q
    }
}

/// Distance and speed (units) under which a stepped spring snaps to rest at its target.
const SNAP: u64 = 16;

/// Damped spring with integer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spring {
    /// Current position.
    pub pos: i64,
    /// Current velocity.
    pub vel: i64,
    /// Position the spring is pulled towards.
    pub target: i64,
    /// Pull towards the target, per mille.
    pub stiffness: i64,
    /// Drag on the velocity, per mille.
    pub damping: i64,
}

impl Spring {
    /// Advance by `dt_ms` milliseconds (semi-implicit Euler, saturating on overflow).
    pub fn step(&mut self, dt_ms: u32) {
        let dt = i64::from(dt_ms);
        if dt == 0 {
            return;
        }
        // shortcut: a non-zero term that truncates to 0 moves by one unit, else low stiffness stalls short
        let pull_src = self
            .stiffness
            .saturating_mul(self.target.saturating_sub(self.pos));
        let pull = nudge(pull_src / 1000, pull_src);
        let drag = self.damping.saturating_mul(self.vel) / 1000;
        let f = pull.saturating_sub(drag);
        let dv_src = f.saturating_mul(dt);
        self.vel = self.vel.saturating_add(nudge(dv_src / 1000, dv_src));
        let dp_src = self.vel.saturating_mul(dt);
        self.pos = self.pos.saturating_add(nudge(dp_src / 1000, dp_src));
        // the one-unit nudges can orbit the target; inside this band snap to rest so settled() turns true
        if self.target.saturating_sub(self.pos).unsigned_abs() <= SNAP
            && self.vel.unsigned_abs() <= SNAP
        {
            self.pos = self.target;
            self.vel = 0;
        }
    }

    /// True when the spring is within 1 unit of its target and nearly at rest.
    pub fn settled(&self) -> bool {
        self.target.saturating_sub(self.pos).unsigned_abs() <= 1 && self.vel.unsigned_abs() <= 1
    }
}
