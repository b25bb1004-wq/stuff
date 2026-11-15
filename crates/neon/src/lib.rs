//! neon: WebAssembly builds of the stack with our own glue (built by T71).

#![cfg_attr(not(target_arch = "wasm32"), forbid(unsafe_code))]

#[cfg(not(target_arch = "wasm32"))]
pub mod pack;
pub mod playground;
pub mod session;
#[cfg(target_arch = "wasm32")]
pub mod wasm;
