//! Byte spans, diagnostics and line/column lookup.
use alloc::string::String;

/// A half-open byte range `start..end` in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte offset.
    pub start: u32,
    /// One past the last byte offset.
    pub end: u32,
}

/// An error: where it is and what went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// Source range the error points at.
    pub span: Span,
    /// Human-readable message.
    pub message: String,
}

/// 1-based line and column (counted in characters) of a byte offset.
pub fn line_col(src: &str, offset: u32) -> (u32, u32) {
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    for (i, c) in src.char_indices() {
        if i >= offset as usize {
            break;
        }
        if c == '\n' {
            line = line.saturating_add(1);
            col = 1;
        } else {
            col = col.saturating_add(1);
        }
    }
    (line, col)
}

/// Span from byte offsets, saturating at `u32::MAX`.
pub(crate) fn sp(start: usize, end: usize) -> Span {
    Span {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    }
}

/// Diagnostic at a byte range.
pub(crate) fn err(start: usize, end: usize, message: impl Into<String>) -> Diag {
    Diag {
        span: sp(start, end),
        message: message.into(),
    }
}
