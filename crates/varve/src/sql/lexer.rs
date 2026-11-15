//! SQL tokenizer tracking exact byte offsets for diagnostics and column naming.

use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub pos: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Ident(String),
    Int(String),
    Text(String),
    Blob(Vec<u8>),
    LParen,
    RParen,
    Comma,
    Semi,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Concat,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

pub(crate) fn tokenize(sql: &str) -> Result<Vec<Token>, Error> {
    let bytes = sql.as_bytes();
    let mut i = 0usize;
    let mut tokens = Vec::new();

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // Line comment -- ...
        if b == b'-' && bytes.get(i + 1) == Some(&b'-') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment /* ... */
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            let mut closed = false;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(parse_err(start, "unterminated block comment"));
            }
            continue;
        }

        let start = i;
        if (b == b'x' || b == b'X') && bytes.get(i + 1) == Some(&b'\'') {
            let (blob, next) = lex_blob(bytes, start)?;
            tokens.push(Token {
                kind: TokenKind::Blob(blob),
                pos: start,
                end: next,
            });
            i = next;
            continue;
        }
        if b == b'\'' {
            let (text, next) = lex_string(sql, start)?;
            tokens.push(Token {
                kind: TokenKind::Text(text),
                pos: start,
                end: next,
            });
            i = next;
            continue;
        }
        if b == b'"' {
            let (ident, next) = lex_quoted_ident(sql, start)?;
            tokens.push(Token {
                kind: TokenKind::Ident(ident),
                pos: start,
                end: next,
            });
            i = next;
            continue;
        }
        if b.is_ascii_digit() {
            let (num, next) = lex_int(sql, start)?;
            tokens.push(Token {
                kind: TokenKind::Int(num),
                pos: start,
                end: next,
            });
            i = next;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident(sql[start..i].to_owned()),
                pos: start,
                end: i,
            });
            continue;
        }

        let (kind, next) = lex_punct(bytes, start)?;
        tokens.push(Token {
            kind,
            pos: start,
            end: next,
        });
        i = next;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        pos: sql.len(),
        end: sql.len(),
    });
    Ok(tokens)
}

fn lex_string(sql: &str, start: usize) -> Result<(String, usize), Error> {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    let mut out = String::new();
    let mut seg_start = i;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            out.push_str(&sql[seg_start..i]);
            if bytes.get(i + 1) == Some(&b'\'') {
                out.push('\'');
                i += 2;
                seg_start = i;
                continue;
            }
            return Ok((out, i + 1));
        }
        i += 1;
    }
    Err(parse_err(start, "unterminated string literal"))
}

fn lex_quoted_ident(sql: &str, start: usize) -> Result<(String, usize), Error> {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    let mut out = String::new();
    let mut seg_start = i;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            out.push_str(&sql[seg_start..i]);
            if bytes.get(i + 1) == Some(&b'"') {
                out.push('"');
                i += 2;
                seg_start = i;
                continue;
            }
            if out.is_empty() {
                return Err(parse_err(start, "empty quoted identifier"));
            }
            return Ok((out, i + 1));
        }
        i += 1;
    }
    Err(parse_err(start, "unterminated quoted identifier"))
}

fn lex_blob(bytes: &[u8], start: usize) -> Result<(Vec<u8>, usize), Error> {
    let mut i = start + 2;
    let hex_start = i;
    while i < bytes.len() && bytes[i] != b'\'' {
        if !bytes[i].is_ascii_hexdigit() {
            return Err(parse_err(i, "invalid hex digit in blob literal"));
        }
        i += 1;
    }
    if i >= bytes.len() {
        return Err(parse_err(start, "unterminated blob literal"));
    }
    let hex = &bytes[hex_start..i];
    if !hex.len().is_multiple_of(2) {
        return Err(parse_err(start, "blob literal must have even hex digits"));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_chunks::<2>().0 {
        let hi = hex_nibble(pair[0]);
        let lo = hex_nibble(pair[1]);
        out.push((hi << 4) | lo);
    }
    Ok((out, i + 1))
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    }
}

fn lex_int(sql: &str, start: usize) -> Result<(String, usize), Error> {
    let bytes = sql.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        return Err(parse_err(i, "floating point literals are not supported"));
    }
    if let Some(&ch) = bytes.get(i) {
        if ch.is_ascii_alphabetic() || ch == b'_' {
            return Err(parse_err(i, "invalid character in integer literal"));
        }
    }
    Ok((sql[start..i].to_owned(), i))
}

fn lex_punct(bytes: &[u8], start: usize) -> Result<(TokenKind, usize), Error> {
    let b = bytes[start];
    let next = bytes.get(start + 1).copied();
    match (b, next) {
        (b'(', _) => Ok((TokenKind::LParen, start + 1)),
        (b')', _) => Ok((TokenKind::RParen, start + 1)),
        (b',', _) => Ok((TokenKind::Comma, start + 1)),
        (b';', _) => Ok((TokenKind::Semi, start + 1)),
        (b'.', _) => Ok((TokenKind::Dot, start + 1)),
        (b'+', _) => Ok((TokenKind::Plus, start + 1)),
        (b'-', _) => Ok((TokenKind::Minus, start + 1)),
        (b'*', _) => Ok((TokenKind::Star, start + 1)),
        (b'/', _) => Ok((TokenKind::Slash, start + 1)),
        (b'%', _) => Ok((TokenKind::Percent, start + 1)),
        (b'|', Some(b'|')) => Ok((TokenKind::Concat, start + 2)),
        (b'=', Some(b'=')) => Ok((TokenKind::EqEq, start + 2)),
        (b'=', _) => Ok((TokenKind::Eq, start + 1)),
        (b'!', Some(b'=')) => Ok((TokenKind::Ne, start + 2)),
        (b'<', Some(b'>')) => Ok((TokenKind::Ne, start + 2)),
        (b'<', Some(b'=')) => Ok((TokenKind::Le, start + 2)),
        (b'<', _) => Ok((TokenKind::Lt, start + 1)),
        (b'>', Some(b'=')) => Ok((TokenKind::Ge, start + 2)),
        (b'>', _) => Ok((TokenKind::Gt, start + 1)),
        _ => Err(parse_err(start, "unexpected character")),
    }
}

pub(crate) fn parse_err(pos: usize, msg: impl Into<String>) -> Error {
    Error::Parse {
        pos,
        msg: msg.into(),
    }
}
