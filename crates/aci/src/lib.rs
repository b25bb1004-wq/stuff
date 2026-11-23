//! aci: the agent interface every Monolith tool speaks (docs/ai-computer/ACI.md).
//!
//! Standard library only on the host, `core` and `alloc` on titan: a JSON value with a reader and
//! writer, the manifest types, the envelope helpers, and the validators that the `aci` binary exposes.
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod envelope;
pub mod json;
pub mod manifest;
