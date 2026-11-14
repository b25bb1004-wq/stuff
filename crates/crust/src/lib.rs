//! crust: `no_std` + `alloc` POSIX shell parser, word expander, arithmetic evaluator, and globber.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;

mod arith;
pub mod ast;
mod compound;
#[cfg(feature = "std")]
pub mod exec;
mod expand;
mod glob;
mod heredoc;
mod ifs;
mod lexer;
mod parser;
mod types;
mod word;

pub use crate::arith::arith;
pub use crate::expand::{expand, expand_one};
pub use crate::glob::glob_match;
pub use crate::parser::parse;
pub use crate::types::{Env, ExpandError, ParseError};
