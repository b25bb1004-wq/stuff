//! geyser: deterministic multi-CPU fair scheduler core.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod scheduler;
mod types;

pub use scheduler::Scheduler;
pub use types::{Class, Config, Decision, Error, State, Stats, TaskId};

#[cfg(test)]
mod tests;
