//! The Monolith site: one section per system, each with a live demo (T75).

#![cfg_attr(not(target_arch = "wasm32"), forbid(unsafe_code))]

pub mod demos;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack;
#[cfg(target_arch = "wasm32")]
pub mod wasm;
