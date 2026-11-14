//! HTML5 tokenizer and tree builder.

mod entities;
mod tokenizer;
mod tree;

pub use tokenizer::{tokenize, Token};
pub use tree::parse;
