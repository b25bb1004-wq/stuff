//! Terminal editor tests: key sequences drive a `Session` and the frames it renders are checked.
//! No console is needed; the console shim in the binary only translates bytes and records.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use calliope::file::{self, Eol};
use calliope::{ansi_changed, Ev, Key, Mouse, Session, Style};

fn session(text: &str, name: Option<&str>) -> Session {
    let path = name.map(PathBuf::from);
    Session::from_bytes(path, text.as_bytes(), 40, 10).unwrap()
}

/// Feed text as key events: `\n` is Enter, everything else is a typed character.
fn type_str(s: &mut Session, text: &str) {
    for c in text.chars() {
        let key = if c == '\n' { Key::Enter } else { Key::Char(c) };
        s.handle(Ev::Key(key));
    }
}

fn press(s: &mut Session, key: Key) {
    s.handle(Ev::Key(key));
}

fn ctrl(s: &mut Session, c: char) {
    s.handle(Ev::Key(Key::Ctrl(c)));
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("calliope_term_{label}_{}", std::process::id()))
}

#[test]
fn typing_enter_and_backspace() {
    let mut s = session("", None);
    type_str(&mut s, "hello\nwor");
    press(&mut s, Key::Backspace);
    type_str(&mut s, "rld");
    assert_eq!(s.text(), "hello\nworld");
    assert!(s.modified());
    assert_eq!(s.cursor_line_col(), (2, 6));
}

#[test]
fn shift_select_copy_paste_then_undo() {
    let mut s = session("abc", Some("notes.txt"));
    s.handle(Ev::Shift(Key::Right));
    s.handle(Ev::Shift(Key::Right));
    ctrl(&mut s, 'c');
    press(&mut s, Key::End);
    ctrl(&mut s, 'v');
    assert_eq!(s.text(), "abcab");
    ctrl(&mut s, 'z');
    assert_eq!(s.text(), "abc");
    ctrl(&mut s, 'y');
    assert_eq!(s.text(), "abcab");
}

#[test]
fn cut_removes_selection_and_paste_replaces_it() {
    let mut s = session("one two", None);
    press(&mut s, Key::Right);
    s.handle(Ev::Shift(Key::Right));
    s.handle(Ev::Shift(Key::Right));
    ctrl(&mut s, 'x');
    assert_eq!(s.text(), "o two");
    press(&mut s, Key::End);
    ctrl(&mut s, 'v');
    assert_eq!(s.text(), "o twone");
}

#[test]
fn search_finds_and_wraps() {
    let mut s = session("foo bar foo baz", None);
    ctrl(&mut s, 'f');
    type_str(&mut s, "foo");
    press(&mut s, Key::Enter);
    ctrl(&mut s, 'n');
    press(&mut s, Key::Char('X'));
    assert_eq!(s.text(), "foo bar X baz");
    press(&mut s, Key::Backspace);
    // The only "foo" left is at the start, so the next search wraps back to it.
    ctrl(&mut s, 'n');
    press(&mut s, Key::Char('Y'));
    assert_eq!(s.text(), "Y bar  baz");

    let mut s = session("foo bar foo", None);
    ctrl(&mut s, 'f');
    type_str(&mut s, "foo");
    press(&mut s, Key::Enter);
    ctrl(&mut s, 'n');
    ctrl(&mut s, 'p');
    press(&mut s, Key::Char('Z'));
    assert_eq!(s.text(), "Z bar foo");
}

#[test]
fn go_to_line_moves_the_cursor() {
    let mut s = session("a\nb\nc\n", None);
    ctrl(&mut s, 'g');
    type_str(&mut s, "3");
    press(&mut s, Key::Enter);
    assert_eq!(s.cursor_line_col(), (3, 1));
    ctrl(&mut s, 'g');
    type_str(&mut s, "99");
    press(&mut s, Key::Enter);
    assert!(s.message().contains("no line 99"));
    assert_eq!(s.cursor_line_col(), (3, 1));
}

#[test]
fn quit_asks_when_dirty() {
    let mut s = session("", None);
    type_str(&mut s, "x");
    ctrl(&mut s, 'q');
    assert!(!s.quit_requested());
    press(&mut s, Key::Char('n'));
    assert!(s.quit_requested());

    let mut clean = session("x", None);
    ctrl(&mut clean, 'q');
    assert!(clean.quit_requested());
}

#[test]
fn mouse_click_places_cursor_and_wheel_scrolls() {
    let text: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    let mut s = session(&text, None);
    s.handle(Ev::Mouse(Mouse::Down(2, 5)));
    assert_eq!(s.cursor_line_col(), (3, 6));
    s.handle(Ev::Mouse(Mouse::Wheel(3)));
    let frame = s.render();
    assert!(
        frame.rows[0].starts_with("line 4"),
        "row 0 was {:?}",
        frame.rows[0]
    );
    // The cursor stayed on line 3, which scrolled out of view, so it moved to the top row.
    assert_eq!(s.cursor_line_col(), (4, 6));
}

#[test]
fn mouse_drag_selects() {
    let mut s = session("abcdef", None);
    s.handle(Ev::Mouse(Mouse::Down(0, 1)));
    s.handle(Ev::Mouse(Mouse::Drag(0, 4)));
    s.handle(Ev::Mouse(Mouse::Up));
    ctrl(&mut s, 'c');
    press(&mut s, Key::End);
    ctrl(&mut s, 'v');
    assert_eq!(s.text(), "abcdefbcd");
}

#[test]
fn render_has_exact_size_and_status_line() {
    let mut s = session("hello\nworld", Some("a.txt"));
    let frame = s.render();
    assert_eq!(frame.rows.len(), 10);
    assert!(frame.rows.iter().all(|r| r.chars().count() == 40));
    assert!(frame.rows[0].starts_with("hello"));
    assert!(frame.rows[9].contains("a.txt") && frame.rows[9].contains("2 lines"));
    assert_eq!(frame.cursor, (0, 0));
    type_str(&mut s, "!");
    let frame = s.render();
    assert!(frame.rows[9].contains("a.txt [+]"));
    assert_eq!(frame.cursor, (0, 1));
}

#[test]
fn rust_colours_keywords_strings_and_comments() {
    let mut s = session("fn main() { let x = \"hi\"; // c\n}", Some("main.rs"));
    let frame = s.render();
    let row = &frame.styles[0];
    assert_eq!(row[0], Style::Keyword);
    assert_eq!(row[3], Style::Plain);
    let quote = frame.rows[0].find('"').unwrap();
    assert_eq!(row[quote], Style::String);
    let slashes = frame.rows[0].find("//").unwrap();
    assert_eq!(row[slashes], Style::Comment);
}

#[test]
fn rust_block_comment_state_carries_to_later_lines() {
    let mut s = session("/* a\nb */ fn f() {}", Some("x.rs"));
    let frame = s.render();
    assert_eq!(frame.styles[1][0], Style::Comment);
    assert_eq!(frame.styles[1][5], Style::Keyword);
}

#[test]
fn markdown_fence_and_headings() {
    let mut s = session("```\nfn x\n```\n# Title", Some("n.md"));
    let frame = s.render();
    assert_eq!(frame.styles[0][0], Style::Keyword);
    assert_eq!(frame.styles[1][0], Style::String);
    assert_eq!(frame.styles[3][0], Style::Keyword);
}

#[test]
fn logos_file_uses_the_logos_highlighter() {
    let mut s = session("let x = 1", Some("a.lg"));
    let frame = s.render();
    assert_eq!(frame.styles[0].len(), 40);
}

#[test]
fn changed_row_only_redraw() {
    let mut s = session("one\ntwo\nthree", None);
    let before = s.render();
    type_str(&mut s, "z");
    let after = s.render();
    let diff = ansi_changed(Some(&before), &after);
    assert!(diff.contains("\x1b[1;1H"));
    assert!(!diff.contains("\x1b[2;1H"));
    assert!(!diff.contains("\x1b[3;1H"));
    let full = ansi_changed(None, &after);
    assert!(full.contains("\x1b[3;1H"));
}

#[test]
fn invalid_utf8_is_refused() {
    let err = Session::from_bytes(None, &[b'a', 0xff, b'b'], 10, 4)
        .err()
        .unwrap();
    assert_eq!(err, file::DecodeError::InvalidUtf8 { at: 1 });
}

#[test]
fn crlf_round_trips_through_save() {
    let path = temp_path("crlf");
    let original = b"one\r\ntwo\r\n".to_vec();
    fs::write(&path, &original).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mut s = Session::from_bytes(Some(path.clone()), &bytes, 40, 10).unwrap();
    assert_eq!(s.eol(), Eol::Crlf);
    assert_eq!(s.text(), "one\ntwo\n");
    assert!(s.save());
    assert_eq!(fs::read(&path).unwrap(), original);

    press(&mut s, Key::Enter);
    assert!(s.save());
    assert_eq!(fs::read(&path).unwrap(), b"\r\none\r\ntwo\r\n".to_vec());
    let _ = fs::remove_file(&path);
}

#[test]
fn missing_trailing_newline_is_not_added() {
    let path = temp_path("nonl");
    fs::write(&path, b"one\r\ntwo").unwrap();
    let bytes = fs::read(&path).unwrap();
    let mut s = Session::from_bytes(Some(path.clone()), &bytes, 40, 10).unwrap();
    press(&mut s, Key::Down);
    press(&mut s, Key::End);
    type_str(&mut s, "!");
    assert!(s.save());
    assert_eq!(fs::read(&path).unwrap(), b"one\r\ntwo!".to_vec());
    let _ = fs::remove_file(&path);
}

#[test]
fn mixed_line_endings_are_kept_byte_for_byte() {
    let raw = b"a\r\nb\nc\r\n".to_vec();
    let (text, eol) = file::decode(&raw).unwrap();
    assert_eq!(eol, Eol::Mixed);
    assert_eq!(file::encode(&text, eol).into_bytes(), raw);
}

/// Build about 10 MB of text in 80-column lines.
fn big_text() -> String {
    let mut text = String::with_capacity(10_500_000);
    let mut i = 0usize;
    while text.len() < 10_000_000 {
        text.push_str(&format!(
            "{i:08} the quick brown fox jumps over the lazy dog {:>36}\n",
            i * 7
        ));
        i += 1;
    }
    text
}

#[test]
fn ten_megabyte_file_timings() {
    let text = big_text();
    let path = temp_path("big");
    fs::write(&path, text.as_bytes()).unwrap();
    let bytes = fs::read(&path).unwrap();
    let lines = text.matches('\n').count();

    let t0 = Instant::now();
    let mut s = Session::from_bytes(Some(path.clone()), &bytes, 120, 40).unwrap();
    let _ = s.render();
    let open_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    ctrl(&mut s, 'g');
    type_str(&mut s, &lines.to_string());
    press(&mut s, Key::Enter);
    let frame = s.render();
    let jump_ms = t1.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(s.cursor_line_col().0, lines);
    assert!(frame.rows.iter().all(|r| r.chars().count() == 120));

    let t2 = Instant::now();
    let keys = 100;
    for k in 0..keys {
        press(&mut s, Key::Char((b'a' + (k % 26) as u8) as char));
        let _ = s.render();
    }
    let type_ms = t2.elapsed().as_secs_f64() * 1000.0;

    let t3 = Instant::now();
    for _ in 0..50 {
        s.handle(Ev::Mouse(Mouse::Wheel(-3)));
        let _ = s.render();
    }
    let scroll_ms = t3.elapsed().as_secs_f64() * 1000.0;

    let t4 = Instant::now();
    assert!(s.save());
    let save_ms = t4.elapsed().as_secs_f64() * 1000.0;
    let saved = fs::read(&path).unwrap();
    println!(
        "10MB timings (debug build): bytes={} lines={lines}",
        bytes.len()
    );
    println!("  open+first frame: {open_ms:.1} ms");
    println!("  go to last line + frame: {jump_ms:.1} ms");
    println!(
        "  type {keys} chars at end, frame each: {type_ms:.1} ms total ({:.2} ms per key)",
        type_ms / keys as f64
    );
    println!("  scroll 50 wheel steps + frame: {scroll_ms:.1} ms");
    println!(
        "  save 10 MB: {save_ms:.1} ms, bytes on disk {}",
        saved.len()
    );
    assert!(
        saved.len() == bytes.len() + keys as usize,
        "saved size {}",
        saved.len()
    );
    assert!(open_ms < 1000.0, "open took {open_ms:.1} ms");
    let _ = fs::remove_file(&path);
}

#[test]
fn ten_megabyte_save_round_trips_unchanged() {
    let text = big_text();
    let path = temp_path("big_rt");
    fs::write(&path, text.as_bytes()).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mut s = Session::from_bytes(Some(path.clone()), &bytes, 80, 24).unwrap();
    assert!(s.save());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let _ = fs::remove_file(&path);
}

#[test]
fn unix_bytes_decode_to_keys_and_mouse() {
    use calliope::input::decode_bytes;
    assert_eq!(decode_bytes(b"\x1b[A"), vec![Ev::Key(Key::Up)]);
    assert_eq!(decode_bytes(b"\x1b[1;2D"), vec![Ev::Shift(Key::Left)]);
    assert_eq!(decode_bytes(b"\x1b[1;5C"), vec![Ev::Key(Key::Right)]);
    assert_eq!(decode_bytes(b"\x1bOH"), vec![Ev::Key(Key::Home)]);
    assert_eq!(decode_bytes(b"\x1b[3~"), vec![Ev::Key(Key::Delete)]);
    assert_eq!(decode_bytes(b"\x13"), vec![Ev::Key(Key::Ctrl('s'))]);
    assert_eq!(decode_bytes(b"\r"), vec![Ev::Key(Key::Enter)]);
    assert_eq!(decode_bytes(b"\x7f"), vec![Ev::Key(Key::Backspace)]);
    assert_eq!(decode_bytes(b"\t"), vec![Ev::Key(Key::Tab)]);
    assert_eq!(decode_bytes(b"\x1b"), vec![Ev::Key(Key::Esc)]);
    assert_eq!(decode_bytes("é".as_bytes()), vec![Ev::Key(Key::Char('é'))]);
    assert_eq!(
        decode_bytes(b"\x1b[<0;10;5M"),
        vec![Ev::Mouse(Mouse::Down(4, 9))]
    );
    assert_eq!(decode_bytes(b"\x1b[<0;10;5m"), vec![Ev::Mouse(Mouse::Up)]);
    assert_eq!(
        decode_bytes(b"\x1b[<64;1;1M"),
        vec![Ev::Mouse(Mouse::Wheel(-3))]
    );
    assert_eq!(
        decode_bytes(b"\x1b[<65;1;1M"),
        vec![Ev::Mouse(Mouse::Wheel(3))]
    );
}

#[test]
fn windows_records_map_to_keys_and_mouse() {
    use calliope::input::{key_from_windows, mouse_from_windows};
    assert_eq!(key_from_windows(0x25, 0, 0x10), Some(Ev::Shift(Key::Left)));
    assert_eq!(key_from_windows(0x25, 0, 0), Some(Ev::Key(Key::Left)));
    assert_eq!(
        key_from_windows(0x53, 0x13, 0x8),
        Some(Ev::Key(Key::Ctrl('s')))
    );
    assert_eq!(key_from_windows(0x0d, 0x0d, 0), Some(Ev::Key(Key::Enter)));
    assert_eq!(
        key_from_windows(0x41, 0x61, 0),
        Some(Ev::Key(Key::Char('a')))
    );
    assert_eq!(key_from_windows(0x10, 0, 0x10), None);
    assert_eq!(key_from_windows(0x41, 0x61, 0x2), None);
    assert_eq!(
        mouse_from_windows(9, 2, 1, 0, 0),
        Some(Ev::Mouse(Mouse::Down(2, 9)))
    );
    assert_eq!(
        mouse_from_windows(9, 2, 0, 0, 1),
        Some(Ev::Mouse(Mouse::Up))
    );
    assert_eq!(
        mouse_from_windows(0, 0, 0xff88_0000, 4, 0),
        Some(Ev::Mouse(Mouse::Wheel(3)))
    );
}
