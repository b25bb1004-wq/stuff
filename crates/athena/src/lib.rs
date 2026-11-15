//! Athena: browser IDE shell for Logos and Titan (T72).

#![cfg_attr(not(target_arch = "wasm32"), forbid(unsafe_code))]

pub mod debug;
#[cfg(not(target_arch = "wasm32"))]
mod icons;
pub mod lang;
mod lang_fmt;
mod lang_walk;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack;
pub mod service;
#[cfg(target_arch = "wasm32")]
pub mod wasm;
