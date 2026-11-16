//! calliope: piece-table text buffer, multi-cursor undo-tree editor, linear-time regex engine,
//! and modal editor app with Logos syntax highlighting. The terminal editor's pure parts (key
//! decoding, session state, rendering, file encoding) are here too; the console FFI lives in the
//! `calliope` binary.
#![forbid(unsafe_code)]

extern crate alloc;

mod app;
mod buffer;
mod command;
mod cursor;
pub mod file;
mod highlight;
pub mod input;
mod keys;
mod normal;
mod regex;
mod screen;
pub mod session;
pub mod syntax;
mod undo;

pub use app::{App, Mode};
pub use buffer::Buffer;
pub use cursor::Cursor;
pub use highlight::{highlight, Style};
pub use input::{Ev, Mouse};
pub use keys::{decode_keys, parse_keys, Key};
pub use regex::{Regex, RegexError};
pub use screen::{ansi, ansi_changed, Screen};
pub use session::{Prompt, Session};
pub use undo::Editor;
