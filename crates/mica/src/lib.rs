//! Mica front end: lexer, syntax tree, recursive-descent parser, canonical printer,
//! type and layer checker, and typed intermediate representation.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod span;

pub mod ast;
pub mod check;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod print;

pub use span::{line_col, Diag, Span};

#[cfg(test)]
pub(crate) use testkit::no_span;

#[cfg(test)]
mod testkit {
    use alloc::string::String;
    use core::fmt::Debug;

    /// Debug text of a tree with every span removed, for comparing trees structurally.
    pub fn no_span<T: Debug>(value: &T) -> String {
        let text = alloc::format!("{:?}", value);
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(i) = rest.find("Span { start:") {
            out.push_str(&rest[..i]);
            out.push_str("Span");
            match rest[i..].find('}') {
                Some(j) => rest = &rest[i + j + 1..],
                None => rest = "",
            }
        }
        out.push_str(rest);
        out
    }
}
