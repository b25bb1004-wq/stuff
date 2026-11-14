//! tectonic: fixed-point maths, entity system, fixed time step and world hash.
//! Integer-only: the same inputs give the same bits on every platform.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod clock;
pub mod ecs;
pub mod fx;
pub mod rng;

pub use clock::Clock;
