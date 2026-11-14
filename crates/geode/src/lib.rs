//! geode: the Strata VM's data model, verifier and binary format (MBC1). See docs/MBC.md.
//! The interpreter and the garbage collector are a later task.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod decode;
mod encode;
mod leb;
mod module;
mod op;
mod opcode;
mod value;
mod verify;

#[cfg(test)]
mod testgen;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod verify_tests;

pub use decode::{decode, DecodeError};
pub use encode::encode;
pub use module::{Const, Func, Module};
pub use op::Op;
pub use value::{Handle, Obj, Trap, Value};
pub use verify::{verify, VerifyError};
