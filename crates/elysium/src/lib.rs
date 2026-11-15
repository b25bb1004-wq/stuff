//! elysium: the Monolith VM's data model, verifier, binary format (MBC1) and interpreter. See docs/MBC.md.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod debug;
mod decode;
mod encode;
mod exec;
mod exec_obj;
mod exec_ops;
mod heap;
mod leb;
mod module;
mod op;
mod opcode;
mod value;
mod verify;
mod vm;

#[cfg(test)]
mod prog_tests;
#[cfg(test)]
mod testgen;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod verify_tests;
#[cfg(test)]
mod vm_tests;

pub use debug::{Event, FrameView, Position, Session};
pub use decode::{decode, DecodeError};
pub use encode::encode;
pub use heap::Heap;
pub use module::{Const, Func, Module};
pub use op::Op;
pub use value::{Handle, Obj, Trap, Value};
pub use verify::{verify, VerifyError};
pub use vm::{Host, Limits, LoadError, NoHost, Vm};
