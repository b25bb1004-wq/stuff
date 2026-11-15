//! The Monolith site: one section per system, each with a live demo (T75).

#![cfg_attr(not(target_arch = "wasm32"), forbid(unsafe_code))]

pub mod demos;

/// ABI number of the wasm exports. Bump it when the set of `#[no_mangle]` exports changes;
/// the kit reloads once when the page and the wasm disagree.
pub const SITE_ABI: u32 = 1;

#[cfg(not(target_arch = "wasm32"))]
mod icons;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack;
#[cfg(target_arch = "wasm32")]
pub mod wasm;
