//! Shell word, parameter, arithmetic, and backquote parsing.

use alloc::string::String;
use alloc::vec::Vec;

use crate::ast::{Param, ParamOp, Part, Script, Word};
use crate::lexer::Scanner;
use crate::types::{Escapes, ParseError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TildeMode {
    None,
    Start,
    Assign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopMode {
    Word,
    Brace,
    EndOfInput,
}

pub(crate) fn push_lit(parts: &mut Vec<Part>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Part::Lit(last)) = parts.last_mut() {
        last.push_str(s);
    } else {
        parts.push(Part::Lit(String::from(s)));
    }
}

pub(crate) fn push_lit_char(parts: &mut Vec<Part>, ch: char) {
    if let Some(Part::Lit(last)) = parts.last_mut() {
        last.push(ch);
    } else {
        let mut s = String::new();
        s.push(ch);
        parts.push(Part::Lit(s));
    }
}

pub(crate) fn is_word_delim(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t' | '\n' | '&' | '|' | ';' | '(' | ')' | '<' | '>'
    )
}

fn try_scan_tilde(scanner: &mut Scanner<'_>, mode: TildeMode, stop: StopMode) -> Option<Part> {
    if mode == TildeMode::None {
        return None;
    }
    if scanner.cursor.peek() != Some('~') {
        return None;
    }
    let cp = scanner.cursor.clone();
    scanner.cursor.next_char();
    let mut user = String::new();
    while let Some(ch) = scanner.cursor.peek() {
        if ch == '/' || is_word_delim(ch) {
            break;
        }
        if mode == TildeMode::Assign && ch == ':' {
            break;
        }
        if stop == StopMode::Brace && ch == '}' {
            break;
        }
        if matches!(ch, '\'' | '"' | '\\' | '$' | '`') {
            scanner.cursor = cp;
            return None;
        }
        user.push(ch);
        scanner.cursor.next_char();
    }
    Some(Part::Tilde(user))
}

fn parse_single_quote(scanner: &mut Scanner<'_>) -> Result<Part, ParseError> {
    let start_err = scanner.cursor.err("unterminated single quote", true);
    scanner.cursor.next_char(); // consume opening '\''
    let mut s = String::new();
    loop {
        match scanner.cursor.next_raw() {
            None => return Err(start_err),
            Some('\'') => return Ok(Part::Single(s)),
            Some(c) => s.push(c),
        }
    }
}

/// Read one hex or octal escape payload of up to `max` digits, starting from `val`.
fn take_digits(scanner: &mut Scanner<'_>, radix: u32, max: usize, mut val: u32) -> (u32, usize) {
    let mut count = 0;
    while count < max {
        let Some(d) = scanner.cursor.peek_raw().and_then(|ch| ch.to_digit(radix)) else {
            break;
        };
        scanner.cursor.next_raw();
        val = val.saturating_mul(radix).saturating_add(d);
        count += 1;
    }
    (val, count)
}

/// Parse `$'...'` (ANSI-C quoting). The `$` is at the cursor; the quote follows it.
fn parse_ansi_c(scanner: &mut Scanner<'_>) -> Result<Part, ParseError> {
    let start_err = scanner.cursor.err("unterminated $' quote", true);
    scanner.cursor.next_char(); // consume '$'
    scanner.cursor.next_char(); // consume opening '\''
    let mut s = String::new();
    loop {
        match scanner.cursor.next_raw() {
            None => return Err(start_err),
            Some('\'') => return Ok(Part::Single(s)),
            Some('\\') => {
                let Some(c) = scanner.cursor.next_raw() else {
                    return Err(start_err);
                };
                let decoded = match c {
                    'a' => '\u{07}',
                    'b' => '\u{08}',
                    'e' | 'E' => '\u{1b}',
                    'f' => '\u{0c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'v' => '\u{0b}',
                    '\\' | '\'' | '"' | '?' => c,
                    '0'..='7' => {
                        let (v, _) = take_digits(scanner, 8, 2, c.to_digit(8).unwrap_or(0));
                        char::from_u32(v).unwrap_or('\u{FFFD}')
                    }
                    // shortcut: \xHH and \NNN above 0x7f become the code point, not a raw byte,
                    // because words are UTF-8 strings; upgrade when words can carry bytes
                    'x' => {
                        let (v, n) = take_digits(scanner, 16, 2, 0);
                        if n == 0 {
                            s.push_str("\\x");
                            continue;
                        }
                        char::from_u32(v).unwrap_or('\u{FFFD}')
                    }
                    'u' => {
                        let (v, n) = take_digits(scanner, 16, 4, 0);
                        if n == 0 {
                            s.push_str("\\u");
                            continue;
                        }
                        char::from_u32(v).unwrap_or('\u{FFFD}')
                    }
                    other => {
                        s.push('\\');
                        other
                    }
                };
                s.push(decoded);
            }
            Some(c) => s.push(c),
        }
    }
}

fn parse_double_quote(scanner: &mut Scanner<'_>) -> Result<Part, ParseError> {
    let start_err = scanner.cursor.err("unterminated double quote", true);
    scanner.cursor.next_char(); // consume opening '"'
    let mut parts = Vec::new();
    loop {
        match scanner.cursor.peek() {
            None => return Err(start_err),
            Some('"') => {
                scanner.cursor.next_char();
                return Ok(Part::Double(parts));
            }
            Some('\\') => {
                scanner.cursor.next_char();
                match scanner.cursor.next_raw() {
                    None => return Err(start_err),
                    Some(c) if matches!(c, '$' | '`' | '"' | '\\') => {
                        parts.push(Part::Escaped(c));
                    }
                    Some(c) => {
                        push_lit_char(&mut parts, '\\');
                        push_lit_char(&mut parts, c);
                    }
                }
            }
            Some('$') => {
                let p = parse_dollar(scanner, true)?;
                match p {
                    Part::Lit(s) => push_lit(&mut parts, &s),
                    other => parts.push(other),
                }
            }
            Some('`') => {
                let script = parse_backquote(scanner, true)?;
                parts.push(Part::Cmd(script));
            }
            Some(c) => {
                scanner.cursor.next_char();
                push_lit_char(&mut parts, c);
            }
        }
    }
}

fn parse_backquote(scanner: &mut Scanner<'_>, in_double: bool) -> Result<Script, ParseError> {
    let start_err = scanner.cursor.err("unterminated backquote", true);
    scanner.cursor.next_char(); // consume opening '`'
    let mut inner = String::new();
    loop {
        match scanner.cursor.peek() {
            None => return Err(start_err),
            Some('`') => {
                scanner.cursor.next_char();
                break;
            }
            Some('\\') => {
                scanner.cursor.next_char();
                match scanner.cursor.next_raw() {
                    None => return Err(start_err),
                    Some(c) if c == '$' || c == '`' || c == '\\' || (in_double && c == '"') => {
                        inner.push(c);
                    }
                    Some(c) => {
                        inner.push('\\');
                        inner.push(c);
                    }
                }
            }
            Some(c) => {
                scanner.cursor.next_char();
                inner.push(c);
            }
        }
    }
    crate::parser::parse_nested(scanner.depth + 1, &inner, scanner.esc)
}

fn skip_arith_quoted(
    scanner: &mut Scanner<'_>,
    raw: &mut String,
    quote: char,
) -> Result<(), ParseError> {
    let err = scanner.cursor.err("unterminated quote in arithmetic", true);
    raw.push(quote);
    scanner.cursor.next_char();
    while let Some(c) = if quote == '\'' {
        scanner.cursor.next_raw()
    } else {
        scanner.cursor.next_char()
    } {
        raw.push(c);
        if c == quote {
            return Ok(());
        }
        if quote == '"' && c == '\\' {
            if let Some(esc) = scanner.cursor.next_raw() {
                raw.push(esc);
            } else {
                return Err(err);
            }
        }
    }
    Err(err)
}

fn try_parse_arith(scanner: &mut Scanner<'_>) -> Result<Option<String>, ParseError> {
    // Cursor is right after `$`, positioned at `((`.
    let cp = scanner.cursor.clone();
    let start_err = scanner
        .cursor
        .err("unterminated arithmetic expansion", true);
    scanner.cursor.next_char(); // '('
    scanner.cursor.next_char(); // '('

    let mut raw = String::new();
    let mut paren_depth: usize = 0;

    loop {
        match scanner.cursor.peek() {
            None => return Err(start_err),
            Some('\'') => skip_arith_quoted(scanner, &mut raw, '\'')?,
            Some('"') => skip_arith_quoted(scanner, &mut raw, '"')?,
            Some('\\') => {
                scanner.cursor.next_char();
                let Some(c) = scanner.cursor.next_raw() else {
                    return Err(start_err);
                };
                raw.push('\\');
                raw.push(c);
            }
            Some('(') => {
                scanner.cursor.next_char();
                raw.push('(');
                paren_depth += 1;
            }
            Some(')') => {
                if paren_depth == 0 {
                    if scanner.cursor.peek2() == Some(')') {
                        scanner.cursor.next_char();
                        scanner.cursor.next_char();
                        return Ok(Some(raw));
                    }
                    // Not `$((...))`; rewind to parse as `$( ( ... ) )`.
                    scanner.cursor = cp;
                    return Ok(None);
                }
                scanner.cursor.next_char();
                raw.push(')');
                paren_depth -= 1;
            }
            Some(c) => {
                scanner.cursor.next_char();
                raw.push(c);
            }
        }
    }
}

fn read_brace_param_name(scanner: &mut Scanner<'_>) -> Result<String, ParseError> {
    let Some(first) = scanner.cursor.peek() else {
        return Err(scanner.cursor.err("unterminated ${...}", true));
    };
    if first.is_ascii_alphabetic() || first == '_' {
        let mut name = String::new();
        name.push(first);
        scanner.cursor.next_char();
        while let Some(c) = scanner.cursor.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                name.push(c);
                scanner.cursor.next_char();
            } else {
                break;
            }
        }
        return Ok(name);
    }
    if first.is_ascii_digit() {
        let mut digits = String::new();
        while let Some(c) = scanner.cursor.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                scanner.cursor.next_char();
            } else {
                break;
            }
        }
        return Ok(digits);
    }
    if matches!(first, '@' | '*' | '#' | '?' | '$' | '!' | '-') {
        scanner.cursor.next_char();
        return Ok(String::from(first));
    }
    Err(scanner.cursor.err("invalid parameter name", false))
}

fn parse_brace_param(scanner: &mut Scanner<'_>) -> Result<Param, ParseError> {
    let start_err = scanner.cursor.err("unterminated ${...}", true);
    scanner.cursor.next_char(); // consume '{'

    if scanner.cursor.peek() == Some('#') {
        let after_brace = scanner.cursor.clone();
        scanner.cursor.next_char(); // consume '#'
        if scanner.cursor.peek() == Some('}') {
            scanner.cursor.next_char();
            return Ok(Param {
                name: String::from("#"),
                op: ParamOp::Plain,
            });
        }
        if let Ok(name) = read_brace_param_name(scanner) {
            if scanner.cursor.peek() == Some('}') {
                scanner.cursor.next_char();
                return Ok(Param {
                    name,
                    op: ParamOp::Length,
                });
            }
        }
        scanner.cursor = after_brace;
    }

    let name = read_brace_param_name(scanner)?;
    let op = match scanner.cursor.peek() {
        None => return Err(start_err),
        Some('}') => {
            scanner.cursor.next_char();
            return Ok(Param {
                name,
                op: ParamOp::Plain,
            });
        }
        Some(':') => {
            scanner.cursor.next_char();
            let op_ch = scanner.cursor.peek().ok_or_else(|| start_err.clone())?;
            scanner.cursor.next_char();
            let word = parse_word_inner(scanner, TildeMode::Start, StopMode::Brace)?;
            match op_ch {
                '-' => ParamOp::Default(true, word),
                '=' => ParamOp::Assign(true, word),
                '?' => ParamOp::Error(true, word),
                '+' => ParamOp::Alt(true, word),
                _ => return Err(scanner.cursor.err("bad parameter modifier", false)),
            }
        }
        Some(op_ch @ ('-' | '=' | '?' | '+')) => {
            scanner.cursor.next_char();
            let word = parse_word_inner(scanner, TildeMode::Start, StopMode::Brace)?;
            match op_ch {
                '-' => ParamOp::Default(false, word),
                '=' => ParamOp::Assign(false, word),
                '?' => ParamOp::Error(false, word),
                '+' => ParamOp::Alt(false, word),
                _ => unreachable!(),
            }
        }
        Some('%') => {
            scanner.cursor.next_char();
            let longest = if scanner.cursor.peek() == Some('%') {
                scanner.cursor.next_char();
                true
            } else {
                false
            };
            let word = parse_word_inner(scanner, TildeMode::Start, StopMode::Brace)?;
            ParamOp::TrimSuffix(longest, word)
        }
        Some('#') => {
            scanner.cursor.next_char();
            let longest = if scanner.cursor.peek() == Some('#') {
                scanner.cursor.next_char();
                true
            } else {
                false
            };
            let word = parse_word_inner(scanner, TildeMode::Start, StopMode::Brace)?;
            ParamOp::TrimPrefix(longest, word)
        }
        Some(_) => return Err(scanner.cursor.err("bad substitution", false)),
    };

    Ok(Param { name, op })
}

fn parse_dollar(scanner: &mut Scanner<'_>, in_double: bool) -> Result<Part, ParseError> {
    scanner.depth += 1;
    if scanner.depth > 128 {
        scanner.depth -= 1;
        return Err(scanner.cursor.err("nesting depth exceeded", false));
    }
    let res = parse_dollar_inner(scanner, in_double);
    scanner.depth -= 1;
    res
}

fn parse_dollar_inner(scanner: &mut Scanner<'_>, in_double: bool) -> Result<Part, ParseError> {
    scanner.cursor.next_char(); // consume '$'
    match scanner.cursor.peek() {
        Some('(') => {
            if scanner.cursor.peek2() == Some('(') {
                if let Some(raw) = try_parse_arith(scanner)? {
                    return Ok(Part::Arith(raw));
                }
            }
            scanner.cursor.next_char(); // consume '('
            let script = crate::parser::parse_cmd_subst(scanner)?;
            Ok(Part::Cmd(script))
        }
        Some('{') => Ok(Part::Param(parse_brace_param(scanner)?)),
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            let mut name = String::new();
            name.push(c);
            scanner.cursor.next_char();
            while let Some(ch) = scanner.cursor.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    name.push(ch);
                    scanner.cursor.next_char();
                } else {
                    break;
                }
            }
            Ok(Part::Param(Param {
                name,
                op: ParamOp::Plain,
            }))
        }
        Some(c) if c.is_ascii_digit() || matches!(c, '@' | '*' | '#' | '?' | '$' | '!' | '-') => {
            scanner.cursor.next_char();
            Ok(Part::Param(Param {
                name: String::from(c),
                op: ParamOp::Plain,
            }))
        }
        _ => {
            let _ = in_double;
            Ok(Part::Lit(String::from("$")))
        }
    }
}

fn parse_word_inner(
    scanner: &mut Scanner<'_>,
    tilde_mode: TildeMode,
    stop: StopMode,
) -> Result<Word, ParseError> {
    let mut parts = Vec::new();
    let mut can_tilde = tilde_mode != TildeMode::None;

    loop {
        if can_tilde {
            if let Some(t) = try_scan_tilde(scanner, tilde_mode, stop) {
                parts.push(t);
                can_tilde = false;
                continue;
            }
        }
        can_tilde = false;

        match scanner.cursor.peek() {
            None => {
                if stop == StopMode::Brace {
                    return Err(scanner.cursor.err("unterminated ${...}", true));
                }
                return Ok(parts);
            }
            Some('}') if stop == StopMode::Brace => {
                scanner.cursor.next_char();
                return Ok(parts);
            }
            Some(c) if stop == StopMode::Word && is_word_delim(c) => return Ok(parts),
            Some('\'') if stop != StopMode::EndOfInput => {
                parts.push(parse_single_quote(scanner)?);
            }
            Some('"') if stop != StopMode::EndOfInput => {
                parts.push(parse_double_quote(scanner)?);
            }
            Some('\\') => {
                scanner.cursor.next_char();
                // Windows: a backslash is a path separator, so it escapes only before these.
                if scanner.esc == Escapes::Windows
                    && !matches!(scanner.cursor.peek_raw(), Some('"' | '$' | '`' | '\\'))
                {
                    push_lit_char(&mut parts, '\\');
                    continue;
                }
                match scanner.cursor.next_raw() {
                    None => {
                        if stop == StopMode::EndOfInput {
                            push_lit_char(&mut parts, '\\');
                        } else {
                            return Err(scanner.cursor.err("trailing backslash", true));
                        }
                    }
                    Some(c) if stop == StopMode::EndOfInput => {
                        if matches!(c, '$' | '`' | '\\') {
                            parts.push(Part::Escaped(c));
                        } else {
                            push_lit_char(&mut parts, '\\');
                            push_lit_char(&mut parts, c);
                        }
                    }
                    Some(c) => parts.push(Part::Escaped(c)),
                }
            }
            Some('$') if stop != StopMode::EndOfInput && scanner.cursor.peek2() == Some('\'') => {
                parts.push(parse_ansi_c(scanner)?);
            }
            Some('$') => {
                let p = parse_dollar(scanner, false)?;
                match p {
                    Part::Lit(s) => push_lit(&mut parts, &s),
                    other => parts.push(other),
                }
            }
            Some('`') => {
                let script = parse_backquote(scanner, false)?;
                parts.push(Part::Cmd(script));
            }
            Some(c) => {
                scanner.cursor.next_char();
                push_lit_char(&mut parts, c);
                if tilde_mode == TildeMode::Assign && c == ':' {
                    can_tilde = true;
                }
            }
        }
    }
}

pub(crate) fn parse_word(
    scanner: &mut Scanner<'_>,
    tilde_mode: TildeMode,
) -> Result<Word, ParseError> {
    parse_word_inner(scanner, tilde_mode, StopMode::Word)
}

// Arithmetic text has no paths, so it always uses the POSIX escape rule.
pub(crate) fn parse_arith_template(src: &str, depth: usize) -> Result<Word, ParseError> {
    parse_unquoted_template(src, depth, Escapes::Posix)
}

pub(crate) fn parse_unquoted_template(
    src: &str,
    depth: usize,
    esc: Escapes,
) -> Result<Word, ParseError> {
    if depth > 128 {
        return Err(ParseError {
            line: 1,
            col: 1,
            msg: String::from("nesting depth exceeded"),
            incomplete: false,
        });
    }
    let mut scanner = Scanner::new(src, esc);
    scanner.depth = depth;
    let mut w = parse_word_inner(&mut scanner, TildeMode::None, StopMode::EndOfInput)?;
    if !scanner.pending_heredocs.is_empty() {
        return Err(ParseError {
            line: 1,
            col: 1,
            msg: String::from("unterminated here-document"),
            incomplete: true,
        });
    }
    crate::compound::resolve_word(&mut w, &mut scanner.heredocs);
    Ok(w)
}
