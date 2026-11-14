//! HTML5 tokenizer covering data, RCDATA, RAWTEXT, script data, tags, comments, and DOCTYPE.

use super::entities::decode_char_ref;

/// A token produced by [`tokenize`].
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// `<!DOCTYPE ...>` declaration with lowercased name.
    Doctype(String),
    /// `<tag ...>` start tag.
    StartTag {
        /// Lowercased tag name.
        name: String,
        /// Attribute `(name, value)` pairs; duplicate names keep the first.
        attrs: Vec<(String, String)>,
        /// Whether the tag ended with `/>`.
        self_closing: bool,
    },
    /// `</tag>` end tag with lowercased name.
    EndTag(String),
    /// Character data (adjacent text runs are merged).
    Text(String),
    /// `<!-- ... -->` comment text.
    Comment(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Data,
    Rcdata(String),
    Rawtext(String),
    ScriptData,
}

fn is_html_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0C')
}

fn preprocess(input: &str) -> String {
    if !input.contains('\r') {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            out.push('\n');
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            let ch = input[i..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn push_text(tokens: &mut Vec<Token>, text: &mut String) {
    if text.is_empty() {
        return;
    }
    if let Some(Token::Text(prev)) = tokens.last_mut() {
        prev.push_str(text);
        text.clear();
    } else {
        tokens.push(Token::Text(std::mem::take(text)));
    }
}

fn push_char_at(input: &str, pos: &mut usize, out: &mut String) {
    if let Some(ch) = input[*pos..].chars().next() {
        out.push(ch);
        *pos += ch.len_utf8();
    } else {
        *pos = input.len();
    }
}

/// Tokenize an HTML input string into [`Token`]s.
pub fn tokenize(input: &str) -> Vec<Token> {
    let owned = preprocess(input);
    let s = owned.as_str();
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    let mut tokens: Vec<Token> = Vec::new();
    let mut text = String::new();
    let mut mode = Mode::Data;

    while pos < bytes.len() {
        match &mode {
            Mode::Rcdata(tag) | Mode::Rawtext(tag) => {
                let is_rcdata = matches!(mode, Mode::Rcdata(_));
                if bytes[pos] == b'<' && bytes.get(pos + 1) == Some(&b'/') {
                    let after_slash = pos + 2;
                    let end = after_slash + tag.len();
                    if let Some(cand) = s.get(after_slash..end) {
                        if cand.eq_ignore_ascii_case(tag) {
                            let next_b = bytes.get(end).copied();
                            if matches!(next_b, Some(b) if is_html_ws(b) || b == b'/' || b == b'>')
                            {
                                push_text(&mut tokens, &mut text);
                                let tag_lower = tag.clone();
                                pos = end;
                                let _ = parse_tag_rest(s, &mut pos);
                                tokens.push(Token::EndTag(tag_lower));
                                mode = Mode::Data;
                                continue;
                            }
                        }
                    }
                }
                if is_rcdata && bytes[pos] == b'&' {
                    if let Some((ch, consumed)) = decode_char_ref(s, pos, false) {
                        text.push(ch);
                        pos += consumed;
                        continue;
                    }
                }
                push_char_at(s, &mut pos, &mut text);
            }
            Mode::ScriptData => {
                if bytes[pos] == b'<' && bytes.get(pos + 1) == Some(&b'/') {
                    let after_slash = pos + 2;
                    let end = after_slash + 6;
                    if let Some(cand) = s.get(after_slash..end) {
                        if cand.eq_ignore_ascii_case("script") {
                            let next_b = bytes.get(end).copied();
                            if matches!(next_b, Some(b) if is_html_ws(b) || b == b'/' || b == b'>')
                            {
                                push_text(&mut tokens, &mut text);
                                pos = end;
                                let _ = parse_tag_rest(s, &mut pos);
                                tokens.push(Token::EndTag("script".to_owned()));
                                mode = Mode::Data;
                                continue;
                            }
                        }
                    }
                }
                push_char_at(s, &mut pos, &mut text);
            }
            Mode::Data => {
                if bytes[pos] == b'&' {
                    if let Some((ch, consumed)) = decode_char_ref(s, pos, false) {
                        text.push(ch);
                        pos += consumed;
                    } else {
                        text.push('&');
                        pos += 1;
                    }
                    continue;
                }
                if bytes[pos] != b'<' {
                    push_char_at(s, &mut pos, &mut text);
                    continue;
                }
                // Starts with '<'
                let Some(&next) = bytes.get(pos + 1) else {
                    text.push('<');
                    pos += 1;
                    continue;
                };
                if next == b'!' {
                    push_text(&mut tokens, &mut text);
                    pos += 2;
                    if s[pos..].starts_with("--") {
                        pos += 2;
                        tokens.push(Token::Comment(parse_comment(s, &mut pos)));
                    } else if s
                        .get(pos..pos + 7)
                        .is_some_and(|d| d.eq_ignore_ascii_case("doctype"))
                    {
                        pos += 7;
                        tokens.push(Token::Doctype(parse_doctype(s, &mut pos)));
                    } else {
                        tokens.push(Token::Comment(parse_bogus_comment(s, &mut pos)));
                    }
                } else if next == b'/' {
                    let Some(&after) = bytes.get(pos + 2) else {
                        text.push_str("</");
                        pos += 2;
                        continue;
                    };
                    if after.is_ascii_alphabetic() {
                        push_text(&mut tokens, &mut text);
                        pos += 2;
                        let name = parse_tag_name(s, &mut pos);
                        let _ = parse_tag_rest(s, &mut pos);
                        tokens.push(Token::EndTag(name));
                    } else if after == b'>' {
                        push_text(&mut tokens, &mut text);
                        pos += 3;
                    } else {
                        push_text(&mut tokens, &mut text);
                        pos += 2;
                        tokens.push(Token::Comment(parse_bogus_comment(s, &mut pos)));
                    }
                } else if next.is_ascii_alphabetic() {
                    push_text(&mut tokens, &mut text);
                    pos += 1;
                    let name = parse_tag_name(s, &mut pos);
                    let (attrs, self_closing) = parse_tag_rest(s, &mut pos);
                    match name.as_str() {
                        "script" => mode = Mode::ScriptData,
                        "style" => mode = Mode::Rawtext("style".to_owned()),
                        "title" | "textarea" => mode = Mode::Rcdata(name.clone()),
                        _ => {}
                    }
                    tokens.push(Token::StartTag {
                        name,
                        attrs,
                        self_closing,
                    });
                } else if next == b'?' {
                    push_text(&mut tokens, &mut text);
                    pos += 2;
                    tokens.push(Token::Comment(parse_bogus_comment(s, &mut pos)));
                } else {
                    text.push('<');
                    pos += 1;
                }
            }
        }
    }

    push_text(&mut tokens, &mut text);
    tokens
}

fn parse_comment(s: &str, pos: &mut usize) -> String {
    let bytes = s.as_bytes();
    if bytes.get(*pos) == Some(&b'>') {
        *pos += 1;
        return String::new();
    }
    if s[*pos..].starts_with("->") {
        *pos += 2;
        return String::new();
    }
    let start = *pos;
    while *pos < bytes.len() {
        if bytes[*pos..].starts_with(b"-->") {
            let out = s[start..*pos].to_owned();
            *pos += 3;
            return out;
        }
        if bytes[*pos..].starts_with(b"--!>") {
            let out = s[start..*pos].to_owned();
            *pos += 4;
            return out;
        }
        *pos += 1;
    }
    s[start..].to_owned()
}

fn parse_bogus_comment(s: &str, pos: &mut usize) -> String {
    let start = *pos;
    let bytes = s.as_bytes();
    while *pos < bytes.len() {
        if bytes[*pos] == b'>' {
            let out = s[start..*pos].to_owned();
            *pos += 1;
            return out;
        }
        *pos += 1;
    }
    s[start..].to_owned()
}

fn parse_doctype(s: &str, pos: &mut usize) -> String {
    let bytes = s.as_bytes();
    while *pos < bytes.len() && is_html_ws(bytes[*pos]) {
        *pos += 1;
    }
    let mut name = String::new();
    while *pos < bytes.len() && !is_html_ws(bytes[*pos]) && bytes[*pos] != b'>' {
        if let Some(ch) = s[*pos..].chars().next() {
            name.push(ch.to_ascii_lowercase());
            *pos += ch.len_utf8();
        } else {
            break;
        }
    }
    while *pos < bytes.len() {
        if bytes[*pos] == b'>' {
            *pos += 1;
            break;
        }
        *pos += 1;
    }
    name
}

fn parse_tag_name(s: &str, pos: &mut usize) -> String {
    let bytes = s.as_bytes();
    let mut name = String::new();
    while *pos < bytes.len() {
        let b = bytes[*pos];
        if is_html_ws(b) || b == b'/' || b == b'>' {
            break;
        }
        if let Some(ch) = s[*pos..].chars().next() {
            name.push(ch.to_ascii_lowercase());
            *pos += ch.len_utf8();
        } else {
            break;
        }
    }
    name
}

fn parse_tag_rest(s: &str, pos: &mut usize) -> (Vec<(String, String)>, bool) {
    let bytes = s.as_bytes();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut self_closing = false;

    loop {
        while *pos < bytes.len() && is_html_ws(bytes[*pos]) {
            *pos += 1;
        }
        if *pos >= bytes.len() {
            break;
        }
        if bytes[*pos] == b'>' {
            *pos += 1;
            break;
        }
        if bytes[*pos] == b'/' {
            *pos += 1;
            if bytes.get(*pos) == Some(&b'>') {
                self_closing = true;
                *pos += 1;
                break;
            }
            continue;
        }

        // Attribute name
        let mut attr_name = String::new();
        while *pos < bytes.len() {
            let b = bytes[*pos];
            if is_html_ws(b) || b == b'=' || b == b'/' || b == b'>' {
                break;
            }
            if let Some(ch) = s[*pos..].chars().next() {
                attr_name.push(ch.to_ascii_lowercase());
                *pos += ch.len_utf8();
            } else {
                break;
            }
        }

        while *pos < bytes.len() && is_html_ws(bytes[*pos]) {
            *pos += 1;
        }

        let attr_val = if bytes.get(*pos) == Some(&b'=') {
            *pos += 1;
            while *pos < bytes.len() && is_html_ws(bytes[*pos]) {
                *pos += 1;
            }
            if *pos >= bytes.len() {
                String::new()
            } else if bytes[*pos] == b'"' || bytes[*pos] == b'\'' {
                let quote = bytes[*pos];
                *pos += 1;
                let mut val = String::new();
                while *pos < bytes.len() && bytes[*pos] != quote {
                    if bytes[*pos] == b'&' {
                        if let Some((ch, consumed)) = decode_char_ref(s, *pos, true) {
                            val.push(ch);
                            *pos += consumed;
                            continue;
                        }
                    }
                    push_char_at(s, pos, &mut val);
                }
                if bytes.get(*pos) == Some(&quote) {
                    *pos += 1;
                }
                val
            } else if bytes[*pos] == b'>' {
                String::new()
            } else {
                let mut val = String::new();
                while *pos < bytes.len() && !is_html_ws(bytes[*pos]) && bytes[*pos] != b'>' {
                    if bytes[*pos] == b'&' {
                        if let Some((ch, consumed)) = decode_char_ref(s, *pos, true) {
                            val.push(ch);
                            *pos += consumed;
                            continue;
                        }
                    }
                    push_char_at(s, pos, &mut val);
                }
                val
            }
        } else {
            String::new()
        };

        if !attr_name.is_empty() && !attrs.iter().any(|(k, _)| k == &attr_name) {
            attrs.push((attr_name, attr_val));
        }
    }

    (attrs, self_closing)
}
