//! Key representation, test key-notation parser, and terminal byte decoder.

/// A keyboard input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Printable character.
    Char(char),
    /// Control key combination (`Ctrl('r')`, etc.).
    Ctrl(char),
    /// Enter / Return key.
    Enter,
    /// Escape key.
    Esc,
    /// Backspace key.
    Backspace,
    /// Delete key.
    Delete,
    /// Tab key.
    Tab,
    /// Left arrow key.
    Left,
    /// Right arrow key.
    Right,
    /// Up arrow key.
    Up,
    /// Down arrow key.
    Down,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
}

/// Parse test key notation (e.g. `<Esc>`, `<CR>`, `<C-r>`, `<lt>`, literal chars) into `Key`s.
pub fn parse_keys(s: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(ch) = rest.chars().next() {
        if ch == '<' {
            if let Some((key, consumed)) = parse_tag(rest) {
                out.push(key);
                rest = &rest[consumed..];
                continue;
            }
        }
        out.push(Key::Char(ch));
        rest = &rest[ch.len_utf8()..];
    }
    out
}

fn parse_tag(s: &str) -> Option<(Key, usize)> {
    let end = s.find('>')?;
    let inner = &s[1..end];
    let key = match inner {
        "Esc" => Key::Esc,
        "CR" => Key::Enter,
        "BS" => Key::Backspace,
        "Del" => Key::Delete,
        "Tab" => Key::Tab,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Home" => Key::Home,
        "End" => Key::End,
        "PgUp" => Key::PageUp,
        "PgDn" => Key::PageDown,
        "lt" => Key::Char('<'),
        _ => {
            let after_c = inner.strip_prefix("C-")?;
            let mut chars = after_c.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Key::Ctrl(c)
        }
    };
    Some((key, end + 1))
}

/// Decode raw terminal input bytes into `Key` events; unknown sequences are dropped.
pub fn decode_keys(bytes: &[u8]) -> Vec<Key> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            13 => {
                out.push(Key::Enter);
                i += 1;
            }
            9 => {
                out.push(Key::Tab);
                i += 1;
            }
            8 | 127 => {
                out.push(Key::Backspace);
                i += 1;
            }
            27 => {
                if i + 1 >= bytes.len() {
                    out.push(Key::Esc);
                    i += 1;
                } else if bytes[i + 1] == b'[' {
                    i += decode_csi(&bytes[i + 2..], &mut out) + 2;
                } else if bytes[i + 1] == b'O' {
                    i += if i + 2 < bytes.len() { 3 } else { 2 };
                } else {
                    out.push(Key::Esc);
                    i += 1;
                }
            }
            1..=26 => {
                let letter = (b - 1 + b'a') as char;
                out.push(Key::Ctrl(letter));
                i += 1;
            }
            0x20..=0x7E => {
                out.push(Key::Char(b as char));
                i += 1;
            }
            0x80..=0xFF => {
                if let Some((c, len)) = decode_utf8_char(&bytes[i..]) {
                    if !c.is_control() {
                        out.push(Key::Char(c));
                    }
                    i += len;
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    out
}

fn decode_csi(rest: &[u8], out: &mut Vec<Key>) -> usize {
    let mut end = 0;
    while end < rest.len() {
        let b = rest[end];
        end += 1;
        if (0x40..=0x7E).contains(&b) {
            break;
        }
    }
    let seq = &rest[..end];
    match seq {
        b"A" => out.push(Key::Up),
        b"B" => out.push(Key::Down),
        b"C" => out.push(Key::Right),
        b"D" => out.push(Key::Left),
        b"H" => out.push(Key::Home),
        b"F" => out.push(Key::End),
        b"3~" => out.push(Key::Delete),
        b"5~" => out.push(Key::PageUp),
        b"6~" => out.push(Key::PageDown),
        _ => {}
    }
    end
}

fn decode_utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    for len in 2..=4 {
        if bytes.len() >= len {
            if let Ok(s) = std::str::from_utf8(&bytes[..len]) {
                if let Some(c) = s.chars().next() {
                    return Some((c, len));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_keys_notation() {
        let keys = parse_keys("iab<CR><Esc><C-r><lt><PgUp><PgDn><Home><End><Del><BS><Tab>");
        assert_eq!(
            keys,
            vec![
                Key::Char('i'),
                Key::Char('a'),
                Key::Char('b'),
                Key::Enter,
                Key::Esc,
                Key::Ctrl('r'),
                Key::Char('<'),
                Key::PageUp,
                Key::PageDown,
                Key::Home,
                Key::End,
                Key::Delete,
                Key::Backspace,
                Key::Tab,
            ]
        );
        assert_eq!(
            parse_keys("<unknown>"),
            vec![
                Key::Char('<'),
                Key::Char('u'),
                Key::Char('n'),
                Key::Char('k'),
                Key::Char('n'),
                Key::Char('o'),
                Key::Char('w'),
                Key::Char('n'),
                Key::Char('>'),
            ]
        );
    }

    #[test]
    fn decode_keys_sequences() {
        let bytes = b"a\r\t\x7f\x08\x12\x1b[A\x1b[B\x1b[C\x1b[D\x1b[H\x1b[F\x1b[3~\x1b[5~\x1b[6~\x1b[99~\xc3\xa9\x1b";
        let keys = decode_keys(bytes);
        assert_eq!(
            keys,
            vec![
                Key::Char('a'),
                Key::Enter,
                Key::Tab,
                Key::Backspace,
                Key::Backspace,
                Key::Ctrl('r'),
                Key::Up,
                Key::Down,
                Key::Right,
                Key::Left,
                Key::Home,
                Key::End,
                Key::Delete,
                Key::PageUp,
                Key::PageDown,
                Key::Char('é'),
                Key::Esc,
            ]
        );
    }
}
