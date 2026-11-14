//! geyser: deterministic multi-CPU fair scheduler core.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub mod pool;
mod scheduler;
mod types;

pub use scheduler::Scheduler;
pub use types::{Class, Config, Decision, Error, State, Stats, TaskId};

#[cfg(test)]
mod tests;
