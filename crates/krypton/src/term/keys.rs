//! Key events and their decoding. Pure functions, so they are tested without a console.

/// One logical key press read from the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    /// A printable character.
    Char(char),
    /// Control plus a lowercase ASCII letter (`Ctrl('a')` is Ctrl-A).
    Ctrl(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// End of input (console closed or stdin ended).
    Eof,
}

/// Decode raw terminal bytes (Unix raw mode) into keys. A lone ESC is the Escape key.
#[cfg(any(unix, test))]
pub(crate) fn decode_bytes(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            let (key, used) = decode_escape(&bytes[i..]);
            keys.push(key);
            i += used;
            continue;
        }
        if b >= 0x80 {
            let len = match b {
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => 1,
            };
            if let Some(chunk) = bytes.get(i..i + len) {
                if let Some(c) = std::str::from_utf8(chunk)
                    .ok()
                    .and_then(|s| s.chars().next())
                {
                    keys.push(Key::Char(c));
                }
            }
            i += len;
            continue;
        }
        keys.push(key_from_control_byte(b));
        i += 1;
    }
    keys
}

#[cfg(any(unix, windows, test))]
fn key_from_control_byte(b: u8) -> Key {
    match b {
        b'\r' | b'\n' => Key::Enter,
        b'\t' => Key::Tab,
        0x08 | 0x7f => Key::Backspace,
        1..=26 => Key::Ctrl((b - 1 + b'a') as char),
        _ => Key::Char(b as char),
    }
}

/// Decode an escape sequence starting at `bytes[0] == 0x1b`; returns the key and bytes used.
#[cfg(any(unix, test))]
fn decode_escape(bytes: &[u8]) -> (Key, usize) {
    match bytes.get(1) {
        Some(b'[') | Some(b'O') => {}
        _ => return (Key::Escape, 1),
    }
    // Parameters, then one final byte in 0x40..=0x7e.
    let mut j = 2;
    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
        j += 1;
    }
    let Some(&last) = bytes.get(j) else {
        return (Key::Escape, 1);
    };
    let params = std::str::from_utf8(&bytes[2..j]).unwrap_or("");
    let key = match last {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'~' => match params {
            "1" | "7" => Key::Home,
            "4" | "8" => Key::End,
            "3" => Key::Delete,
            _ => Key::Escape,
        },
        _ => Key::Escape,
    };
    (key, j + 1)
}

/// Map a Windows console key event (virtual key code and UTF-16 unit) to a key.
#[cfg(any(windows, test))]
pub(crate) fn key_from_win_event(vkey: u16, unit: u16) -> Option<Key> {
    let special = match vkey {
        0x08 => Some(Key::Backspace),
        0x09 => Some(Key::Tab),
        0x0d => Some(Key::Enter),
        0x1b => Some(Key::Escape),
        0x23 => Some(Key::End),
        0x24 => Some(Key::Home),
        0x25 => Some(Key::Left),
        0x26 => Some(Key::Up),
        0x27 => Some(Key::Right),
        0x28 => Some(Key::Down),
        0x2e => Some(Key::Delete),
        _ => None,
    };
    if special.is_some() {
        return special;
    }
    match unit {
        0 => None,
        0x7f => Some(Key::Backspace),
        1..=26 => key_from_control_byte(unit as u8).into(),
        _ => char::from_u32(u32::from(unit)).map(Key::Char),
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_bytes, key_from_win_event, Key};
    use std::vec;

    #[test]
    fn arrows_and_editing_keys_decode_from_escape_sequences() {
        let keys = decode_bytes(b"\x1b[A\x1b[B\x1b[C\x1b[D\x1b[H\x1b[F\x1b[3~\x1bOH\x1b[1~");
        assert_eq!(
            keys,
            vec![
                Key::Up,
                Key::Down,
                Key::Right,
                Key::Left,
                Key::Home,
                Key::End,
                Key::Delete,
                Key::Home,
                Key::Home
            ]
        );
    }

    #[test]
    fn control_bytes_and_text_decode() {
        assert_eq!(
            decode_bytes(b"a\x01\x05\x17\x03\r\t\x7f\x08"),
            vec![
                Key::Char('a'),
                Key::Ctrl('a'),
                Key::Ctrl('e'),
                Key::Ctrl('w'),
                Key::Ctrl('c'),
                Key::Enter,
                Key::Tab,
                Key::Backspace,
                Key::Backspace
            ]
        );
        assert_eq!(decode_bytes("é".as_bytes()), vec![Key::Char('é')]);
        assert_eq!(decode_bytes(b"\x1b"), vec![Key::Escape]);
    }

    #[test]
    fn windows_events_map_special_keys_and_text() {
        assert_eq!(key_from_win_event(0x25, 0), Some(Key::Left));
        assert_eq!(key_from_win_event(0x2e, 0), Some(Key::Delete));
        assert_eq!(key_from_win_event(0, 0x03), Some(Key::Ctrl('c')));
        assert_eq!(
            key_from_win_event(0x52, u16::from(b'r')),
            Some(Key::Char('r'))
        );
        assert_eq!(key_from_win_event(0x10, 0), None);
    }
}
