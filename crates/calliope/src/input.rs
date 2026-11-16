//! Input events for the terminal editor: key and mouse events, the xterm byte decoder used on
//! Unix terminals, and the mapping of Windows console records. Everything here is pure, so the
//! key handling is tested without a console.

use crate::keys::Key;

/// A mouse action; positions are 0-based screen cells (row, column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    /// Left button pressed.
    Down(usize, usize),
    /// Pointer moved with the left button held.
    Drag(usize, usize),
    /// Left button released.
    Up,
    /// Wheel turned by this many lines (negative scrolls up).
    Wheel(isize),
}

/// One decoded input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ev {
    /// A key press.
    Key(Key),
    /// A navigation key (arrows, Home, End, Page keys) with Shift held.
    Shift(Key),
    /// A mouse action.
    Mouse(Mouse),
}

/// Map one typed character or control byte to a key.
fn key_from_char(c: char) -> Key {
    match c {
        '\r' | '\n' => Key::Enter,
        '\t' => Key::Tab,
        '\u{8}' | '\u{7f}' => Key::Backspace,
        '\u{1b}' => Key::Esc,
        '\u{1}'..='\u{1a}' => Key::Ctrl((c as u8 + b'a' - 1) as char),
        other => Key::Char(other),
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// Decode raw terminal bytes (Unix, raw mode) into events. Escape sequences that are not
/// understood are dropped; a lone ESC is the Escape key.
pub fn decode_bytes(bytes: &[u8]) -> Vec<Ev> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            let (ev, used) = decode_escape(&bytes[i..]);
            out.extend(ev);
            i += used;
            continue;
        }
        let end = (i + utf8_len(bytes[i])).min(bytes.len());
        if let Some(c) = std::str::from_utf8(&bytes[i..end])
            .ok()
            .and_then(|s| s.chars().next())
        {
            out.push(Ev::Key(key_from_char(c)));
        }
        i = end;
    }
    out
}

fn decode_escape(s: &[u8]) -> (Option<Ev>, usize) {
    match s.get(1) {
        None => (Some(Ev::Key(Key::Esc)), 1),
        Some(b'[') => decode_csi(s),
        Some(b'O') => match s.get(2) {
            Some(&f) => match nav_key(f, None) {
                Some(key) => (Some(Ev::Key(key)), 3),
                None => (None, 3),
            },
            None => (Some(Ev::Key(Key::Esc)), 1),
        },
        Some(_) => (Some(Ev::Key(Key::Esc)), 1),
    }
}

/// Keys named by a CSI final byte (`A`-`D`, `H`, `F`); `param` is the numeric parameter for `~`.
fn nav_key(final_byte: u8, param: Option<u32>) -> Option<Key> {
    match (final_byte, param) {
        (b'A', _) => Some(Key::Up),
        (b'B', _) => Some(Key::Down),
        (b'C', _) => Some(Key::Right),
        (b'D', _) => Some(Key::Left),
        (b'H', _) | (b'~', Some(1 | 7)) => Some(Key::Home),
        (b'F', _) | (b'~', Some(4 | 8)) => Some(Key::End),
        (b'~', Some(3)) => Some(Key::Delete),
        (b'~', Some(5)) => Some(Key::PageUp),
        (b'~', Some(6)) => Some(Key::PageDown),
        _ => None,
    }
}

fn decode_csi(s: &[u8]) -> (Option<Ev>, usize) {
    // Parameters are bytes 0x30-0x3f, intermediates 0x20-0x2f, the final byte 0x40-0x7e.
    let mut j = 2;
    while j < s.len() && (0x20..=0x3f).contains(&s[j]) {
        j += 1;
    }
    let Some(&final_byte) = s.get(j) else {
        return (Some(Ev::Key(Key::Esc)), 1);
    };
    if !(0x40..=0x7e).contains(&final_byte) {
        return (None, j + 1);
    }
    let params = std::str::from_utf8(&s[2..j]).unwrap_or("");
    let used = j + 1;
    if let Some(sgr) = params.strip_prefix('<') {
        return (decode_sgr_mouse(sgr, final_byte), used);
    }
    let fields: Vec<&str> = params.split(';').collect();
    let first = fields.first().and_then(|f| f.parse::<u32>().ok());
    // xterm modifier: 1 + bits, where Shift is bit 0.
    let shift = fields
        .get(1)
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|m| m >= 2 && (m - 1) & 1 == 1);
    let Some(key) = nav_key(final_byte, first) else {
        return (None, used);
    };
    let ev = if shift && !matches!(key, Key::Delete) {
        Ev::Shift(key)
    } else {
        Ev::Key(key)
    };
    (Some(ev), used)
}

/// Mouse report in SGR form: `button;column;row` followed by `M` (press) or `m` (release).
fn decode_sgr_mouse(body: &str, final_byte: u8) -> Option<Ev> {
    let mut parts = body.split(';').map(|p| p.parse::<usize>().ok());
    let button = parts.next()??;
    let col = parts.next()??.saturating_sub(1);
    let row = parts.next()??.saturating_sub(1);
    let mouse = if button & 64 != 0 {
        Mouse::Wheel(if button & 1 == 0 { -3 } else { 3 })
    } else if final_byte == b'm' {
        Mouse::Up
    } else if button & 32 != 0 {
        if button & 3 == 3 {
            return None;
        }
        Mouse::Drag(row, col)
    } else if button & 3 == 0 {
        Mouse::Down(row, col)
    } else {
        return None;
    };
    Some(Ev::Mouse(mouse))
}

/// Map a Windows console key record. `control` is `dwControlKeyState`; Shift is `0x10`, and
/// Alt without Ctrl (`0x3`) is ignored. Returns `None` for keys that produce no edit.
pub fn key_from_windows(vkey: u16, unit: u16, control: u32) -> Option<Ev> {
    let shift = control & 0x10 != 0;
    let nav = match vkey {
        0x21 => Some(Key::PageUp),
        0x22 => Some(Key::PageDown),
        0x23 => Some(Key::End),
        0x24 => Some(Key::Home),
        0x25 => Some(Key::Left),
        0x26 => Some(Key::Up),
        0x27 => Some(Key::Right),
        0x28 => Some(Key::Down),
        0x2e => return Some(Ev::Key(Key::Delete)),
        _ => None,
    };
    if let Some(key) = nav {
        return Some(if shift { Ev::Shift(key) } else { Ev::Key(key) });
    }
    if control & 0x3 != 0 && control & 0xc == 0 {
        return None;
    }
    let c = char::from_u32(u32::from(unit))?;
    if c == '\0' {
        return None;
    }
    Some(Ev::Key(key_from_char(c)))
}

/// Map a Windows mouse record. `buttons` and `prev` are `dwButtonState` now and at the last
/// record; `flags` is `dwEventFlags` (`1` moved, `4` wheel).
pub fn mouse_from_windows(x: i16, y: i16, buttons: u32, flags: u32, prev: u32) -> Option<Ev> {
    let row = usize::try_from(y).unwrap_or(0);
    let col = usize::try_from(x).unwrap_or(0);
    if flags & 4 != 0 {
        let delta = (buttons >> 16) as u16 as i16;
        return Some(Ev::Mouse(Mouse::Wheel(if delta > 0 { -3 } else { 3 })));
    }
    let left = buttons & 1 != 0;
    let was_left = prev & 1 != 0;
    let mouse = if flags & 1 != 0 {
        if !left {
            return None;
        }
        Mouse::Drag(row, col)
    } else if left && !was_left {
        Mouse::Down(row, col)
    } else if !left && was_left {
        Mouse::Up
    } else {
        return None;
    };
    Some(Ev::Mouse(mouse))
}
