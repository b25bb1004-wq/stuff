//! CSS Syntax Level 3 tokenizer.

/// A CSS token produced by [`tokenize`].
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Identifier token.
    Ident(String),
    /// Function token (`name(`).
    Function(String),
    /// At-keyword token (`@name`).
    AtKeyword(String),
    /// Hash token (`#value`), with `id` true when it would start an ident.
    Hash {
        /// Hash text without `#`.
        value: String,
        /// Whether `#value` is a valid ID selector / ident-starting hash.
        id: bool,
    },
    /// String token (`"..."` or `'...'`).
    Str(String),
    /// Unclosed string containing an unescaped newline.
    BadStr,
    /// Unquoted `url(...)` token.
    Url(String),
    /// Malformed unquoted `url(...)` token.
    BadUrl,
    /// Single-character delimiter.
    Delim(char),
    /// Numeric literal.
    Number {
        /// Parsed floating-point value.
        value: f64,
        /// True when the number had no `.` and no exponent.
        int: bool,
    },
    /// Percentage literal (`value%`).
    Percentage(f64),
    /// Dimension literal (`value` followed by `unit`).
    Dimension {
        /// Parsed floating-point value.
        value: f64,
        /// True when the number had no `.` and no exponent.
        int: bool,
        /// Unit identifier.
        unit: String,
    },
    /// Whitespace run.
    Whitespace,
    /// `<!--`
    Cdo,
    /// `-->`
    Cdc,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `[`
    OpenSquare,
    /// `]`
    CloseSquare,
    /// `{`
    OpenCurly,
    /// `}`
    CloseCurly,
}

fn preprocess(input: &str) -> String {
    if !input.contains(['\r', '\x0C', '\0']) {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                out.push('\n');
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'\x0C' => {
                out.push('\n');
                i += 1;
            }
            0 => {
                out.push('\u{FFFD}');
                i += 1;
            }
            _ => {
                let ch = input[i..].chars().next().unwrap_or('\u{FFFD}');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n')
}

fn is_ident_start_char(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || ch >= '\u{0080}'
}

fn is_ident_char(ch: char) -> bool {
    is_ident_start_char(ch) || ch.is_ascii_digit() || ch == '-'
}

fn is_non_printable(ch: char) -> bool {
    ch <= '\x08' || ch == '\x0B' || ('\x0E'..='\x1F').contains(&ch) || ch == '\x7F'
}

fn is_valid_escape(s: &str, pos: usize) -> bool {
    let bytes = s.as_bytes();
    bytes.get(pos) == Some(&b'\\') && bytes.get(pos + 1) != Some(&b'\n')
}

fn consume_escape(s: &str, pos: &mut usize) -> char {
    let bytes = s.as_bytes();
    if *pos >= bytes.len() {
        return '\u{FFFD}';
    }
    if bytes[*pos].is_ascii_hexdigit() {
        let mut val: u32 = 0;
        let mut count = 0;
        while count < 6 && *pos < bytes.len() && bytes[*pos].is_ascii_hexdigit() {
            let b = bytes[*pos];
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => 0,
            };
            val = val * 16 + d;
            *pos += 1;
            count += 1;
        }
        if *pos < bytes.len() && is_ws(bytes[*pos]) {
            *pos += 1;
        }
        if val == 0 || val > 0x10_FFFF || (0xD800..=0xDFFF).contains(&val) {
            return '\u{FFFD}';
        }
        return char::from_u32(val).unwrap_or('\u{FFFD}');
    }
    if let Some(ch) = s[*pos..].chars().next() {
        *pos += ch.len_utf8();
        ch
    } else {
        '\u{FFFD}'
    }
}

fn would_start_ident(s: &str, pos: usize) -> bool {
    let bytes = s.as_bytes();
    let Some(&b) = bytes.get(pos) else {
        return false;
    };
    if b == b'-' {
        let Some(next_ch) = s[pos + 1..].chars().next() else {
            return false;
        };
        return is_ident_start_char(next_ch) || next_ch == '-' || is_valid_escape(s, pos + 1);
    }
    if b == b'\\' {
        return is_valid_escape(s, pos);
    }
    s[pos..].chars().next().is_some_and(is_ident_start_char)
}

fn would_start_number(s: &str, pos: usize) -> bool {
    let bytes = s.as_bytes();
    let Some(&b) = bytes.get(pos) else {
        return false;
    };
    if b == b'+' || b == b'-' {
        let Some(&b2) = bytes.get(pos + 1) else {
            return false;
        };
        if b2.is_ascii_digit() {
            return true;
        }
        return b2 == b'.' && bytes.get(pos + 2).is_some_and(|c| c.is_ascii_digit());
    }
    if b == b'.' {
        return bytes.get(pos + 1).is_some_and(|c| c.is_ascii_digit());
    }
    b.is_ascii_digit()
}

fn consume_ident(s: &str, pos: &mut usize) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    while *pos < bytes.len() {
        if bytes[*pos] == b'\\' && is_valid_escape(s, *pos) {
            *pos += 1;
            out.push(consume_escape(s, pos));
        } else if let Some(ch) = s[*pos..].chars().next() {
            if is_ident_char(ch) {
                out.push(ch);
                *pos += ch.len_utf8();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    out
}

fn consume_number(s: &str, pos: &mut usize) -> (f64, bool) {
    let bytes = s.as_bytes();
    let start = *pos;
    if matches!(bytes.get(*pos), Some(b'+' | b'-')) {
        *pos += 1;
    }
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }
    let mut has_dot = false;
    if bytes.get(*pos) == Some(&b'.') && bytes.get(*pos + 1).is_some_and(|b| b.is_ascii_digit()) {
        has_dot = true;
        *pos += 1;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
    }
    let mut has_exp = false;
    if matches!(bytes.get(*pos), Some(b'e' | b'E')) {
        let mut exp_pos = *pos + 1;
        if matches!(bytes.get(exp_pos), Some(b'+' | b'-')) {
            exp_pos += 1;
        }
        if bytes.get(exp_pos).is_some_and(|b| b.is_ascii_digit()) {
            has_exp = true;
            *pos = exp_pos;
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
        }
    }
    let value = s[start..*pos].parse::<f64>().unwrap_or(0.0);
    (value, !has_dot && !has_exp)
}

fn consume_string(s: &str, pos: &mut usize, quote: u8) -> Token {
    let bytes = s.as_bytes();
    let mut out = String::new();
    while *pos < bytes.len() {
        let b = bytes[*pos];
        if b == quote {
            *pos += 1;
            return Token::Str(out);
        }
        if b == b'\n' {
            return Token::BadStr;
        }
        if b == b'\\' {
            if *pos + 1 >= bytes.len() {
                *pos += 1;
            } else if bytes[*pos + 1] == b'\n' {
                *pos += 2;
            } else {
                *pos += 1;
                out.push(consume_escape(s, pos));
            }
            continue;
        }
        if let Some(ch) = s[*pos..].chars().next() {
            out.push(ch);
            *pos += ch.len_utf8();
        } else {
            break;
        }
    }
    Token::Str(out)
}

fn consume_bad_url_remnants(s: &str, pos: &mut usize) {
    let bytes = s.as_bytes();
    while *pos < bytes.len() {
        let b = bytes[*pos];
        if b == b')' {
            *pos += 1;
            return;
        }
        if b == b'\\' && is_valid_escape(s, *pos) {
            *pos += 1;
            let _ = consume_escape(s, pos);
        } else if let Some(ch) = s[*pos..].chars().next() {
            *pos += ch.len_utf8();
        } else {
            break;
        }
    }
}

fn consume_url(s: &str, pos: &mut usize) -> Token {
    let bytes = s.as_bytes();
    while *pos < bytes.len() && is_ws(bytes[*pos]) {
        *pos += 1;
    }
    let mut out = String::new();
    while *pos < bytes.len() {
        let b = bytes[*pos];
        if b == b')' {
            *pos += 1;
            return Token::Url(out);
        }
        if is_ws(b) {
            while *pos < bytes.len() && is_ws(bytes[*pos]) {
                *pos += 1;
            }
            if *pos >= bytes.len() {
                return Token::Url(out);
            }
            if bytes[*pos] == b')' {
                *pos += 1;
                return Token::Url(out);
            }
            consume_bad_url_remnants(s, pos);
            return Token::BadUrl;
        }
        if matches!(b, b'"' | b'\'' | b'(') {
            consume_bad_url_remnants(s, pos);
            return Token::BadUrl;
        }
        if b == b'\\' {
            if is_valid_escape(s, *pos) {
                *pos += 1;
                out.push(consume_escape(s, pos));
            } else {
                consume_bad_url_remnants(s, pos);
                return Token::BadUrl;
            }
            continue;
        }
        if let Some(ch) = s[*pos..].chars().next() {
            if is_non_printable(ch) {
                consume_bad_url_remnants(s, pos);
                return Token::BadUrl;
            }
            out.push(ch);
            *pos += ch.len_utf8();
        } else {
            break;
        }
    }
    Token::Url(out)
}

/// Tokenize a CSS input string into [`Token`]s (comments are dropped).
pub fn tokenize(input: &str) -> Vec<Token> {
    let owned = preprocess(input);
    let s = owned.as_str();
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    let mut tokens = Vec::new();

    while pos < bytes.len() {
        // Comments
        if bytes[pos] == b'/' && bytes.get(pos + 1) == Some(&b'*') {
            pos += 2;
            while pos < bytes.len() {
                if bytes[pos] == b'*' && bytes.get(pos + 1) == Some(&b'/') {
                    pos += 2;
                    break;
                }
                pos += 1;
            }
            continue;
        }

        let b = bytes[pos];
        if is_ws(b) {
            while pos < bytes.len() && is_ws(bytes[pos]) {
                pos += 1;
            }
            tokens.push(Token::Whitespace);
            continue;
        }

        match b {
            b'"' | b'\'' => {
                pos += 1;
                tokens.push(consume_string(s, &mut pos, b));
            }
            b'#' => {
                pos += 1;
                let has_ident_char =
                    is_valid_escape(s, pos) || s[pos..].chars().next().is_some_and(is_ident_char);
                if has_ident_char {
                    let id = would_start_ident(s, pos);
                    let value = consume_ident(s, &mut pos);
                    tokens.push(Token::Hash { value, id });
                } else {
                    tokens.push(Token::Delim('#'));
                }
            }
            b'(' => {
                pos += 1;
                tokens.push(Token::OpenParen);
            }
            b')' => {
                pos += 1;
                tokens.push(Token::CloseParen);
            }
            b',' => {
                pos += 1;
                tokens.push(Token::Comma);
            }
            b':' => {
                pos += 1;
                tokens.push(Token::Colon);
            }
            b';' => {
                pos += 1;
                tokens.push(Token::Semicolon);
            }
            b'[' => {
                pos += 1;
                tokens.push(Token::OpenSquare);
            }
            b']' => {
                pos += 1;
                tokens.push(Token::CloseSquare);
            }
            b'{' => {
                pos += 1;
                tokens.push(Token::OpenCurly);
            }
            b'}' => {
                pos += 1;
                tokens.push(Token::CloseCurly);
            }
            b'@' => {
                pos += 1;
                if would_start_ident(s, pos) {
                    tokens.push(Token::AtKeyword(consume_ident(s, &mut pos)));
                } else {
                    tokens.push(Token::Delim('@'));
                }
            }
            b'<' if s[pos..].starts_with("<!--") => {
                pos += 4;
                tokens.push(Token::Cdo);
            }
            b'-' => {
                if would_start_number(s, pos) {
                    tokens.push(tokenize_numeric(s, &mut pos));
                } else if s[pos..].starts_with("-->") {
                    pos += 3;
                    tokens.push(Token::Cdc);
                } else if would_start_ident(s, pos) {
                    tokens.push(tokenize_ident_like(s, &mut pos));
                } else {
                    pos += 1;
                    tokens.push(Token::Delim('-'));
                }
            }
            b'+' | b'.' => {
                if would_start_number(s, pos) {
                    tokens.push(tokenize_numeric(s, &mut pos));
                } else {
                    pos += 1;
                    tokens.push(Token::Delim(b as char));
                }
            }
            b'0'..=b'9' => {
                tokens.push(tokenize_numeric(s, &mut pos));
            }
            b'\\' => {
                if is_valid_escape(s, pos) {
                    tokens.push(tokenize_ident_like(s, &mut pos));
                } else {
                    pos += 1;
                    tokens.push(Token::Delim('\\'));
                }
            }
            _ => {
                if would_start_ident(s, pos) {
                    tokens.push(tokenize_ident_like(s, &mut pos));
                } else if let Some(ch) = s[pos..].chars().next() {
                    pos += ch.len_utf8();
                    tokens.push(Token::Delim(ch));
                } else {
                    break;
                }
            }
        }
    }

    tokens
}

fn tokenize_numeric(s: &str, pos: &mut usize) -> Token {
    let (value, int) = consume_number(s, pos);
    if would_start_ident(s, *pos) {
        let unit = consume_ident(s, pos);
        Token::Dimension { value, int, unit }
    } else if s.as_bytes().get(*pos) == Some(&b'%') {
        *pos += 1;
        Token::Percentage(value)
    } else {
        Token::Number { value, int }
    }
}

fn tokenize_ident_like(s: &str, pos: &mut usize) -> Token {
    let ident = consume_ident(s, pos);
    let bytes = s.as_bytes();
    if bytes.get(*pos) == Some(&b'(') {
        *pos += 1;
        if ident.eq_ignore_ascii_case("url") {
            let mut peek = *pos;
            while peek < bytes.len() && is_ws(bytes[peek]) {
                peek += 1;
            }
            if matches!(bytes.get(peek), Some(b'"' | b'\'')) {
                return Token::Function(ident);
            }
            return consume_url(s, pos);
        }
        Token::Function(ident)
    } else {
        Token::Ident(ident)
    }
}
