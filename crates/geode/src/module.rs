//! The module: constants, imports, functions and the entry point.

use crate::op::Op;
use alloc::{string::String, vec::Vec};

/// Magic bytes that start every encoded module.
pub(crate) const MAGIC: [u8; 4] = *b"MBC1";
/// Limit on functions, constants and imports (each).
pub(crate) const MAX_TABLE: usize = 65_536;
/// Limit on instructions per function.
pub(crate) const MAX_CODE: usize = 1_048_576;
/// Limit on a function's frame size in registers.
pub(crate) const MAX_REGS: u16 = 4096;
/// Limit on a function or import name, in bytes.
pub(crate) const MAX_NAME: usize = 255;

/// A verified-or-not bytecode module.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// The constant pool.
    pub consts: Vec<Const>,
    /// Host function names the module calls.
    pub imports: Vec<String>,
    /// The functions.
    pub funcs: Vec<Func>,
    /// Index of the function `run` calls.
    pub entry: u32,
}

/// A constant pool entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    /// A string constant.
    Str(String),
    /// A byte-string constant.
    Bytes(Vec<u8>),
}

/// A function.
#[derive(Debug, Clone, PartialEq)]
pub struct Func {
    /// The function name.
    pub name: String,
    /// Number of parameters; they arrive in registers `0..params`.
    pub params: u16,
    /// Frame size in registers.
    pub regs: u16,
    /// The instructions.
    pub code: Vec<Op>,
}
