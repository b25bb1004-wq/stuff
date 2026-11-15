//! slate: piece-table text buffer, multi-cursor undo-tree editor, linear-time regex engine,
//! and modal editor app with Mica syntax highlighting.
#![forbid(unsafe_code)]

extern crate alloc;

mod app;
mod buffer;
mod command;
mod cursor;
mod highlight;
mod keys;
mod normal;
mod regex;
mod screen;
mod undo;

pub use app::{App, Mode};
pub use buffer::Buffer;
pub use cursor::Cursor;
pub use highlight::{highlight, Style};
pub use keys::{decode_keys, parse_keys, Key};
pub use regex::{Regex, RegexError};
pub use screen::{ansi, Screen};
pub use undo::Editor;
