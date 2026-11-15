//! Expression tokenizer.

use crate::expr::RelocKind;
use crate::AsmError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    Num(i64),
    Ident(String),
    NumLabel { num: u64, forward: bool },
    Reloc(RelocKind),
    LParen,
    RParen,
    Plus,
    Minus,
    Tilde,
    Star,
    Slash,
    Percent,
    Shl,
    Shr,
    Pipe,
    Amp,
    Caret,
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || matches!(b, b'_' | b'.' | b'$')
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$')
}

fn parse_char_lit(bytes: &[u8], i: usize, line: usize) -> Result<(i64, usize), AsmError> {
    let mut pos = i + 1;
    if pos >= bytes.len() {
        return Err(AsmError::new(line, "unterminated character literal"));
    }
    let ch = if bytes[pos] == b'\\' {
        pos += 1;
        if pos >= bytes.len() {
            return Err(AsmError::new(line, "unterminated character literal"));
        }
        let c = match bytes[pos] {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'0' => b'\0',
            b'\\' => b'\\',
            b'\'' => b'\'',
            b'"' => b'"',
            other => {
                return Err(AsmError::new(
                    line,
                    format!("unknown escape '\\{}' in character literal", other as char),
                ));
            }
        };
        pos += 1;
        c
    } else {
        let c = bytes[pos];
        pos += 1;
        c
    };
    if pos >= bytes.len() || bytes[pos] != b'\'' {
        return Err(AsmError::new(line, "unterminated character literal"));
    }
    Ok((i64::from(ch), pos + 1))
}

fn parse_num_or_numlabel(word: &str, line: usize) -> Result<Tok, AsmError> {
    if let Some(hex) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        if hex.is_empty() {
            return Err(AsmError::new(line, "empty hex literal"));
        }
        let v = u64::from_str_radix(hex, 16)
            .map_err(|_| AsmError::new(line, format!("invalid hex literal '{word}'")))?;
        return Ok(Tok::Num(v as i64));
    }
    if let Some(bin) = word.strip_prefix("0b").or_else(|| word.strip_prefix("0B")) {
        if !bin.is_empty() {
            let v = u64::from_str_radix(bin, 2)
                .map_err(|_| AsmError::new(line, format!("invalid binary literal '{word}'")))?;
            return Ok(Tok::Num(v as i64));
        }
        return Ok(Tok::NumLabel {
            num: 0,
            forward: false,
        });
    }
    if let Some(digits) = word.strip_suffix('b') {
        if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
            let num: u64 = digits
                .parse()
                .map_err(|_| AsmError::new(line, format!("invalid numeric label '{word}'")))?;
            return Ok(Tok::NumLabel {
                num,
                forward: false,
            });
        }
    }
    if let Some(digits) = word.strip_suffix('f') {
        if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
            let num: u64 = digits
                .parse()
                .map_err(|_| AsmError::new(line, format!("invalid numeric label '{word}'")))?;
            return Ok(Tok::NumLabel { num, forward: true });
        }
    }
    let v: u64 = word
        .parse()
        .map_err(|_| AsmError::new(line, format!("invalid integer literal '{word}'")))?;
    Ok(Tok::Num(v as i64))
}

/// Tokenize an expression string.
pub(crate) fn tokenize(s: &str, line: usize) -> Result<Vec<Tok>, AsmError> {
    let bytes = s.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match b {
            b'(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            b'+' => {
                toks.push(Tok::Plus);
                i += 1;
            }
            b'-' => {
                toks.push(Tok::Minus);
                i += 1;
            }
            b'~' => {
                toks.push(Tok::Tilde);
                i += 1;
            }
            b'*' => {
                toks.push(Tok::Star);
                i += 1;
            }
            b'/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            b'|' => {
                toks.push(Tok::Pipe);
                i += 1;
            }
            b'&' => {
                toks.push(Tok::Amp);
                i += 1;
            }
            b'^' => {
                toks.push(Tok::Caret);
                i += 1;
            }
            b'<' if i + 1 < bytes.len() && bytes[i + 1] == b'<' => {
                toks.push(Tok::Shl);
                i += 2;
            }
            b'>' if i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                toks.push(Tok::Shr);
                i += 2;
            }
            b'%' => {
                let rest = &s[i..];
                if let Some(kind) = if rest.starts_with("%pcrel_hi")
                    && !rest[9..].bytes().next().is_some_and(is_ident_cont)
                {
                    i += 9;
                    Some(RelocKind::PcrelHi)
                } else if rest.starts_with("%pcrel_lo")
                    && !rest[9..].bytes().next().is_some_and(is_ident_cont)
                {
                    i += 9;
                    Some(RelocKind::PcrelLo)
                } else if rest.starts_with("%hi")
                    && !rest[3..].bytes().next().is_some_and(is_ident_cont)
                {
                    i += 3;
                    Some(RelocKind::Hi)
                } else if rest.starts_with("%lo")
                    && !rest[3..].bytes().next().is_some_and(is_ident_cont)
                {
                    i += 3;
                    Some(RelocKind::Lo)
                } else {
                    None
                } {
                    toks.push(Tok::Reloc(kind));
                } else {
                    toks.push(Tok::Percent);
                    i += 1;
                }
            }
            b'\'' => {
                let (v, next) = parse_char_lit(bytes, i, line)?;
                toks.push(Tok::Num(v));
                i = next;
            }
            b'0'..=b'9' => {
                let start = i;
                while i < bytes.len() && is_ident_cont(bytes[i]) {
                    i += 1;
                }
                toks.push(parse_num_or_numlabel(&s[start..i], line)?);
            }
            _ if is_ident_start(b) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_cont(bytes[i]) {
                    i += 1;
                }
                toks.push(Tok::Ident(s[start..i].to_string()));
            }
            _ => {
                return Err(AsmError::new(
                    line,
                    format!("unexpected character '{}' in expression", b as char),
                ));
            }
        }
    }
    Ok(toks)
}
