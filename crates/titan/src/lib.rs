//! Titan: the Rust kernel for the Silicon machine (see docs/TITAN.md).
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod frames;
pub mod sv39;
pub mod trap;
