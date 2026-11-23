//! calliope: piece-table text buffer, multi-cursor undo-tree editor, linear-time regex engine,
//! and modal editor app with Logos syntax highlighting. The terminal editor's pure parts (key
//! decoding, session state, rendering, file encoding) are here too; the console FFI lives in the
//! `calliope` binary.
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

// Host-only: the interactive editor (terminal, screen, keys, session, highlight, Logos syntax).
#[cfg(not(target_os = "none"))]
mod app;
mod buffer;
#[cfg(not(target_os = "none"))]
mod command;
mod cursor;
pub mod file;
#[cfg(not(target_os = "none"))]
mod highlight;
#[cfg(not(target_os = "none"))]
pub mod input;
#[cfg(not(target_os = "none"))]
mod keys;
#[cfg(not(target_os = "none"))]
mod normal;
mod regex;
#[cfg(not(target_os = "none"))]
mod screen;
#[cfg(not(target_os = "none"))]
pub mod session;
#[cfg(not(target_os = "none"))]
pub mod syntax;
mod undo;

#[cfg(not(target_os = "none"))]
pub use app::{App, Mode};
pub use buffer::Buffer;
pub use cursor::Cursor;
#[cfg(not(target_os = "none"))]
pub use highlight::{highlight, Style};
#[cfg(not(target_os = "none"))]
pub use input::{Ev, Mouse};
#[cfg(not(target_os = "none"))]
pub use keys::{decode_keys, parse_keys, Key};
pub use regex::{Regex, RegexError};
#[cfg(not(target_os = "none"))]
pub use screen::{ansi, ansi_changed, Screen};
#[cfg(not(target_os = "none"))]
pub use session::{Prompt, Session};
pub use undo::Editor;
