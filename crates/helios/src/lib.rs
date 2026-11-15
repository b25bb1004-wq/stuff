//! helios: fixed-point maths, entity system, fixed time step and world hash.
//! Integer-only: the same inputs give the same bits on every platform.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod audio;
mod clock;
pub mod draw;
pub mod ecs;
pub mod fx;
pub mod phys;
pub mod rng;
pub mod scene;
pub mod script;

pub use clock::Clock;
