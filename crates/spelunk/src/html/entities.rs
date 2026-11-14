//! HTML character reference decoding (numeric and named).

const WIN1252: [u32; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

fn decode_numeric_codepoint(cp: u32) -> char {
    if cp == 0 || cp > 0x10_FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return '\u{FFFD}';
    }
    if (0x80..=0x9F).contains(&cp) {
        let mapped = WIN1252[(cp - 0x80) as usize];
        return char::from_u32(mapped).unwrap_or('\u{FFFD}');
    }
    char::from_u32(cp).unwrap_or('\u{FFFD}')
}

fn lookup_named(name: &str) -> Option<(char, bool)> {
    // (decoded char, legacy_without_semicolon_allowed)
    let ch = match name {
        "amp" | "AMP" => ('&', true),
        "lt" | "LT" => ('<', true),
        "gt" | "GT" => ('>', true),
        "quot" | "QUOT" => ('"', true),
        "apos" => ('\'', false),
        "nbsp" => ('\u{00A0}', true),
        "copy" | "COPY" => ('\u{00A9}', true),
        "reg" | "REG" => ('\u{00AE}', true),
        "trade" => ('\u{2122}', false),
        "hellip" => ('\u{2026}', false),
        "mdash" => ('\u{2014}', false),
        "ndash" => ('\u{2013}', false),
        "lsquo" => ('\u{2018}', false),
        "rsquo" => ('\u{2019}', false),
        "ldquo" => ('\u{201C}', false),
        "rdquo" => ('\u{201D}', false),
        "laquo" => ('\u{00AB}', true),
        "raquo" => ('\u{00BB}', true),
        "deg" => ('\u{00B0}', true),
        "plusmn" => ('\u{00B1}', true),
        "times" => ('\u{00D7}', true),
        "divide" => ('\u{00F7}', true),
        "middot" => ('\u{00B7}', true),
        "para" => ('\u{00B6}', true),
        "sect" => ('\u{00A7}', true),
        "euro" => ('\u{20AC}', false),
        "pound" => ('\u{00A3}', true),
        "yen" => ('\u{00A5}', true),
        "cent" => ('\u{00A2}', true),
        "bull" => ('\u{2022}', false),
        "larr" => ('\u{2190}', false),
        "rarr" => ('\u{2192}', false),
        "uarr" => ('\u{2191}', false),
        "darr" => ('\u{2193}', false),
        "hearts" => ('\u{2665}', false),
        _ => return None,
    };
    Some(ch)
}

/// Decode a character reference starting at `input[pos]` (where `input[pos] == '&'`).
/// Returns `(decoded_char, bytes_consumed)` or `None` if `&` is literal.
pub(crate) fn decode_char_ref(input: &str, pos: usize, in_attr: bool) -> Option<(char, usize)> {
    let rest = input.get(pos + 1..)?;
    if let Some(after_hash) = rest.strip_prefix('#') {
        if let Some(hex_str) = after_hash
            .strip_prefix('x')
            .or_else(|| after_hash.strip_prefix('X'))
        {
            let digits_len = hex_str
                .bytes()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            if digits_len == 0 {
                return None;
            }
            let mut val: u32 = 0;
            for &b in &hex_str.as_bytes()[..digits_len] {
                let d = match b {
                    b'0'..=b'9' => (b - b'0') as u32,
                    b'a'..=b'f' => (b - b'a' + 10) as u32,
                    b'A'..=b'F' => (b - b'A' + 10) as u32,
                    _ => 0,
                };
                val = val.saturating_mul(16).saturating_add(d);
                if val > 0x11_0000 {
                    val = 0x11_0000;
                }
            }
            let semi = usize::from(hex_str.as_bytes().get(digits_len) == Some(&b';'));
            let consumed = 1 + 1 + 1 + digits_len + semi;
            return Some((decode_numeric_codepoint(val), consumed));
        }
        let digits_len = after_hash
            .bytes()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits_len == 0 {
            return None;
        }
        let mut val: u32 = 0;
        for &b in &after_hash.as_bytes()[..digits_len] {
            let d = (b - b'0') as u32;
            val = val.saturating_mul(10).saturating_add(d);
            if val > 0x11_0000 {
                val = 0x11_0000;
            }
        }
        let semi = usize::from(after_hash.as_bytes().get(digits_len) == Some(&b';'));
        let consumed = 1 + 1 + digits_len + semi;
        return Some((decode_numeric_codepoint(val), consumed));
    }

    // Named character reference
    let name_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    if name_len == 0 {
        return None;
    }
    let has_semi = rest.as_bytes().get(name_len) == Some(&b';');
    if has_semi {
        let name = &rest[..name_len];
        if let Some((ch, _)) = lookup_named(name) {
            return Some((ch, 1 + name_len + 1));
        }
        return None;
    }

    // Without semicolon: only legacy entities, and in attributes not followed by '=' or alnum
    for len in (2..=name_len).rev() {
        let prefix = &rest[..len];
        if let Some((ch, true)) = lookup_named(prefix) {
            if in_attr {
                if let Some(&next_b) = rest.as_bytes().get(len) {
                    if next_b == b'=' || next_b.is_ascii_alphanumeric() {
                        return None;
                    }
                }
            }
            return Some((ch, 1 + len));
        }
    }
    None
}
