//! Monotonic time from the kernel tick counter.
use super::calls;

/// Kernel uptime. shortcut: returns kernel ticks as milliseconds; docs/TITAN.md does not give the tick rate, verify it.
pub fn uptime_ms() -> u64 {
    u64::try_from(calls::uptime()).unwrap_or(0)
}
