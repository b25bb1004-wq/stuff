//! aci: the agent interface every Monolith tool speaks (docs/ai-computer/ACI.md).
//!
//! Standard library only: a JSON value with a reader and writer, the manifest types,
//! the envelope helpers, and the validators that the `aci` binary exposes.
#![forbid(unsafe_code)]

pub mod envelope;
pub mod json;
pub mod manifest;
