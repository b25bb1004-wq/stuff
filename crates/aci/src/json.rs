//! A small JSON value: reader (depth and size limited), writer (keys keep their order), and helpers.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::{self, Write as _};
use std::path::Path;

/// Deepest nesting of arrays and objects the reader accepts.
const MAX_DEPTH: usize = 64;
/// Largest input the reader accepts, in bytes.
const MAX_INPUT: usize = 8 * 1024 * 1024;

/// A JSON value. Objects keep their keys in insertion order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A whole number written without a fraction or exponent.
    Int(i64),
    /// Any other number. Always finite; written with a `.` or an exponent.
    Num(f64),
    /// A string.
    Str(String),
    /// An array.
    Arr(Vec<Json>),
    /// An object: key and value pairs in order.
    Obj(Vec<(String, Json)>),
}

/// Why and where a text failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    /// Byte offset in the input where the problem was found.
    pub offset: usize,
    /// What was wrong.
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}

impl Json {
    /// Read one JSON value. Fails on nesting deeper than 64, input over 8 MiB, duplicate keys,
    /// trailing text and malformed input. Never panics.
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        if text.len() > MAX_INPUT {
            return Err(JsonError {
                offset: 0,
                message: "input is larger than 8 MiB".into(),
            });
        }
        let mut p = Parser {
            b: text.as_bytes(),
            pos: 0,
            depth: 0,
        };
        let value = p.value()?;
        p.skip_ws();
        if p.pos != p.b.len() {
            return Err(p.error("trailing characters after the value"));
        }
        Ok(value)
    }

    /// Write the value as compact JSON. NaN and infinities become `null`.
    pub fn write(&self) -> String {
        let mut out = String::new();
        write_value(self, &mut out);
        out
    }

    /// The value under `key` when this is an object (the first match, though keys are unique).
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The text when this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The number when this is an `Int`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The flag when this is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The items when this is an array.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// Build an object from literal pairs, keeping their order.
    pub fn obj<const N: usize>(pairs: [(&str, Json); N]) -> Json {
        obj_vec(Vec::from(pairs))
    }

    /// Bytes as text, invalid UTF-8 replaced with U+FFFD. The flag is true when that happened.
    pub fn text(bytes: &[u8]) -> (Json, bool) {
        let (s, lossy) = lossy(bytes);
        (Json::Str(s), lossy)
    }

    /// A path as text with forward slashes, also on Windows.
    pub fn path(p: &Path) -> Json {
        Json::Str(p.to_string_lossy().replace('\\', "/"))
    }
}

/// Build an object from pairs that are already a `Vec`.
pub(crate) fn obj_vec(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Lossy UTF-8 decode; the flag is true when any byte had to be replaced.
pub(crate) fn lossy(bytes: &[u8]) -> (String, bool) {
    match String::from_utf8_lossy(bytes) {
        Cow::Borrowed(s) => (s.to_string(), false),
        Cow::Owned(s) => (s, true),
    }
}

fn write_value(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Json::Num(f) => {
            if f.is_finite() {
                // `{:?}` always keeps a `.` or an exponent, so the reader gives back a Num.
                let _ = write!(out, "{f:?}");
            } else {
                out.push_str("null");
            }
        }
        Json::Str(s) => write_str(s, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Json::Obj(pairs) => {
            out.push('{');
            for (i, (k, item)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_value(item, out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Recursive-descent reader. Every container costs one level of `depth`, so the stack stays bounded.
struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> JsonError {
        JsonError {
            offset: self.pos,
            message: message.to_string(),
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.b.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), JsonError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", c as char)))
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn nested(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<Json, JsonError>,
    ) -> Result<Json, JsonError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("nesting is deeper than 64 levels"));
        }
        self.depth += 1;
        let result = body(self);
        self.depth -= 1;
        result
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self
            .b
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(word.as_bytes()))
        {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut pairs: Vec<(String, Json)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.error("object key must be a string"));
            }
            let key = self.string()?;
            if !seen.insert(key.clone()) {
                return Err(self.error(&format!("duplicate key \"{key}\"")));
            }
            self.skip_ws();
            self.expect(b':')?;
            let value = self.value()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(&c) = self.b.get(self.pos) {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            // The run has no quote, backslash or control byte, so it is whole UTF-8 characters.
            match std::str::from_utf8(&self.b[start..self.pos]) {
                Ok(s) => out.push_str(s),
                Err(_) => return Err(self.error("invalid UTF-8")),
            }
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    out.push(self.escape()?);
                }
                Some(_) => return Err(self.error("control character in string")),
            }
        }
    }

    fn escape(&mut self) -> Result<char, JsonError> {
        let c = self
            .peek()
            .ok_or_else(|| self.error("unterminated escape"))?;
        self.pos += 1;
        Ok(match c {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let hi = self.hex4()?;
                let code = if (0xD800..0xDC00).contains(&hi) {
                    if self.b.get(self.pos..self.pos + 2) != Some(&b"\\u"[..]) {
                        return Err(self.error("unpaired surrogate"));
                    }
                    self.pos += 2;
                    let lo = self.hex4()?;
                    if !(0xDC00..0xE000).contains(&lo) {
                        return Err(self.error("unpaired surrogate"));
                    }
                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                } else if (0xDC00..0xE000).contains(&hi) {
                    return Err(self.error("unpaired surrogate"));
                } else {
                    hi
                };
                char::from_u32(code).ok_or_else(|| self.error("invalid unicode escape"))?
            }
            _ => return Err(self.error("invalid escape")),
        })
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = self
                .peek()
                .and_then(|c| (c as char).to_digit(16))
                .ok_or_else(|| self.error("invalid \\u escape"))?;
            v = v * 16 + d;
            self.pos += 1;
        }
        Ok(v)
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("invalid number")),
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            float = true;
            self.pos += 1;
            if self.digits() == 0 {
                return Err(self.error("digit expected after '.'"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("digit expected in exponent"));
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.pos])
            .map_err(|_| self.error("invalid number"))?;
        if let (false, Ok(i)) = (float, text.parse::<i64>()) {
            return Ok(Json::Int(i));
        }
        let f: f64 = text.parse().map_err(|_| self.error("invalid number"))?;
        if !f.is_finite() {
            return Err(self.error("number is out of range"));
        }
        Ok(Json::Num(f))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use assay::{forall, Rng};
    use std::path::Path;

    fn gen_string(rng: &mut Rng) -> String {
        let len = rng.below(12);
        let mut s = String::new();
        for _ in 0..len {
            if rng.chance(1, 3) {
                s.push(*rng.pick(&[
                    '"', '\\', '\n', '\r', '\t', '\u{1}', '\u{1f}', '/', ' ', '\u{7f}',
                ]));
            } else {
                loop {
                    if let Some(c) = char::from_u32(rng.range(0, 0x11_0000) as u32) {
                        s.push(c);
                        break;
                    }
                }
            }
        }
        s
    }

    fn gen_value(rng: &mut Rng, depth: u32) -> Json {
        let kinds = if depth >= 3 { 5 } else { 7 };
        match rng.below(kinds) {
            0 => Json::Null,
            1 => Json::Bool(rng.chance(1, 2)),
            2 => Json::Int(rng.u64() as i64),
            3 => {
                let f = f64::from_bits(rng.u64());
                Json::Num(if f.is_finite() { f } else { 0.5 })
            }
            4 => Json::Str(gen_string(rng)),
            5 => {
                let n = rng.below(4);
                Json::Arr((0..n).map(|_| gen_value(rng, depth + 1)).collect())
            }
            _ => {
                let mut pairs: Vec<(String, Json)> = Vec::new();
                for _ in 0..rng.below(4) {
                    let key = gen_string(rng);
                    if pairs.iter().all(|(k, _)| *k != key) {
                        pairs.push((key, gen_value(rng, depth + 1)));
                    }
                }
                Json::Obj(pairs)
            }
        }
    }

    #[test]
    fn parse_write_round_trip() {
        forall(400, 0xA11CE, |rng| {
            let v = gen_value(rng, 0);
            let text = v.write();
            assert_eq!(Json::parse(&text), Ok(v), "text was {text}");
        });
    }

    #[test]
    fn write_escapes_control_and_quotes() {
        assert_eq!(
            Json::Str("a\"b\\c\n\u{1}".into()).write(),
            r#""a\"b\\c\n\u0001""#
        );
    }

    #[test]
    fn parse_rejects_deep_nesting() {
        let deep = "[".repeat(100_000);
        assert!(Json::parse(&deep).is_err());
        let fine = format!("{}{}", "[".repeat(60), "]".repeat(60));
        assert!(Json::parse(&fine).is_ok());
    }

    #[test]
    fn parse_rejects_trailing_garbage() {
        assert!(Json::parse("1 2").is_err());
        assert!(Json::parse("{} x").is_err());
        assert!(Json::parse("[1,]").is_err());
        assert!(Json::parse("").is_err());
        assert!(Json::parse(r#"{"a":1,"a":2}"#).is_err());
        assert!(Json::parse(" 7 ").is_ok());
    }

    #[test]
    fn parse_reads_unicode_escapes_and_surrogate_pairs() {
        assert_eq!(
            Json::parse(r#""\ud83d\ude00""#),
            Ok(Json::Str("\u{1F600}".into()))
        );
        assert_eq!(Json::parse(r#""\u00e9""#), Ok(Json::Str("\u{e9}".into())));
        assert!(Json::parse(r#""\ud83d""#).is_err());
        assert!(Json::parse(r#""\ude00""#).is_err());
    }

    #[test]
    fn parse_keeps_int_and_num_apart() {
        assert_eq!(Json::parse("1"), Ok(Json::Int(1)));
        assert_eq!(Json::parse("1.0"), Ok(Json::Num(1.0)));
        assert_eq!(Json::parse("1e2"), Ok(Json::Num(100.0)));
        assert!(Json::parse("1e400").is_err());
        assert!(Json::parse("01").is_err());
    }

    #[test]
    fn object_keeps_insertion_order() {
        let text = r#"{"b":1,"a":[true,null]}"#;
        assert_eq!(Json::parse(text).unwrap().write(), text);
    }

    #[test]
    fn write_never_emits_nan() {
        assert_eq!(Json::Num(f64::NAN).write(), "null");
        assert_eq!(Json::Num(f64::INFINITY).write(), "null");
        assert_eq!(Json::Num(1.0).write(), "1.0");
        assert_eq!(Json::Int(1).write(), "1");
    }

    #[test]
    fn path_uses_forward_slashes() {
        assert_eq!(
            Json::path(Path::new(r"C:\a b\c.txt")).write(),
            r#""C:/a b/c.txt""#
        );
    }

    #[test]
    fn text_marks_lossy() {
        assert_eq!(
            Json::text(&[0x61, 0xff]),
            (Json::Str("a\u{fffd}".into()), true)
        );
        assert_eq!(Json::text(b"ok"), (Json::Str("ok".into()), false));
    }
}
