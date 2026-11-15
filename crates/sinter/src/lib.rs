//! sinter: Mica IR to Geode bytecode compiler and runner.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod compile;
mod meta;
mod run;
pub mod rv64;

#[cfg(test)]
mod prog_tests;
#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec::Vec;

pub use compile::compile;
pub use run::{run_main, run_prop, run_test};

/// A Mica program compiled to a Geode bytecode module with source spans and test metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// The verified Geode bytecode module.
    pub module: geode::Module,
    /// Geode function index of `main`, if defined.
    pub main: Option<u32>,
    /// Unit tests `(name, func_index)` in the same order as `ir::Program::tests`.
    pub tests: Vec<(String, u32)>,
    /// Property tests in the same order as `ir::Program::props`.
    pub props: Vec<CompiledProp>,
    /// Instruction source spans: `spans[func][op]` is the source span of that op.
    pub spans: Vec<Vec<mica::Span>>,
}

/// A compiled property test entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledProp {
    /// Property description.
    pub name: String,
    /// Optional Geode function index of the `where` guard.
    pub guard: Option<u32>,
    /// Geode function index of the property body.
    pub body: u32,
    /// Parameter types of the property.
    pub params: Vec<mica::ir::Ty>,
}
