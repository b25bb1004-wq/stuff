//! Integration tests for calliope::App, highlighting, keys, and rendering.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use assay::{forall, Rng};
use calliope::{ansi, decode_keys, highlight, parse_keys, App, Key, Mode, Style};

fn run_keys(app: &mut App, s: &str) {
    for k in parse_keys(s) {
        app.handle(k);
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("slate_test_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn motions_and_counts() {
    let mut app = App::with_text("a.txt", "  hello world\nshort\n  foo+bar\n\nend", 40, 10);
    assert_eq!(app.cursor(), (0, 0));

    run_keys(&mut app, "l");
    assert_eq!(app.cursor(), (0, 1));
    run_keys(&mut app, "3l");
    assert_eq!(app.cursor(), (0, 4));
    run_keys(&mut app, "2h");
    assert_eq!(app.cursor(), (0, 2));

    // Sticky column across shorter line
    run_keys(&mut app, "$");
    assert_eq!(app.cursor(), (0, 12));
    run_keys(&mut app, "j");
    assert_eq!(app.cursor(), (1, 4));
    run_keys(&mut app, "k");
    assert_eq!(app.cursor(), (0, 12));

    // 0 and Home/End
    run_keys(&mut app, "0");
    assert_eq!(app.cursor(), (0, 0));
    app.handle(Key::End);
    assert_eq!(app.cursor(), (0, 12));
    app.handle(Key::Home);
    assert_eq!(app.cursor(), (0, 0));

    // Word motions w / b across punctuation and empty lines
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (0, 2)); // 'h' in hello
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (0, 8)); // 'w' in world
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (1, 0)); // 's' in short
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (2, 2)); // 'f' in foo
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (2, 5)); // '+'
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (2, 6)); // 'b' in bar
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (3, 0)); // empty line
    run_keys(&mut app, "w");
    assert_eq!(app.cursor(), (4, 0)); // 'e' in end

    run_keys(&mut app, "b");
    assert_eq!(app.cursor(), (3, 0)); // empty line
    run_keys(&mut app, "b");
    assert_eq!(app.cursor(), (2, 6)); // 'b' in bar
    run_keys(&mut app, "2b");
    assert_eq!(app.cursor(), (2, 2)); // 'f' in foo

    // gg, G, {n}G to first non-blank
    run_keys(&mut app, "G");
    assert_eq!(app.cursor(), (4, 0));
    run_keys(&mut app, "gg");
    assert_eq!(app.cursor(), (0, 2));
    run_keys(&mut app, "3G");
    assert_eq!(app.cursor(), (2, 2));

    // PageDown / PageUp (height=10 -> text_rows=9 -> step=8)
    let mut big = App::with_text(
        "big.txt",
        &(0..30)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        20,
        6,
    );
    run_keys(&mut big, "<PgDn>");
    assert_eq!(big.cursor(), (4, 0)); // text_rows=5, step=4
    run_keys(&mut big, "<PgDn>");
    assert_eq!(big.cursor(), (8, 0));
    run_keys(&mut big, "<PgUp>");
    assert_eq!(big.cursor(), (4, 0));
}

#[test]
fn editing_registers_and_undo_redo() {
    let mut app = App::with_text("f.txt", "  alpha\nbeta\ngamma", 40, 10);

    // x and p/P
    run_keys(&mut app, "2x");
    assert_eq!(app.text(), "alpha\nbeta\ngamma");
    assert_eq!(app.cursor(), (0, 0));
    run_keys(&mut app, "p");
    assert_eq!(app.text(), "a  lpha\nbeta\ngamma");
    assert_eq!(app.cursor(), (0, 2));
    run_keys(&mut app, "u");
    assert_eq!(app.text(), "alpha\nbeta\ngamma");
    assert_eq!(app.cursor(), (0, 1));
    run_keys(&mut app, "u");
    assert_eq!(app.text(), "  alpha\nbeta\ngamma");
    assert_eq!(app.cursor(), (0, 0));
    run_keys(&mut app, "<C-r>");
    assert_eq!(app.text(), "alpha\nbeta\ngamma");

    // dd and yy and linewise p/P
    run_keys(&mut app, "dd");
    assert_eq!(app.text(), "beta\ngamma");
    run_keys(&mut app, "p");
    assert_eq!(app.text(), "beta\nalpha\ngamma");
    assert_eq!(app.cursor(), (1, 0));
    run_keys(&mut app, "yy");
    run_keys(&mut app, "P");
    assert_eq!(app.text(), "beta\nalpha\nalpha\ngamma");

    // dd on only line leaves one empty line
    let mut one = App::with_text("o.txt", "only", 20, 5);
    run_keys(&mut one, "dd");
    assert_eq!(one.text(), "");
    assert_eq!(one.cursor(), (0, 0));
}

#[test]
fn insert_mode_and_session_undo() {
    let mut app = App::with_text("i.txt", "  hello", 40, 10);
    run_keys(&mut app, "Ifoo <Esc>");
    assert_eq!(app.text(), "  foo hello");
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.cursor(), (0, 5)); // on ' ', one left of 'h'

    // One undo reverts entire Insert session
    run_keys(&mut app, "u");
    assert_eq!(app.text(), "  hello");
    assert!(!app.modified());

    // Redo restores it and puts cursor at start of changed text
    run_keys(&mut app, "<C-r>");
    assert_eq!(app.text(), "  foo hello");
    assert_eq!(app.cursor(), (0, 2));

    // No-op insert session adds no undo step
    run_keys(&mut app, "i<Esc>");
    run_keys(&mut app, "u");
    assert_eq!(app.text(), "  hello");

    // Enter, Tab, Backspace, Delete, a, A, o, O
    let mut app2 = App::new(40, 10);
    run_keys(&mut app2, "iab<Tab>c<CR>de<BS><Left><Del><Esc>");
    assert_eq!(app2.text(), "ab\tc\n");
    run_keys(&mut app2, "ox<Esc>");
    assert_eq!(app2.text(), "ab\tc\n\nx");
    run_keys(&mut app2, "Oy<Esc>");
    assert_eq!(app2.text(), "ab\tc\n\ny\nx");
}

#[test]
fn search_wrap_and_substitutions() {
    let mut app = App::with_text("s.txt", "foo bar foo\nbaz foo/bar\n", 40, 10);
    run_keys(&mut app, "/foo<CR>");
    assert_eq!(app.cursor(), (0, 8));
    run_keys(&mut app, "n");
    assert_eq!(app.cursor(), (1, 4));
    run_keys(&mut app, "n");
    assert_eq!(app.cursor(), (0, 0)); // wrapped
    run_keys(&mut app, "N");
    assert_eq!(app.cursor(), (1, 4)); // wrapped backward

    // No match and bad regex messages
    run_keys(&mut app, "/zzz<CR>");
    let scr = app.render();
    assert!(scr.rows.last().unwrap().starts_with("no match: zzz"));
    // One-shot message clears on next render
    let scr2 = app.render();
    assert!(scr2.rows.last().unwrap().starts_with("NORMAL s.txt"));

    run_keys(&mut app, "/(<CR>");
    let scr3 = app.render();
    assert!(scr3.rows.last().unwrap().starts_with("bad regex"));

    // Substitution on current line (cursor is on line 1)
    run_keys(&mut app, ":s/foo\\/bar/qux/<CR>");
    assert_eq!(app.text(), "foo bar foo\nbaz qux\n");
    let scr4 = app.render();
    assert!(scr4.rows.last().unwrap().starts_with("1 substitutions"));

    // Global substitution across all lines
    run_keys(&mut app, ":%s/foo/X/g<CR>");
    assert_eq!(app.text(), "X bar X\nbaz qux\n");
    let scr5 = app.render();
    assert!(scr5.rows.last().unwrap().starts_with("2 substitutions"));

    // Undo %s in one step
    run_keys(&mut app, "u");
    assert_eq!(app.text(), "foo bar foo\nbaz qux\n");
}

#[test]
fn screen_scrolling_tabs_and_status() {
    let mut app = App::new(30, 4);
    assert_eq!(app.render().rows[3], "NORMAL [scratch] 1:1          ");
    run_keys(&mut app, "i\txyz<CR>line2<CR>line3<CR>line4<Esc>");
    let scr = app.render();
    assert_eq!(scr.rows.len(), 4);
    assert!(scr.rows.iter().all(|r| r.chars().count() == 30));
    assert!(scr.rows[3].starts_with("NORMAL [scratch] [+] 4:5"));

    // Horizontal scroll on long line with tab
    let mut wide = App::with_text("w.txt", "\t0123456789abcdef", 10, 3);
    run_keys(&mut wide, "$");
    let s = wide.render();
    assert_eq!(s.rows[0].chars().count(), 10);
    assert!(s.rows[0].ends_with('f'));
    assert_eq!(s.cursor, (0, 9));

    let out = ansi(&s);
    assert!(out.starts_with("\x1b[H"));
    assert!(out.contains("\x1b[0m\r\n"));
    assert!(out.ends_with("\x1b[1;10H"));
}

#[test]
fn file_round_trip_and_commands() {
    let dir = temp_dir("files");
    let missing = dir.join("new.txt");
    let mut app = App::open(&missing, 30, 5).unwrap();
    assert_eq!(app.text(), "");
    assert!(!app.modified());

    run_keys(&mut app, "ihello<Esc>");
    assert!(app.modified());

    // :q refused when modified
    run_keys(&mut app, ":q<CR>");
    assert!(!app.quit_requested());
    assert!(app.render().rows[4].starts_with("unsaved changes"));

    // :e refused when modified
    let other = dir.join("other.txt");
    std::fs::write(&other, "other content").unwrap();
    run_keys(&mut app, &format!(":e {}<CR>", other.display()));
    assert_eq!(app.text(), "hello");

    // :w path writes without renaming
    let copy = dir.join("copy.txt");
    run_keys(&mut app, &format!(":w {}<CR>", copy.display()));
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "hello");
    assert!(!app.modified());

    // :e other loads other
    run_keys(&mut app, &format!(":e {}<CR>", other.display()));
    assert_eq!(app.text(), "other content");
    assert_eq!(app.cursor(), (0, 0));

    // :wq writes and requests quit
    run_keys(&mut app, "A!<Esc>:wq<CR>");
    assert!(app.quit_requested());
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "other content!");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mica_highlighting_and_decode_keys() {
    let src = "fn add(a: Int) -> Int {\n  // sum\n  let s = \"hi\"\n  a + 1\n}\n";
    let styles = highlight("math.logos", src);
    assert_eq!(styles.len(), src.len());
    assert_eq!(styles[0..2], [Style::Keyword; 2]);
    let int_pos = src.find("Int").unwrap();
    assert_eq!(styles[int_pos..int_pos + 3], [Style::Type; 3]);
    let c_pos = src.find("// sum").unwrap();
    assert_eq!(styles[c_pos..c_pos + 6], [Style::Comment; 6]);
    let str_pos = src.find("\"hi\"").unwrap();
    assert_eq!(styles[str_pos..str_pos + 4], [Style::String; 4]);
    let num_pos = src.find('1').unwrap();
    assert_eq!(styles[num_pos], Style::Number);

    let decoded = decode_keys(b"x\x1b[A\x1b[3~\x12\r\x1b");
    assert_eq!(
        decoded,
        vec![
            Key::Char('x'),
            Key::Up,
            Key::Delete,
            Key::Ctrl('r'),
            Key::Enter,
            Key::Esc,
        ]
    );
}

#[test]
fn prop_random_keys_never_panic_and_cursor_valid() {
    let palette = [
        Key::Char('h'),
        Key::Char('j'),
        Key::Char('k'),
        Key::Char('l'),
        Key::Char('w'),
        Key::Char('b'),
        Key::Char('0'),
        Key::Char('$'),
        Key::Char('g'),
        Key::Char('G'),
        Key::Char('x'),
        Key::Char('d'),
        Key::Char('y'),
        Key::Char('p'),
        Key::Char('P'),
        Key::Char('i'),
        Key::Char('a'),
        Key::Char('o'),
        Key::Char('O'),
        Key::Char('u'),
        Key::Ctrl('r'),
        Key::Char('/'),
        Key::Char(':'),
        Key::Char('n'),
        Key::Char('N'),
        Key::Char('z'),
        Key::Enter,
        Key::Esc,
        Key::Backspace,
        Key::Delete,
        Key::Tab,
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
    ];

    forall(100, 0x51a7e, |rng: &mut Rng| {
        let mut app = App::with_text("p.logos", "fn f() {\n  let x = 1\n}\n", 20, 6);
        let len = rng.range(1, 81);
        for _ in 0..len {
            let idx = rng.range(0, palette.len() as u64) as usize;
            let k = palette[idx];
            app.handle(k);
            let _ = app.render();
            let txt = app.text();
            let lines: Vec<&str> = txt.split('\n').collect();
            let (cl, cc) = app.cursor();
            assert!(cl < lines.len());
            let line_chars = lines[cl].chars().count();
            if app.mode() == Mode::Normal {
                assert!(cc <= line_chars.saturating_sub(1));
            } else {
                assert!(cc <= line_chars);
            }
        }
    });
}
