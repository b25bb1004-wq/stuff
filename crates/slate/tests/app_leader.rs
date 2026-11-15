//! Leader check for T55: the slate modal editor app, written from briefs/T55.md.
#![allow(clippy::unwrap_used, clippy::type_complexity)]
use assay::Rng;
use slate::{ansi, decode_keys, highlight, parse_keys, App, Key, Mode, Style};

const W: usize = 40;
const H: usize = 6;

fn run(text: &str, keys: &str) -> App {
    let mut app = App::with_text("t.txt", text, W, H);
    for k in parse_keys(keys) {
        app.handle(k);
    }
    app
}

fn status(app: &mut App) -> String {
    let scr = app.render();
    scr.rows.last().unwrap().trim_end().to_string()
}

fn pad(s: &str, w: usize) -> String {
    let mut out: String = s.chars().take(w).collect();
    while out.chars().count() < w {
        out.push(' ');
    }
    out
}

#[test]
fn edit_table() {
    // (text, keys, expected text, expected cursor or None)
    let cases: &[(&str, &str, &str, Option<(usize, usize)>)] = &[
        ("hello", "x", "ello", Some((0, 0))),
        ("hello", "3x", "lo", Some((0, 0))),
        ("hello", "$x", "hell", Some((0, 3))),
        ("hello", "3xP", "hello", Some((0, 2))),
        ("a\nb\nc", "jdd", "a\nc", Some((1, 0))),
        ("a\nb\nc", "2dd", "c", Some((0, 0))),
        ("a\nb\nc", "2ddp", "c\na\nb", Some((1, 0))),
        ("a\nb\nc", "Gdd", "a\nb", Some((1, 0))),
        ("only", "dd", "", Some((0, 0))),
        ("a\nb", "yyp", "a\na\nb", Some((1, 0))),
        ("a\nb", "yyjP", "a\na\nb", Some((1, 0))),
        ("  a\nb", "yyjp", "  a\nb\n  a", Some((2, 2))),
        ("abc", "xp", "bac", Some((0, 1))),
        ("abc", "ifoo<Esc>", "fooabc", Some((0, 2))),
        ("abc", "afoo<Esc>", "afoobc", Some((0, 3))),
        ("  abc", "$Ix<Esc>", "  xabc", Some((0, 2))),
        ("abc", "Ax<Esc>", "abcx", Some((0, 3))),
        ("abc", "A<Left>x<Esc>", "abxc", Some((0, 2))),
        ("a\nb", "ox<Esc>", "a\nx\nb", Some((1, 0))),
        ("a\nb", "jOx<Esc>", "a\nx\nb", Some((1, 0))),
        ("abc", "ix<Esc>u", "abc", Some((0, 0))),
        ("abc", "ix<Esc>u<C-r>", "xabc", Some((0, 0))),
        ("abc", "xxuu", "abc", Some((0, 0))),
        ("ab", "xi<Esc>u", "ab", Some((0, 0))),
        ("a\nb\nc", "jddu", "a\nb\nc", Some((1, 0))),
        ("a", "ox<Esc>u", "a", None),
        ("abcd", "llifoo<CR>bar<Esc>", "abfoo\nbarcd", Some((1, 2))),
        ("ab\ncd", "ji<BS><Esc>", "abcd", Some((0, 1))),
        ("ab\ncd", "A<Del><Esc>", "abcd", Some((0, 1))),
        ("ab", "i<BS><Esc>", "ab", Some((0, 0))),
        ("x", "i<Tab><Esc>", "\tx", Some((0, 0))),
        ("aXbXc\nX", ":s/X/-/<CR>", "a-bXc\nX", None),
        ("aXbXc\nX", ":s/X/-/g<CR>", "a-b-c\nX", None),
        ("aXbXc\nX", ":%s/X/-/<CR>", "a-bXc\n-", None),
        ("aXbXc\nX", ":%s/X/-/g<CR>", "a-b-c\n-", None),
        ("a/b", ":s/\\//+/<CR>", "a+b", None),
        ("ab", ":s/b/\\//<CR>", "a/", None),
        ("aXb", ":s/X/-/<CR>u", "aXb", None),
        ("aXbXc", ":s/X/-/<Esc>", "aXbXc", None),
    ];
    for &(text, keys, want, cur) in cases {
        let app = run(text, keys);
        assert_eq!(app.text(), want, "text for {text:?} + {keys:?}");
        if let Some(c) = cur {
            assert_eq!(app.cursor(), c, "cursor for {text:?} + {keys:?}");
        }
        assert_eq!(app.mode(), Mode::Normal, "mode for {text:?} + {keys:?}");
    }
}

#[test]
fn motion_table() {
    let ten = "0\n1\n2\n3\n4\n5\n6\n7\n8\n9";
    let cases: &[(&str, &str, (usize, usize))] = &[
        ("abc def ghi", "w", (0, 4)),
        ("abc def ghi", "2w", (0, 8)),
        ("abc def ghi", "$b", (0, 8)),
        ("abc def ghi", "$bb", (0, 4)),
        ("foo.bar baz", "w", (0, 3)),
        ("foo.bar baz", "ww", (0, 4)),
        ("a\n\nb", "w", (1, 0)),
        ("a\n\nb", "ww", (2, 0)),
        ("a\n\nb", "Gb", (1, 0)),
        ("ab\n  cd", "lw", (1, 2)),
        ("x\n  y\nz", "G", (2, 0)),
        ("x\n  y\nz", "2G", (1, 2)),
        ("  a\nb", "jgg", (0, 2)),
        ("a\n  b\nc", ":2<CR>", (1, 2)),
        ("abcdef\nab\nabcdef", "$jj", (2, 5)),
        ("abcdef\nab", "$jk", (0, 5)),
        ("abcdef\nab\nabcdef", "lllljj", (2, 4)),
        ("abc", "$0", (0, 0)),
        ("abc", "$<Home>", (0, 0)),
        ("abc", "<End>", (0, 2)),
        ("abc", "hhh", (0, 0)),
        ("abc", "lllll", (0, 2)),
        ("abc", "<Right><Right><Left>", (0, 1)),
        ("a\nb", "<Down>", (1, 0)),
        ("a\nb", "<Down><Up>", (0, 0)),
        ("a\nb", "kkk", (0, 0)),
        ("a\nb", "jjj", (1, 0)),
        ("abcdef", "10l", (0, 5)),
        ("a\nb\nc\nd", "2j", (2, 0)),
        ("one two one", "/one<CR>", (0, 8)),
        ("one two one", "/one<CR>n", (0, 0)),
        ("one two one", "/one<CR>N", (0, 0)),
        ("one two one", "/one<CR>nN", (0, 8)),
        ("one two\none", "/o<CR>", (0, 6)),
        ("one two\none", "/o<CR>n", (1, 0)),
        ("one", "/zz<CR>", (0, 0)),
        ("one", "/o<BS><BS>", (0, 0)),
        (ten, "<PgDn>", (4, 0)),
        (ten, "<PgDn><PgDn><PgDn>", (9, 0)),
        (ten, "G<PgUp>", (5, 0)),
        (ten, "<PgDn><PgUp>", (0, 0)),
        ("abc", "Z", (0, 0)),
        ("abc", "l<Esc>", (0, 1)),
    ];
    for &(text, keys, want) in cases {
        let app = run(text, keys);
        assert_eq!(app.cursor(), want, "cursor for {text:?} + {keys:?}");
        assert_eq!(app.text(), text, "text changed by {text:?} + {keys:?}");
        assert_eq!(app.mode(), Mode::Normal, "mode for {text:?} + {keys:?}");
    }
}

#[test]
fn modes_and_flags() {
    let mut app = run("abc", "");
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.modified());
    app.handle(Key::Char('i'));
    assert_eq!(app.mode(), Mode::Insert);
    app.handle(Key::Esc);
    app.handle(Key::Char('/'));
    assert_eq!(app.mode(), Mode::Search);
    app.handle(Key::Esc);
    app.handle(Key::Char(':'));
    assert_eq!(app.mode(), Mode::Command);
    app.handle(Key::Backspace);
    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.modified());
    app.handle(Key::Char('x'));
    assert!(app.modified());
    assert!(!app.quit_requested());

    let q = run("abc", ":q<CR>");
    assert!(q.quit_requested());
    let mut q = run("abc", "x:q<CR>");
    assert!(!q.quit_requested());
    assert_eq!(status(&mut q), "unsaved changes (use :q! to quit)");
    q.handle(Key::Char(':'));
    q.handle(Key::Char('q'));
    q.handle(Key::Char('!'));
    q.handle(Key::Enter);
    assert!(q.quit_requested());

    let mut m = run("aXbXc\nX", ":%s/X/-/g<CR>");
    assert_eq!(status(&mut m), "3 substitutions");
    let mut m = run("abc", ":s/Q/-/<CR>");
    assert_eq!(status(&mut m), "no match: Q");
    assert_eq!(m.text(), "abc");
    let mut m = run("abc", ":s/(/-/<CR>");
    assert_eq!(status(&mut m), "bad regex");
    let mut m = run("abc", "/zz<CR>");
    assert_eq!(status(&mut m), "no match: zz");
    let mut m = run("abc", "/(<CR>");
    assert_eq!(status(&mut m), "bad regex");
    let mut m = run("abc", ":foo bar<CR>");
    assert_eq!(status(&mut m), "unknown command: foo bar");
    // A message shows for one render only.
    assert_eq!(status(&mut m), "NORMAL t.txt 1:1");
}

#[test]
fn screens() {
    let mut app = App::with_text("t.txt", "hello\tworld\nline2", 20, 4);
    let scr = app.render();
    let want: Vec<String> = ["hello   world", "line2", "", "NORMAL t.txt 1:1"]
        .iter()
        .map(|r| pad(r, 20))
        .collect();
    assert_eq!(scr.rows, want);
    assert_eq!(scr.cursor, (0, 0));
    assert_eq!(scr.styles.len(), 4);
    for (i, row) in scr.styles.iter().enumerate() {
        assert_eq!(row.len(), 20);
        let style = if i == 3 { Style::Status } else { Style::Plain };
        assert!(row.iter().all(|s| *s == style), "row {i} styles {row:?}");
    }
    app.handle(Key::Char('$'));
    assert_eq!(app.cursor(), (0, 10));
    assert_eq!(app.render().cursor, (0, 12));

    let mut app = App::with_text("t.txt", "hello\tworld\nline2", 20, 4);
    app.handle(Key::Char('x'));
    let scr = app.render();
    assert_eq!(scr.rows[0], pad("ello    world", 20));
    assert_eq!(scr.rows[3], pad("NORMAL t.txt [+] 1:1", 20));
    app.handle(Key::Char('j'));
    assert_eq!(app.render().rows[3], pad("NORMAL t.txt [+] 2:1", 20));
    app.handle(Key::Char('A'));
    let scr = app.render();
    assert_eq!(scr.rows[3], pad("INSERT t.txt [+] 2:6", 20));
    assert_eq!(scr.cursor, (1, 5));
    app.handle(Key::Esc);
    app.handle(Key::Char(':'));
    let scr = app.render();
    assert_eq!(scr.rows[3], pad(":", 20));
    assert_eq!(scr.cursor, (3, 1));
    app.handle(Key::Char('w'));
    let scr = app.render();
    assert_eq!(scr.rows[3], pad(":w", 20));
    assert_eq!(scr.cursor, (3, 2));
    app.handle(Key::Esc);
    app.handle(Key::Char('/'));
    app.handle(Key::Char('o'));
    app.handle(Key::Char('n'));
    let scr = app.render();
    assert_eq!(scr.rows[3], pad("/on", 20));
    assert_eq!(scr.cursor, (3, 3));

    let mut app = App::new(10, 3);
    let scr = app.render();
    assert_eq!(
        scr.rows,
        vec![pad("", 10), pad("", 10), "NORMAL [sc".to_string()]
    );
    assert_eq!(app.text(), "");
    app.resize(30, 2);
    let scr = app.render();
    assert_eq!(scr.rows, vec![pad("", 30), pad("NORMAL [scratch] 1:1", 30)]);

    // Matches of the last search show as Match.
    let mut app = App::with_text("t.txt", "one two one", 20, 3);
    for k in parse_keys("/one<CR>") {
        app.handle(k);
    }
    let scr = app.render();
    for (c, s) in scr.styles[0].iter().enumerate() {
        let want = if c < 3 || (8..11).contains(&c) {
            Style::Match
        } else {
            Style::Plain
        };
        assert_eq!(*s, want, "column {c}");
    }
    assert_eq!(scr.cursor, (0, 8));
}

#[test]
fn scrolling() {
    let text: Vec<String> = (0..100).map(|i| format!("line{i}")).collect();
    let mut app = App::with_text("t.txt", &text.join("\n"), 20, 5);
    let first = |app: &mut App| app.render().rows[0].trim_end().to_string();
    assert_eq!(first(&mut app), "line0");
    app.handle(Key::Char('G'));
    let scr = app.render();
    assert_eq!(scr.rows[0].trim_end(), "line96");
    assert_eq!(scr.rows[3].trim_end(), "line99");
    assert_eq!(scr.cursor, (3, 0));
    // Render between moves: the brief does not say whether scrolling happens per key or at render.
    app.handle(Key::Char('g'));
    app.handle(Key::Char('g'));
    assert_eq!(app.render().rows[0].trim_end(), "line0");
    for k in parse_keys("5j") {
        app.handle(k);
    }
    let scr = app.render();
    assert_eq!(scr.rows[0].trim_end(), "line2");
    assert_eq!(scr.cursor, (3, 0));
    for k in parse_keys("kkk") {
        app.handle(k);
    }
    assert_eq!(first(&mut app), "line2");
    assert_eq!(app.render().cursor, (0, 0));
    app.handle(Key::Char('k'));
    assert_eq!(first(&mut app), "line1");
    assert_eq!(app.render().cursor, (0, 0));

    let long = "0123456789".repeat(5);
    let mut app = App::with_text("t.txt", &long, 10, 3);
    app.handle(Key::Char('$'));
    let scr = app.render();
    assert_eq!(scr.rows[0], "0123456789");
    assert_eq!(scr.cursor, (0, 9));
    for _ in 0..9 {
        app.handle(Key::Char('h'));
    }
    assert_eq!(app.render().cursor, (0, 0));
    app.handle(Key::Char('h'));
    let scr = app.render();
    assert_eq!(scr.rows[0], "9012345678");
    assert_eq!(scr.cursor, (0, 0));
    app.handle(Key::Char('0'));
    let scr = app.render();
    assert_eq!(scr.rows[0], "0123456789");
    assert_eq!(scr.cursor, (0, 0));
}

fn tmp_dir() -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("slate_leader_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn keys(app: &mut App, s: &str) {
    for k in parse_keys(s) {
        app.handle(k);
    }
}

#[test]
fn files() {
    let dir = tmp_dir();
    let new = dir.join("new.txt");
    let _ = std::fs::remove_file(&new);
    let mut app = App::open(&new, 200, 4).unwrap();
    assert_eq!(app.text(), "");
    assert!(!app.modified());
    let st = status(&mut app);
    assert!(
        st.starts_with("NORMAL ") && st.contains("new.txt") && st.ends_with(" 1:1"),
        "{st}"
    );
    keys(&mut app, "ihi<Esc>");
    assert!(app.modified());
    keys(&mut app, ":w<CR>");
    assert_eq!(status(&mut app), "wrote 2 bytes");
    assert_eq!(std::fs::read(&new).unwrap(), b"hi");
    assert!(!app.modified());
    let st = status(&mut app);
    assert!(
        st.starts_with("NORMAL ") && st.ends_with("new.txt 1:2"),
        "{st}"
    );

    // Byte-exact round trip, including tabs and the trailing newline.
    let rt = dir.join("rt.txt");
    std::fs::write(&rt, b"x\n\ty\n").unwrap();
    let mut app = App::open(&rt, 200, 4).unwrap();
    assert_eq!(app.text(), "x\n\ty\n");
    keys(&mut app, ":w<CR>");
    assert_eq!(status(&mut app), "wrote 5 bytes");
    assert_eq!(std::fs::read(&rt).unwrap(), b"x\n\ty\n");

    // :w path writes there and keeps the name.
    let other = dir.join("other.txt");
    let _ = std::fs::remove_file(&other);
    keys(&mut app, "x");
    keys(&mut app, &format!(":w {}<CR>", other.display()));
    assert_eq!(status(&mut app), "wrote 4 bytes");
    assert_eq!(std::fs::read(&other).unwrap(), b"\n\ty\n");
    assert_eq!(std::fs::read(&rt).unwrap(), b"x\n\ty\n");
    let st = status(&mut app);
    assert!(st.contains("rt.txt") && !st.contains("other.txt"), "{st}");

    // Writing to a directory fails with the path as typed.
    let d = dir.display().to_string();
    keys(&mut app, &format!(":w {d}<CR>"));
    assert_eq!(status(&mut app), format!("cannot write: {d}"));

    // :e refuses while modified, then loads.
    let mut app = App::open(&rt, 200, 4).unwrap();
    keys(&mut app, "x");
    keys(&mut app, &format!(":e {}<CR>", new.display()));
    assert_eq!(status(&mut app), "unsaved changes (use :q! to quit)");
    assert_eq!(app.text(), "\n\ty\n");
    keys(&mut app, "u");
    assert_eq!(app.text(), "x\n\ty\n");
    let mut app = App::open(&rt, 200, 4).unwrap();
    keys(&mut app, &format!(":e {}<CR>", new.display()));
    assert_eq!(app.text(), "hi");
    assert_eq!(app.cursor(), (0, 0));
    let st = status(&mut app);
    assert!(st.contains("new.txt") && !st.contains("rt.txt"), "{st}");

    // :wq writes and quits.
    keys(&mut app, "Ax<Esc>:wq<CR>");
    assert!(app.quit_requested());
    assert_eq!(std::fs::read(&new).unwrap(), b"hix");
    let _ = std::fs::remove_dir_all(&dir);
}

fn mark(exp: &mut [Style], src: &str, needle: &str, nth: usize, style: Style) {
    let at = src.match_indices(needle).nth(nth).unwrap().0;
    for s in &mut exp[at..at + needle.len()] {
        *s = style;
    }
}

#[test]
fn highlighting() {
    let src = "fn main() {\n  let x = 42 // hi\n  let p: Point = \"s\"\n}\n";
    assert_eq!(highlight("a.txt", src), vec![Style::Plain; src.len()]);
    assert_eq!(highlight("mica", src), vec![Style::Plain; src.len()]);
    let mut exp = vec![Style::Plain; src.len()];
    mark(&mut exp, src, "fn", 0, Style::Keyword);
    mark(&mut exp, src, "let", 0, Style::Keyword);
    mark(&mut exp, src, "let", 1, Style::Keyword);
    mark(&mut exp, src, "42", 0, Style::Number);
    mark(&mut exp, src, "// hi", 0, Style::Comment);
    mark(&mut exp, src, "Point", 0, Style::Type);
    mark(&mut exp, src, "\"s\"", 0, Style::String);
    assert_eq!(highlight("a.mica", src), exp);

    let bad = "let s = \"abc\nfn";
    let mut exp = vec![Style::Plain; bad.len()];
    mark(&mut exp, bad, "let", 0, Style::Keyword);
    mark(&mut exp, bad, "\"abc", 0, Style::Error);
    assert_eq!(highlight("b.mica", bad), exp);

    let mut app = App::with_text("m.mica", "let x = 1", 20, 3);
    let scr = app.render();
    let row = &scr.styles[0];
    assert!(row[..3].iter().all(|s| *s == Style::Keyword), "{row:?}");
    assert_eq!(row[3..8], [Style::Plain; 5]);
    assert_eq!(row[8], Style::Number);
}

#[test]
fn key_notation_and_decoding() {
    use Key::*;
    assert_eq!(
        parse_keys(
            "a<Esc><CR><BS><Del><Tab><Left><Right><Up><Down><Home><End><PgUp><PgDn><C-r><lt>"
        ),
        vec![
            Char('a'),
            Esc,
            Enter,
            Backspace,
            Delete,
            Tab,
            Left,
            Right,
            Up,
            Down,
            Home,
            End,
            PageUp,
            PageDown,
            Ctrl('r'),
            Char('<')
        ]
    );
    assert_eq!(parse_keys("é>"), vec![Char('é'), Char('>')]);
    assert_eq!(
        decode_keys(b"a\x1b[A\x1b[3~\r\x7f\x01\x1b"),
        vec![Char('a'), Up, Delete, Enter, Backspace, Ctrl('a'), Esc]
    );
    assert_eq!(
        decode_keys(b"\x1b[5~\x1b[6~\x1b[H\x1b[F\x1b[B\x1b[C\x1b[D\x09\x08\x12"),
        vec![
            PageUp,
            PageDown,
            Home,
            End,
            Down,
            Right,
            Left,
            Tab,
            Backspace,
            Ctrl('r')
        ]
    );
    assert_eq!(decode_keys("é".as_bytes()), vec![Char('é')]);
    assert_eq!(decode_keys(b"\x1b[9~x"), vec![Char('x')]);
    assert_eq!(decode_keys(b""), vec![]);
}

fn strip_sgr(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            let mut seq = String::new();
            for d in it.by_ref() {
                seq.push(d);
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            assert!(seq.ends_with('m'), "non-SGR sequence inside rows: {seq:?}");
            continue;
        }
        out.push(c);
    }
    out
}

#[test]
fn ansi_shape() {
    let mut app = App::with_text("m.mica", "let x = 1\n\tfn", 16, 4);
    keys(&mut app, "j$");
    let scr = app.render();
    let s = ansi(&scr);
    assert!(s.starts_with("\x1b[H"), "{s:?}");
    let tail = format!("\x1b[{};{}H", scr.cursor.0 + 1, scr.cursor.1 + 1);
    assert!(s.ends_with(&tail), "{s:?}");
    let body = &s[3..s.len() - tail.len()];
    let rows: Vec<&str> = body.split("\r\n").collect();
    assert_eq!(rows.len(), scr.rows.len());
    for (raw, want) in rows.iter().zip(&scr.rows) {
        assert!(raw.ends_with("\x1b[0m"), "{raw:?}");
        assert_eq!(&strip_sgr(raw), want);
    }
}

fn check_shape(app: &mut App, w: usize, h: usize, ctx: &str) {
    let text = app.text();
    let lines: Vec<&str> = text.split('\n').collect();
    let (l, c) = app.cursor();
    assert!(l < lines.len(), "{ctx}: line {l} of {}", lines.len());
    let len = lines[l].chars().count();
    match app.mode() {
        Mode::Normal => assert!(c <= len.saturating_sub(1), "{ctx}: col {c} len {len}"),
        _ => assert!(c <= len, "{ctx}: col {c} len {len}"),
    }
    let scr = app.render();
    assert_eq!(scr.rows.len(), h, "{ctx}");
    assert_eq!(scr.styles.len(), h, "{ctx}");
    for (r, s) in scr.rows.iter().zip(&scr.styles) {
        assert_eq!(r.chars().count(), w, "{ctx}: row {r:?}");
        assert_eq!(s.len(), w, "{ctx}");
    }
    assert!(
        scr.cursor.0 < h && scr.cursor.1 <= w,
        "{ctx}: {:?}",
        scr.cursor
    );
}

fn random_key(r: &mut Rng, prev_digit: bool) -> Key {
    use Key::*;
    const NORMAL: &str = "hjkl0$wbgGxdypPiaIAoOunN/:";
    const TEXT: &str = "ab .*(\t";
    match r.below(20) {
        0..=8 => {
            let c = NORMAL.as_bytes()[r.below(NORMAL.len())] as char;
            Char(c)
        }
        9..=11 => {
            let c = TEXT.as_bytes()[r.below(TEXT.len())] as char;
            Char(c)
        }
        12 if !prev_digit => Char(*r.pick(&['2', '3'])),
        13 => *r.pick(&[Left, Right, Up, Down, Home, End, PageUp, PageDown]),
        14 => *r.pick(&[Backspace, Delete, Tab, Ctrl('r')]),
        15 | 16 => Esc,
        17 => Enter,
        18 => Char('é'),
        _ => Char('q'),
    }
}

#[test]
fn fuzz_never_panics_and_undo_all_restores() {
    for seed in 0..4u64 {
        let mut r = Rng::new(0x55_f022_0000 + seed);
        let orig = "fn main() {\n\tlet x = 42 // hi\n}\nabc def\n\n  last line here";
        let (mut w, mut h) = (12, 5);
        let name = if seed % 2 == 0 { "f.mica" } else { "f.txt" };
        let mut app = App::with_text(name, orig, w, h);
        let mut prev_digit = false;
        for step in 0..3000 {
            if r.below(200) == 0 {
                w = 1 + r.below(30);
                h = 2 + r.below(9);
                app.resize(w, h);
            }
            let mut k = random_key(&mut r, prev_digit);
            if k == Key::Enter && app.mode() == Mode::Command {
                k = Key::Esc;
            }
            // Keep counts at one digit so no command repeats more than 3 times.
            if prev_digit && matches!(k, Key::Char('0'..='9')) {
                k = Key::Char('l');
            }
            prev_digit = matches!(k, Key::Char('1'..='9')) && app.mode() == Mode::Normal;
            app.handle(k);
            assert!(!app.quit_requested(), "seed {seed} step {step}");
            check_shape(
                &mut app,
                w,
                h,
                &format!("seed {seed} step {step} key {k:?}"),
            );
        }
        app.handle(Key::Esc);
        app.handle(Key::Esc);
        assert_eq!(app.mode(), Mode::Normal);
        let last = app.text();
        let settle = |app: &mut App, key: Key| {
            let mut quiet = 0;
            for _ in 0..20_000 {
                let before = app.text();
                app.handle(key);
                if app.text() == before {
                    quiet += 1;
                    if quiet == 100 {
                        return;
                    }
                } else {
                    quiet = 0;
                }
            }
            panic!("history never settled");
        };
        settle(&mut app, Key::Char('u'));
        assert_eq!(app.text(), orig, "seed {seed}: undo-all");
        check_shape(&mut app, w, h, "after undo-all");
        settle(&mut app, Key::Ctrl('r'));
        assert_eq!(app.text(), last, "seed {seed}: redo-all");
        check_shape(&mut app, w, h, "after redo-all");
    }
}
