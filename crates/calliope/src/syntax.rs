//! Syntax colour for Rust, Markdown and Logos. Rust and Markdown use a line-state tokenizer:
//! each line starts in a state (block-comment depth, open string, code fence), so a viewport
//! that starts mid-file is coloured the same as the whole file would be.

use std::path::Path;

use crate::highlight::{highlight, Style};

/// Language picked from the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// No colour.
    Plain,
    /// Rust source (`.rs`).
    Rust,
    /// Markdown (`.md`, `.markdown`).
    Markdown,
    /// Logos source (`.lg`, `.logos`).
    Logos,
}

impl Lang {
    /// Choose the language from a path's extension.
    pub fn from_path(path: Option<&Path>) -> Lang {
        match path.and_then(|p| p.extension()).and_then(|e| e.to_str()) {
            Some("rs") => Lang::Rust,
            Some("md" | "markdown") => Lang::Markdown,
            Some("lg" | "logos") => Lang::Logos,
            _ => Lang::Plain,
        }
    }
}

/// Tokenizer state at the start of a line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineState {
    /// Nesting depth of `/* */` comments still open.
    pub depth: u32,
    /// A `"` string is still open from an earlier line.
    pub in_string: bool,
    /// A Markdown code fence is open.
    pub fence: bool,
}

/// Colour one line (one `Style` per character) and return the state at the start of the next.
pub fn color_line(lang: Lang, line: &str, st: LineState) -> (Vec<Style>, LineState) {
    match lang {
        Lang::Rust => rust_line(line, st),
        Lang::Markdown => markdown_line(line, st),
        Lang::Logos => (logos_line(line), st),
        Lang::Plain => (vec![Style::Plain; line.chars().count()], st),
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while",
];

fn rust_line(line: &str, mut st: LineState) -> (Vec<Style>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut out = vec![Style::Plain; n];
    let at = |k: usize| chars.get(k).copied().unwrap_or('\0');
    let mut i = 0;
    while i < n {
        if st.depth > 0 {
            if chars[i] == '*' && at(i + 1) == '/' {
                st.depth -= 1;
                out[i] = Style::Comment;
                out[i + 1] = Style::Comment;
                i += 2;
            } else if chars[i] == '/' && at(i + 1) == '*' {
                st.depth += 1;
                out[i] = Style::Comment;
                out[i + 1] = Style::Comment;
                i += 2;
            } else {
                out[i] = Style::Comment;
                i += 1;
            }
            continue;
        }
        if st.in_string {
            out[i] = Style::String;
            if chars[i] == '\\' && i + 1 < n {
                out[i + 1] = Style::String;
                i += 2;
                continue;
            }
            if chars[i] == '"' {
                st.in_string = false;
            }
            i += 1;
            continue;
        }
        let c = chars[i];
        if c == '/' && at(i + 1) == '/' {
            out[i..].fill(Style::Comment);
            break;
        }
        if c == '/' && at(i + 1) == '*' {
            st.depth = 1;
            out[i] = Style::Comment;
            out[i + 1] = Style::Comment;
            i += 2;
        } else if c == '"' {
            st.in_string = true;
            out[i] = Style::String;
            i += 1;
        } else if c == '\'' {
            // Char literal: 'x' or an escape such as '\n' closing within a few characters.
            if at(i + 1) == '\\' {
                if let Some(end) = (i + 2..n.min(i + 12)).find(|&j| chars[j] == '\'') {
                    out[i..=end].fill(Style::String);
                    i = end + 1;
                    continue;
                }
            } else if at(i + 2) == '\'' {
                out[i..=i + 2].fill(Style::String);
                i += 3;
                continue;
            }
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out[start..i].fill(Style::Number);
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let style = if RUST_KEYWORDS.contains(&word.as_str()) {
                Style::Keyword
            } else if c.is_uppercase() {
                Style::Type
            } else {
                Style::Plain
            };
            out[start..i].fill(style);
        } else {
            i += 1;
        }
    }
    (out, st)
}

fn markdown_line(line: &str, mut st: LineState) -> (Vec<Style>, LineState) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        st.fence = !st.fence;
        return (vec![Style::Keyword; n], st);
    }
    if st.fence {
        return (vec![Style::String; n], st);
    }
    if trimmed.starts_with('#') {
        return (vec![Style::Keyword; n], st);
    }
    if trimmed.starts_with('>') {
        return (vec![Style::Comment; n], st);
    }
    let mut out = vec![Style::Plain; n];
    let indent = n - trimmed.chars().count();
    let marker_end = match chars.get(indent) {
        Some('-' | '*' | '+') if chars.get(indent + 1) == Some(&' ') => indent + 1,
        Some(d) if d.is_ascii_digit() => {
            let digits = chars[indent..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            if chars.get(indent + digits) == Some(&'.') {
                indent + digits + 1
            } else {
                indent
            }
        }
        _ => indent,
    };
    out[indent..marker_end].fill(Style::Type);
    let mut in_code = false;
    for (k, &c) in chars.iter().enumerate() {
        if c == '`' {
            in_code = !in_code;
            out[k] = Style::String;
        } else if in_code {
            out[k] = Style::String;
        }
    }
    (out, st)
}

/// Logos colours come from the existing lexer-based highlighter, applied to one line at a time,
/// so a token that spans lines is coloured only on the line it starts on.
/// shortcut: multi-line Logos tokens are approximate, upgrade by threading a lexer state per line.
fn logos_line(line: &str) -> Vec<Style> {
    let per_byte = highlight("line.logos", line);
    line.char_indices()
        .map(|(b, _)| per_byte.get(b).copied().unwrap_or(Style::Plain))
        .collect()
}
