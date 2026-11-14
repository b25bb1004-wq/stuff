//! slate: piece-table text buffer, multi-cursor undo-tree editor, and linear-time regex engine.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod buffer;
mod cursor;
mod regex;
mod undo;

pub use buffer::Buffer;
pub use cursor::Cursor;
pub use regex::{Regex, RegexError};
pub use undo::Editor;
