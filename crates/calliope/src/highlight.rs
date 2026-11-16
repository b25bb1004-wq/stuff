//! Syntax highlighting for Logos source files.

use logos::lexer::{lex, Tok, Token};

/// Visual style of a rendered character cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Default unstyled text.
    Plain,
    /// Language keyword.
    Keyword,
    /// Type or variant identifier.
    Type,
    /// Numeric or byte literal.
    Number,
    /// String literal.
    String,
    /// Line comment.
    Comment,
    /// Lexer error span.
    Error,
    /// Status bar cell.
    Status,
    /// Search match highlight.
    Match,
    /// Selected text in the editor.
    Selection,
}

/// Compute one `Style` per byte of `text` based on the buffer `name`.
pub fn highlight(name: &str, text: &str) -> Vec<Style> {
    let mut styles = vec![Style::Plain; text.len()];
    if !name.ends_with(".logos") || text.is_empty() {
        return styles;
    }

    match lex(text) {
        Ok(tokens) => {
            apply_tokens(text, &tokens, &mut styles);
        }
        Err(diag) => {
            let err_start = (diag.span.start as usize).min(text.len());
            let line_end = text.as_bytes()[err_start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(text.len(), |rel| err_start + rel);
            for s in &mut styles[err_start..line_end] {
                *s = Style::Error;
            }
            if text.is_char_boundary(err_start) {
                if let Ok(tokens) = lex(&text[..err_start]) {
                    apply_tokens(&text[..err_start], &tokens, &mut styles[..err_start]);
                }
            }
        }
    }
    styles
}

fn apply_tokens(src: &str, tokens: &[Token], styles: &mut [Style]) {
    let mut gap_start = 0usize;
    for tok in tokens {
        let start = (tok.span.start as usize).min(src.len());
        let end = (tok.span.end as usize).min(src.len());
        if start > gap_start {
            highlight_comments_in_gap(src, gap_start, start, styles);
        }
        if start < end {
            let style = classify_token(src, start, end, &tok.tok);
            if style != Style::Plain {
                for s in &mut styles[start..end] {
                    *s = style;
                }
            }
            gap_start = gap_start.max(end);
        }
    }
    if gap_start < src.len() {
        highlight_comments_in_gap(src, gap_start, src.len(), styles);
    }
}

fn classify_token(src: &str, start: usize, end: usize, tok: &Tok) -> Style {
    match tok {
        Tok::LIdent(_) | Tok::Newline | Tok::Eof => Style::Plain,
        Tok::UIdent(_) => Style::Type,
        Tok::Int(_) | Tok::Byte(_) => Style::Number,
        Tok::Str(_) => Style::String,
        _ => {
            let slice = src.as_bytes().get(start..end).unwrap_or(&[]);
            if !slice.is_empty() && slice.iter().all(|&b| b.is_ascii_lowercase() || b == b'_') {
                Style::Keyword
            } else {
                Style::Plain
            }
        }
    }
}

fn highlight_comments_in_gap(src: &str, mut pos: usize, limit: usize, styles: &mut [Style]) {
    let bytes = src.as_bytes();
    while pos + 1 < limit {
        if bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            let line_end = bytes[pos..limit]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(limit, |rel| pos + rel);
            for s in &mut styles[pos..line_end] {
                *s = Style::Comment;
            }
            pos = line_end.saturating_add(1);
        } else {
            pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn non_mica_is_plain() {
        let s = highlight("foo.txt", "fn main() {}");
        assert_eq!(s, vec![Style::Plain; 12]);
    }

    #[test]
    fn mica_tokens_and_comments() {
        let src = "fn main(x: Int) -> Str {\n  // hello\n  let y = 42\n  \"a//b\"\n}\n";
        let s = highlight("test.logos", src);
        assert_eq!(s.len(), src.len());
        assert_eq!(s[0..2], [Style::Keyword, Style::Keyword]); // fn
        assert_eq!(s[3..7], [Style::Plain; 4]); // main
        assert_eq!(s[11..14], [Style::Type; 3]); // Int
        let c_start = src.find("// hello").unwrap();
        assert_eq!(&s[c_start..c_start + 8], &[Style::Comment; 8]);
        let num_start = src.find("42").unwrap();
        assert_eq!(&s[num_start..num_start + 2], &[Style::Number; 2]);
        let str_start = src.find("\"a//b\"").unwrap();
        assert_eq!(&s[str_start..str_start + 6], &[Style::String; 6]);
    }

    #[test]
    fn mica_error_highlights_prefix_and_error_line() {
        let src = "fn ok() {\n  let x = \"unterminated\n}\n";
        let s = highlight("bad.logos", src);
        assert_eq!(s.len(), src.len());
        assert_eq!(s[0..2], [Style::Keyword, Style::Keyword]); // fn
        let err_pos = src.find("\"unterminated").unwrap();
        let line_end = src[err_pos..].find('\n').unwrap() + err_pos;
        assert!(s[err_pos..line_end].iter().all(|&st| st == Style::Error));
    }
}
