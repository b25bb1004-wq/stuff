//! CSS Selectors Level 4 parser and matcher.

mod matcher;
mod parser;

pub use matcher::select;
pub use parser::{parse_selectors, Selector, SelectorError};
