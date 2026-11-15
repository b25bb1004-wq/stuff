//! Turns Logos source text into tokens, applying the statement-end (Newline) rules.
use crate::span::{err, sp, Diag, Span};
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(test)]
mod tests;

/// One token kind. Keywords and punctuation have one variant each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// Lowercase name (values, functions, layers, builtin type names).
    LIdent(String),
    /// Capitalised name (types and variants).
    UIdent(String),
    /// Integer literal. Values above i64::MAX are only accepted for 9223372036854775808.
    Int(u64),
    /// Byte literal `b'a'`.
    Byte(u8),
    /// String literal, escapes already decoded.
    Str(String),
    /// Keyword `fn`.
    Fn,
    /// Keyword `type`.
    Type,
    /// Keyword `layer`.
    Layer,
    /// Keyword `const`.
    Const,
    /// Keyword `let`.
    Let,
    /// Keyword `var`.
    Var,
    /// Keyword `if`.
    If,
    /// Keyword `else`.
    Else,
    /// Keyword `match`.
    Match,
    /// Keyword `while`.
    While,
    /// Keyword `for`.
    For,
    /// Keyword `in`.
    In,
    /// Keyword `return`.
    Return,
    /// Keyword `break`.
    Break,
    /// Keyword `continue`.
    Continue,
    /// Keyword `assert`.
    Assert,
    /// Keyword `test`.
    Test,
    /// Keyword `prop`.
    Prop,
    /// Keyword `where`.
    Where,
    /// Keyword `uses`.
    Uses,
    /// Keyword `extern`.
    Extern,
    /// Keyword `true`.
    True,
    /// Keyword `false`.
    False,
    /// Keyword `none`.
    None,
    /// Keyword `some`.
    Some,
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `*`.
    Star,
    /// `/`.
    Slash,
    /// `%`.
    Percent,
    /// `+%`.
    PlusWrap,
    /// `-%`.
    MinusWrap,
    /// `*%`.
    StarWrap,
    /// `==`.
    EqEq,
    /// `!=`.
    NotEq,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `&&`.
    AndAnd,
    /// `||`.
    OrOr,
    /// `!`.
    Bang,
    /// `&`.
    Amp,
    /// `|`.
    Bar,
    /// `^`.
    Caret,
    /// `<<`.
    Shl,
    /// `>>`.
    Shr,
    /// `=`.
    Eq,
    /// `|>`.
    PipeArrow,
    /// `->`.
    Arrow,
    /// `=>`.
    FatArrow,
    /// `.`.
    Dot,
    /// `..`.
    DotDot,
    /// `,`.
    Comma,
    /// `:`.
    Colon,
    /// `;`, which ends a statement like a line break does.
    Semi,
    /// `?`.
    Question,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `{`.
    LBrace,
    /// `}`.
    RBrace,
    /// A statement end: a line break the rules keep, collapsed to one.
    Newline,
    /// End of input.
    Eof,
}

/// A token with the source range it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub tok: Tok,
    /// Where it came from.
    pub span: Span,
}

const FIXED: &[(&str, Tok)] = &[
    ("fn", Tok::Fn),
    ("type", Tok::Type),
    ("layer", Tok::Layer),
    ("const", Tok::Const),
    ("let", Tok::Let),
    ("var", Tok::Var),
    ("if", Tok::If),
    ("else", Tok::Else),
    ("match", Tok::Match),
    ("while", Tok::While),
    ("for", Tok::For),
    ("in", Tok::In),
    ("return", Tok::Return),
    ("break", Tok::Break),
    ("continue", Tok::Continue),
    ("assert", Tok::Assert),
    ("test", Tok::Test),
    ("prop", Tok::Prop),
    ("where", Tok::Where),
    ("uses", Tok::Uses),
    ("extern", Tok::Extern),
    ("true", Tok::True),
    ("false", Tok::False),
    ("none", Tok::None),
    ("some", Tok::Some),
    ("+", Tok::Plus),
    ("-", Tok::Minus),
    ("*", Tok::Star),
    ("/", Tok::Slash),
    ("%", Tok::Percent),
    ("+%", Tok::PlusWrap),
    ("-%", Tok::MinusWrap),
    ("*%", Tok::StarWrap),
    ("==", Tok::EqEq),
    ("!=", Tok::NotEq),
    ("<", Tok::Lt),
    ("<=", Tok::Le),
    (">", Tok::Gt),
    (">=", Tok::Ge),
    ("&&", Tok::AndAnd),
    ("||", Tok::OrOr),
    ("!", Tok::Bang),
    ("&", Tok::Amp),
    ("|", Tok::Bar),
    ("^", Tok::Caret),
    ("<<", Tok::Shl),
    (">>", Tok::Shr),
    ("=", Tok::Eq),
    ("|>", Tok::PipeArrow),
    ("->", Tok::Arrow),
    ("=>", Tok::FatArrow),
    (".", Tok::Dot),
    ("..", Tok::DotDot),
    (",", Tok::Comma),
    (":", Tok::Colon),
    (";", Tok::Semi),
    ("?", Tok::Question),
    ("(", Tok::LParen),
    (")", Tok::RParen),
    ("[", Tok::LBracket),
    ("]", Tok::RBracket),
    ("{", Tok::LBrace),
    ("}", Tok::RBrace),
];

/// Source text of a keyword or punctuation token, for messages.
pub(crate) fn fixed_text(t: &Tok) -> Option<&'static str> {
    FIXED.iter().find(|(_, k)| k == t).map(|(s, _)| *s)
}

fn lookup(text: &str) -> Option<Tok> {
    FIXED
        .iter()
        .find(|(s, _)| *s == text)
        .map(|(_, t)| t.clone())
}

/// Tokenises a whole source file. The last token is always `Eof`.
pub fn lex(src: &str) -> Result<Vec<Token>, Diag> {
    let mut lx = Lexer {
        src,
        b: src.as_bytes(),
        pos: 0,
        out: Vec::new(),
        brackets: Vec::new(),
    };
    while lx.pos < lx.b.len() {
        lx.step()?;
    }
    lx.out.push(Token {
        tok: Tok::Eof,
        span: sp(src.len(), src.len()),
    });
    Ok(lx.out)
}

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    out: Vec<Token>,
    brackets: Vec<u8>,
}

/// Tokens that cannot start a line: a Newline before them is dropped.
fn starts_line_continuation(t: &Tok) -> bool {
    matches!(
        t,
        Tok::PipeArrow
            | Tok::Dot
            | Tok::AndAnd
            | Tok::OrOr
            | Tok::Else
            | Tok::RParen
            | Tok::RBracket
            | Tok::RBrace
    )
}

/// Tokens that cannot end a line: a Newline after them is dropped.
fn ends_line_continuation(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Plus
            | Tok::Minus
            | Tok::Star
            | Tok::Slash
            | Tok::Percent
            | Tok::PlusWrap
            | Tok::MinusWrap
            | Tok::StarWrap
            | Tok::EqEq
            | Tok::NotEq
            | Tok::Lt
            | Tok::Le
            | Tok::Gt
            | Tok::Ge
            | Tok::AndAnd
            | Tok::OrOr
            | Tok::Bang
            | Tok::Amp
            | Tok::Bar
            | Tok::Caret
            | Tok::Shl
            | Tok::Shr
            | Tok::Eq
            | Tok::PipeArrow
            | Tok::Arrow
            | Tok::FatArrow
            | Tok::Dot
            | Tok::DotDot
            | Tok::Comma
            | Tok::Colon
            | Tok::LParen
            | Tok::LBracket
            | Tok::LBrace
    )
}

fn digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'f' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

impl Lexer<'_> {
    fn at(&self, i: usize) -> u8 {
        self.b.get(i).copied().unwrap_or(0)
    }

    fn push(&mut self, tok: Tok, start: usize, end: usize) {
        if starts_line_continuation(&tok)
            && matches!(
                self.out.last(),
                Some(Token {
                    tok: Tok::Newline,
                    ..
                })
            )
        {
            self.out.pop();
        }
        match tok {
            Tok::LParen => self.brackets.push(b'('),
            Tok::LBracket => self.brackets.push(b'['),
            Tok::LBrace => self.brackets.push(b'{'),
            Tok::RParen | Tok::RBracket | Tok::RBrace => {
                self.brackets.pop();
            }
            _ => {}
        }
        self.out.push(Token {
            tok,
            span: sp(start, end),
        });
    }

    fn line_break(&mut self, at: usize) {
        let inside_paren = matches!(self.brackets.last(), Some(b'(') | Some(b'['));
        let keep = match self.out.last() {
            None => false,
            Some(t) => {
                !matches!(t.tok, Tok::Newline | Tok::Semi) && !ends_line_continuation(&t.tok)
            }
        };
        if keep && !inside_paren {
            self.out.push(Token {
                tok: Tok::Newline,
                span: sp(at, at + 1),
            });
        }
    }

    fn step(&mut self) -> Result<(), Diag> {
        let start = self.pos;
        let c = self.at(start);
        match c {
            b'\n' => {
                self.pos += 1;
                self.line_break(start);
            }
            b' ' | b'\t' | b'\r' => self.pos += 1,
            b'/' if self.at(start + 1) == b'/' => {
                while self.pos < self.b.len() && self.at(self.pos) != b'\n' {
                    self.pos += 1;
                }
            }
            b';' => {
                self.pos += 1;
                let collapse = matches!(
                    self.out.last(),
                    None | Some(Token {
                        tok: Tok::Newline | Tok::Semi,
                        ..
                    })
                );
                if !collapse {
                    self.push(Tok::Semi, start, self.pos);
                }
            }
            b'"' => self.string()?,
            b'b' if self.at(start + 1) == b'\'' => self.byte_lit()?,
            b'a'..=b'z' | b'_' => self.word(false),
            b'A'..=b'Z' => self.word(true),
            b'0'..=b'9' => self.number()?,
            _ => self.punct()?,
        }
        Ok(())
    }

    fn word(&mut self, upper: bool) {
        let start = self.pos;
        loop {
            let c = self.at(self.pos);
            if c.is_ascii_alphanumeric() || (!upper && c == b'_') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = self.src.get(start..self.pos).unwrap_or("");
        let tok = if upper {
            Tok::UIdent(String::from(text))
        } else {
            lookup(text).unwrap_or_else(|| Tok::LIdent(String::from(text)))
        };
        let end = self.pos;
        self.push(tok, start, end);
    }

    fn number(&mut self) -> Result<(), Diag> {
        let start = self.pos;
        let (radix, digits_from) = match (self.at(start), self.at(start + 1)) {
            (b'0', b'x') => (16u128, start + 2),
            (b'0', b'b') => (2u128, start + 2),
            _ => (10u128, start),
        };
        self.pos = digits_from;
        let mut value: u128 = 0;
        let mut digits = 0usize;
        loop {
            let c = self.at(self.pos);
            if c == b'_' {
                self.pos += 1;
                continue;
            }
            match digit_value(c) {
                Some(d) if u128::from(d) < radix => {
                    digits += 1;
                    value = value
                        .checked_mul(radix)
                        .and_then(|v| v.checked_add(u128::from(d)))
                        .unwrap_or(u128::MAX);
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if radix != 10 && digits == 0 {
            return Err(err(
                start,
                self.pos,
                "expected digits after the number prefix",
            ));
        }
        if self.at(self.pos).is_ascii_alphanumeric() {
            return Err(err(
                self.pos,
                self.pos + 1,
                "invalid digit in number literal",
            ));
        }
        let max = u128::from(i64::MAX as u64);
        if value > max && !(radix == 10 && value == 1u128 << 63) {
            return Err(err(
                start,
                self.pos,
                "integer literal does not fit in int (i64)",
            ));
        }
        let end = self.pos;
        self.push(Tok::Int(value as u64), start, end);
        Ok(())
    }

    /// Reads an escape at `self.pos` (the backslash) and returns its code point.
    fn escape(&mut self, in_string: bool) -> Result<u32, Diag> {
        let start = self.pos;
        if start + 1 >= self.b.len() {
            return Err(err(start, start + 1, "unterminated escape sequence"));
        }
        let c = self.at(start + 1);
        self.pos += 2;
        Ok(match c {
            b'n' => 10,
            b't' => 9,
            b'r' => 13,
            b'0' => 0,
            b'\\' => 92,
            b'"' if in_string => 34,
            b'\'' if !in_string => 39,
            b'x' => {
                let v = self.hex_digits(2, start)?;
                if in_string && v > 0x7f {
                    return Err(err(
                        start,
                        self.pos,
                        "\\x escape in a string must be at most \\x7f",
                    ));
                }
                v
            }
            b'u' if in_string => {
                if self.at(self.pos) != b'{' {
                    return Err(err(start, self.pos, "expected '{' after \\u"));
                }
                self.pos += 1;
                let v = self.hex_digits(0, start)?;
                if self.at(self.pos) != b'}' {
                    return Err(err(start, self.pos, "expected '}' to close \\u{...}"));
                }
                self.pos += 1;
                if char::from_u32(v).is_none() {
                    return Err(err(start, self.pos, "invalid unicode escape"));
                }
                v
            }
            _ => {
                return Err(err(
                    start,
                    self.pos,
                    alloc::format!("unknown escape sequence '\\{}'", c as char),
                ))
            }
        })
    }

    /// Reads exactly `n` hex digits, or 1 to 6 when `n` is 0 (the \u{...} form).
    fn hex_digits(&mut self, n: usize, start: usize) -> Result<u32, Diag> {
        let from = self.pos;
        let mut v: u32 = 0;
        let limit = if n == 0 { 6 } else { n };
        while self.pos - from < limit {
            match digit_value(self.at(self.pos)) {
                Some(d) => {
                    v = v.saturating_mul(16).saturating_add(d);
                    self.pos += 1;
                }
                None => break,
            }
        }
        let got = self.pos - from;
        let ok = if n == 0 { got >= 1 } else { got == n };
        if !ok {
            return Err(err(
                start,
                self.pos,
                "expected hex digits in escape sequence",
            ));
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<(), Diag> {
        let start = self.pos;
        self.pos += 1;
        let mut buf: Vec<u8> = Vec::new();
        loop {
            if self.pos >= self.b.len() {
                return Err(err(start, self.pos, "unterminated string literal"));
            }
            match self.at(self.pos) {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\n' => {
                    return Err(err(start, self.pos, "string literal cannot span lines"));
                }
                b'\\' => {
                    let cp = self.escape(true)?;
                    let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                    let mut tmp = [0u8; 4];
                    buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                }
                c => {
                    buf.push(c);
                    self.pos += 1;
                }
            }
        }
        let text = String::from_utf8(buf)
            .map_err(|_| err(start, self.pos, "string literal is not valid UTF-8"))?;
        let end = self.pos;
        self.push(Tok::Str(text), start, end);
        Ok(())
    }

    fn byte_lit(&mut self) -> Result<(), Diag> {
        let start = self.pos;
        self.pos += 2;
        if self.pos >= self.b.len() {
            return Err(err(start, self.pos, "unterminated byte literal"));
        }
        let v: u32 = match self.at(self.pos) {
            b'\\' => self.escape(false)?,
            b'\'' => return Err(err(start, self.pos + 1, "empty byte literal")),
            c if c < 0x80 && c != b'\n' => {
                self.pos += 1;
                u32::from(c)
            }
            _ => {
                return Err(err(
                    self.pos,
                    self.pos + 1,
                    "byte literal must be one ASCII character",
                ))
            }
        };
        if self.at(self.pos) != b'\'' {
            return Err(err(start, self.pos, "expected ' to close the byte literal"));
        }
        self.pos += 1;
        let end = self.pos;
        self.push(Tok::Byte(v as u8), start, end);
        Ok(())
    }

    fn punct(&mut self) -> Result<(), Diag> {
        let start = self.pos;
        for n in [2usize, 1] {
            if let Some(tok) = self.src.get(start..start + n).and_then(lookup) {
                self.pos += n;
                self.push(tok, start, start + n);
                return Ok(());
            }
        }
        let ch = self
            .src
            .get(start..)
            .and_then(|s| s.chars().next())
            .unwrap_or('?');
        Err(err(
            start,
            start + ch.len_utf8(),
            alloc::format!("unexpected character '{}'", ch),
        ))
    }
}
