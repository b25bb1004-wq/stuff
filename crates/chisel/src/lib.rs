//! RV64IMAC assembler producing flat binary machine code matching LLVM 19.
#![forbid(unsafe_code)]

mod asm;
mod directives;
pub mod elf;
mod encode_c;
mod encode_rv;
mod expr;
mod expr_eval;
mod expr_tok;
mod lexer;
mod li;
mod macros;
mod parse_c;
mod parse_rv;
mod parse_rv_ctrl;
mod regs;

/// Assembled machine code and symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Emitted bytes starting at `base`.
    pub bytes: Vec<u8>,
    /// Named labels with their absolute addresses, in definition order.
    pub symbols: Vec<(String, u64)>,
}

/// Assembly error with 1-based source line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmError {
    /// 1-based source line number where the error occurred.
    pub line: usize,
    /// Human-readable description of the error.
    pub msg: String,
}

impl AsmError {
    pub(crate) fn new(line: usize, msg: impl Into<String>) -> Self {
        Self {
            line,
            msg: msg.into(),
        }
    }
}

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for AsmError {}

/// Assemble RV64IMAC source text into a flat byte buffer starting at `base`.
pub fn assemble(src: &str, base: u64) -> Result<Output, AsmError> {
    asm::assemble_impl(src, base)
}
