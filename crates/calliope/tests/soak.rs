//! Soak tests: random and mutated inputs against every calliope parser, decoder and edit path.
//!
//! Budget and seed come from `STRATA_SOAK_MS` (default 300 per target) and `STRATA_SOAK_SEED`
//! (default `0x5EED_7A0`). Run with `-- --nocapture` to see the seed and case counts.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use calliope::{ansi, decode_keys, highlight, parse_keys, App, Buffer, Cursor, Editor, Key, Regex};
use std::time::{Duration, Instant};

const DEFAULT_SEED: u64 = 0x05EE_D7A0;
const DEFAULT_MS: u64 = 300;

fn env_u64(name: &str) -> Option<u64> {
    let raw = std::env::var(name).ok()?.replace('_', "");
    let v = raw.trim();
    match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => v.parse().ok(),
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Prints the failing case seed if the body panics (disarmed with `mem::forget` on success).
struct Report {
    target: &'static str,
    seed: u64,
}

impl Drop for Report {
    fn drop(&mut self) {
        eprintln!(
            "soak: target {} failed, replay case seed {:#018x}",
            self.target, self.seed
        );
    }
}

/// Run `case` on fresh random streams until the per-target budget is spent (at least once).
fn soak(target: &'static str, mut case: impl FnMut(&mut Rng)) {
    let seed = env_u64("STRATA_SOAK_SEED").unwrap_or(DEFAULT_SEED);
    let ms = env_u64("STRATA_SOAK_MS").unwrap_or(DEFAULT_MS);
    let budget = Duration::from_millis(ms);
    let mut master = Rng::new(seed ^ fnv(target));
    let start = Instant::now();
    let mut cases = 0u64;
    loop {
        let case_seed = master.u64();
        let guard = Report {
            target,
            seed: case_seed,
        };
        let t = Instant::now();
        case(&mut Rng::new(case_seed));
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "soak {target}: case {case_seed:#018x} took over 1 s"
        );
        std::mem::forget(guard);
        cases += 1;
        if start.elapsed() >= budget {
            break;
        }
    }
    println!("soak {target}: seed {seed:#x} budget {ms} ms cases {cases}");
}

// ---- generators -----------------------------------------------------------------------------

const PALETTE: &[char] = &[
    'a', 'b', 'z', 'A', ' ', '\n', '\t', '\r', '<', '>', '/', '\\', '"', '\'', '#', '(', ')', '[',
    ']', '{', '}', '*', '+', '?', '|', '.', '^', '$', '-', '_', '0', '9', 'é', '€', '🦀', '\0',
    '\u{1b}', '\u{7f}',
];

const CTRL_LETTERS: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'j', 'k', 'l', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v',
    'w', 'x', 'y', 'z',
];

fn rand_char(r: &mut Rng) -> char {
    if r.chance(3, 4) {
        *r.pick(PALETTE)
    } else {
        char::from_u32(r.range(0, 0x11_0000) as u32).unwrap_or('?')
    }
}

fn rand_text(r: &mut Rng, max_chars: usize) -> String {
    let n = r.below(max_chars + 1);
    (0..n).map(|_| rand_char(r)).collect()
}

fn boundaries(s: &str) -> Vec<usize> {
    (0..=s.len()).filter(|&i| s.is_char_boundary(i)).collect()
}

/// A printable key: ASCII 0x20..=0x7E or a non-control non-ASCII scalar.
fn printable(r: &mut Rng) -> char {
    if r.chance(1, 2) {
        char::from(r.range(0x20, 0x7F) as u8)
    } else {
        char::from_u32(r.range(0x80, 0x11_0000) as u32)
            .filter(|c| !c.is_control())
            .unwrap_or('é')
    }
}

/// A key whose notation and byte encodings both round-trip (no Esc, no Ctrl h/i/m).
fn round_trip_key(r: &mut Rng) -> Key {
    match r.below(12) {
        0 => Key::Enter,
        1 => Key::Tab,
        2 => Key::Backspace,
        3 => Key::Delete,
        4 => *r.pick(&[
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ]),
        5 => Key::Ctrl(*r.pick(CTRL_LETTERS)),
        _ => Key::Char(printable(r)),
    }
}

fn notation(k: Key) -> String {
    match k {
        Key::Char('<') => "<lt>".to_string(),
        Key::Char(c) => c.to_string(),
        Key::Ctrl(c) => format!("<C-{c}>"),
        Key::Enter => "<CR>".to_string(),
        Key::Esc => "<Esc>".to_string(),
        Key::Backspace => "<BS>".to_string(),
        Key::Delete => "<Del>".to_string(),
        Key::Tab => "<Tab>".to_string(),
        Key::Left => "<Left>".to_string(),
        Key::Right => "<Right>".to_string(),
        Key::Up => "<Up>".to_string(),
        Key::Down => "<Down>".to_string(),
        Key::Home => "<Home>".to_string(),
        Key::End => "<End>".to_string(),
        Key::PageUp => "<PgUp>".to_string(),
        Key::PageDown => "<PgDn>".to_string(),
    }
}

/// Terminal bytes for a round-trip key (see `round_trip_key`).
fn bytes_of(k: Key) -> Vec<u8> {
    match k {
        Key::Char(c) => c.to_string().into_bytes(),
        Key::Ctrl(c) => vec![c as u8 - b'a' + 1],
        Key::Enter => vec![13],
        Key::Tab => vec![9],
        Key::Backspace => vec![127],
        Key::Esc => vec![27],
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Up => b"\x1b[A".to_vec(),
        Key::Down => b"\x1b[B".to_vec(),
        Key::Right => b"\x1b[C".to_vec(),
        Key::Left => b"\x1b[D".to_vec(),
    }
}

/// Any key, including controls, Esc and arbitrary characters (for no-panic checks).
fn any_key(r: &mut Rng) -> Key {
    match r.below(10) {
        0..=5 => Key::Char(rand_char(r)),
        6 => Key::Ctrl(rand_char(r)),
        7 => *r.pick(&[Key::Enter, Key::Esc, Key::Backspace, Key::Delete, Key::Tab]),
        _ => *r.pick(&[
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ]),
    }
}

/// Mutate `base` with bit flips, truncation, splices from `other`, and repeated chunks.
fn mutate(r: &mut Rng, base: &[u8], other: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..=r.below(3) {
        match r.below(4) {
            0 if !v.is_empty() => {
                let i = r.below(v.len());
                v[i] ^= 1u8 << r.below(8);
            }
            1 => {
                let n = r.below(v.len() + 1);
                v.truncate(n);
            }
            2 => {
                let a = r.below(other.len() + 1);
                let b = a + r.below(other.len() - a + 1);
                let at = r.below(v.len() + 1);
                v.splice(at..at, other[a..b].iter().copied());
            }
            3 if !v.is_empty() => {
                let a = r.below(v.len());
                let b = a + r.below(v.len() - a + 1);
                let chunk = v[a..b].to_vec();
                let at = r.below(v.len() + 1);
                v.splice(at..at, chunk);
            }
            _ => {}
        }
        v.truncate(4096);
    }
    v
}

fn key_corpus(r: &mut Rng) -> Vec<Key> {
    (0..r.below(20)).map(|_| round_trip_key(r)).collect()
}

/// Valid text the crate itself produces, used as mutation seeds.
fn text_corpus(r: &mut Rng) -> Vec<String> {
    let notation_text: String = key_corpus(r).into_iter().map(notation).collect();
    let mut app = App::with_text("sample.logos", "let x = 1 // note\n", 40, 8);
    vec![
        "fn main() {\n    let x = 0x1F; // note\n}\n".to_string(),
        "a \"quoted\" <tag> é 🦀\n".to_string(),
        notation_text,
        ansi(&app.render()),
    ]
}

/// Half random bytes or characters, half mutations of valid text.
fn text_input(r: &mut Rng) -> String {
    match r.below(4) {
        0 => String::from_utf8_lossy(&r.bytes(256)).into_owned(),
        1 => rand_text(r, 200),
        _ => {
            let corpus = text_corpus(r);
            let base = r.pick(&corpus).clone();
            let other = r.pick(&corpus).clone();
            String::from_utf8_lossy(&mutate(r, base.as_bytes(), other.as_bytes())).into_owned()
        }
    }
}

/// Half random bytes, half mutations of valid terminal input bytes.
fn byte_input(r: &mut Rng) -> Vec<u8> {
    if r.chance(1, 2) {
        return r.bytes(256);
    }
    let base: Vec<u8> = key_corpus(r).into_iter().flat_map(bytes_of).collect();
    let other: Vec<u8> = key_corpus(r).into_iter().flat_map(bytes_of).collect();
    mutate(r, &base, &other)
}

const PATTERN_CORPUS: &[&str] = &[
    "a(b|c)*d",
    "[a-z]+@[0-9]{2,4}",
    "^\\s*#.*$",
    "(?P<x>\\w+)\\b",
    "x*?y+?z??",
    "é.[^\\n]",
    "\\d{1,3}(\\.\\d{1,3}){3}",
];

const PATTERN_TOKENS: &[&str] = &[
    "a", "b", "(", ")", "[", "]", "*", "+", "?", "|", ".", "^", "$", "\\", "{", "}", "-", ",", "0",
    "9", "é", "<", "\n", "d", "w", "s", "(?:", "(?P<n>", "\\b", "*?", "{2}", "\\1", ":",
];

fn pattern_input(r: &mut Rng) -> String {
    match r.below(8) {
        0 => "(".repeat(r.below(300)),
        1..=3 => (0..r.below(12)).map(|_| *r.pick(PATTERN_TOKENS)).collect(),
        _ => {
            let base = *r.pick(PATTERN_CORPUS);
            let other = *r.pick(PATTERN_CORPUS);
            String::from_utf8_lossy(&mutate(r, base.as_bytes(), other.as_bytes())).into_owned()
        }
    }
}

// ---- model helpers ----------------------------------------------------------------------------

/// Undo tree mirrored as snapshots: each node holds the text after its edit.
struct Tree {
    nodes: Vec<Node>,
    cur: usize,
}

struct Node {
    parent: Option<usize>,
    children: Vec<usize>,
    text: String,
}

impl Tree {
    fn new(text: &str) -> Tree {
        Tree {
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                text: text.to_string(),
            }],
            cur: 0,
        }
    }

    fn text(&self) -> &str {
        &self.nodes[self.cur].text
    }

    fn parent(&self) -> Option<usize> {
        self.nodes[self.cur].parent
    }

    fn children(&self) -> Vec<usize> {
        self.nodes[self.cur].children.clone()
    }

    fn commit(&mut self, text: String) {
        let id = self.nodes.len();
        let cur = self.cur;
        self.nodes.push(Node {
            parent: Some(cur),
            children: Vec::new(),
            text,
        });
        self.nodes[cur].children.push(id);
        self.cur = id;
    }
}

fn single_cursor(ed: &Editor) -> Cursor {
    assert_eq!(
        ed.cursors().len(),
        1,
        "single-cursor editor lost its cursor"
    );
    ed.cursors()[0]
}

fn assert_valid_cursor(text: &str, c: Cursor) {
    for p in [c.anchor, c.head] {
        assert!(
            p <= text.len() && text.is_char_boundary(p),
            "cursor {p} invalid for len {}",
            text.len()
        );
    }
}

fn literal_pattern(r: &mut Rng) -> String {
    let alphabet = ['a', 'b', 'é'];
    (0..=r.below(2))
        .map(|_| alphabet[r.below(alphabet.len())])
        .collect()
}

// ---- targets ---------------------------------------------------------------------------------

#[test]
fn soak_editor_single_cursor_model() {
    soak("editor_single_cursor_model", |r| {
        let init = rand_text(r, 60);
        let mut ed = Editor::new(&init);
        let mut t = Tree::new(&init);
        let mut sel = Cursor { anchor: 0, head: 0 };
        for _ in 0..r.below(40) {
            let text = t.text().to_string();
            let (s0, e0) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
            match r.below(8) {
                0 => {
                    let b = boundaries(&text);
                    sel = Cursor {
                        anchor: *r.pick(&b),
                        head: *r.pick(&b),
                    };
                    ed.set_cursors(vec![sel]);
                }
                1 => {
                    let ins = rand_text(r, 4);
                    ed.type_text(&ins);
                    if !(ins.is_empty() && s0 == e0) {
                        t.commit(format!("{}{}{}", &text[..s0], ins, &text[e0..]));
                    }
                    let p = s0 + ins.len();
                    sel = Cursor { anchor: p, head: p };
                }
                2 => {
                    ed.backspace();
                    let (new, caret) = if s0 < e0 {
                        (format!("{}{}", &text[..s0], &text[e0..]), s0)
                    } else {
                        let l = text[..s0].chars().next_back().map_or(0, char::len_utf8);
                        (format!("{}{}", &text[..s0 - l], &text[s0..]), s0 - l)
                    };
                    if !(s0 == 0 && e0 == 0) {
                        t.commit(new);
                    }
                    sel = Cursor {
                        anchor: caret,
                        head: caret,
                    };
                }
                3 => {
                    let parent = t.parent();
                    assert_eq!(ed.undo(), parent.is_some());
                    if let Some(p) = parent {
                        t.cur = p;
                    }
                    sel = single_cursor(&ed);
                }
                4 => {
                    let kids = t.children();
                    assert_eq!(ed.redo(), !kids.is_empty());
                    if let Some(&last) = kids.last() {
                        t.cur = last;
                    }
                    sel = single_cursor(&ed);
                }
                5 => {
                    let kids = t.children();
                    let i = r.below(kids.len() + 2);
                    assert_eq!(ed.redo_branch(i), i < kids.len());
                    if let Some(&k) = kids.get(i) {
                        t.cur = k;
                    }
                    sel = single_cursor(&ed);
                }
                6 => assert_eq!(ed.branches(), t.children().len()),
                _ => {
                    let pat = literal_pattern(r);
                    let got = Regex::new(&pat).unwrap().find_iter(&text);
                    let want: Vec<(usize, usize)> = text
                        .match_indices(&pat)
                        .map(|(i, m)| (i, i + m.len()))
                        .collect();
                    assert_eq!(got, want, "search {pat:?}");
                }
            }
            assert_eq!(ed.buffer.text(), t.text());
            assert_eq!(ed.buffer.len(), t.text().len());
            assert_eq!(ed.buffer.line_count(), t.text().matches('\n').count() + 1);
            assert_eq!(single_cursor(&ed), sel);
            assert_valid_cursor(t.text(), sel);
        }
    });
}

fn check_multi(ed: &Editor) {
    let text = ed.buffer.text();
    assert_eq!(text.len(), ed.buffer.len());
    let mut prev_end: Option<usize> = None;
    for c in ed.cursors() {
        let (s, e) = (c.anchor.min(c.head), c.anchor.max(c.head));
        assert_valid_cursor(&text, Cursor { anchor: s, head: e });
        if let Some(p) = prev_end {
            assert!(p < s, "cursors overlap or touch: {p} vs {s}");
        }
        prev_end = Some(e);
    }
}

#[test]
fn soak_editor_multi_cursor_invariants() {
    soak("editor_multi_cursor_invariants", |r| {
        let mut ed = Editor::new(&rand_text(r, 60));
        for _ in 0..r.below(30) {
            let len = ed.buffer.len();
            match r.below(3) {
                0 => {
                    let n = r.below(5);
                    let cs: Vec<Cursor> = (0..n)
                        .map(|_| Cursor {
                            anchor: r.below(len + 3),
                            head: r.below(len + 3),
                        })
                        .collect();
                    ed.set_cursors(cs);
                    check_multi(&ed);
                }
                1 => mutating_step(&mut ed, |ed| ed.type_text(&rand_text(r, 3))),
                _ => mutating_step(&mut ed, |ed| ed.backspace()),
            }
            check_multi(&ed);
        }
    });
}

/// Apply one edit; if it changed the text, one undo must restore the old text and redo the new.
fn mutating_step(ed: &mut Editor, op: impl FnOnce(&mut Editor)) {
    let before = ed.buffer.text();
    op(ed);
    let after = ed.buffer.text();
    check_multi(ed);
    if before != after {
        assert!(ed.undo(), "undo after a changing edit returned false");
        assert_eq!(ed.buffer.text(), before, "undo of multi-cursor edit");
        check_multi(ed);
        assert!(ed.redo(), "redo after undo returned false");
        assert_eq!(ed.buffer.text(), after, "redo of multi-cursor edit");
        check_multi(ed);
    }
}

#[test]
fn soak_buffer_model() {
    soak("buffer_model", |r| {
        let init = rand_text(r, 120);
        let mut buf = Buffer::new(&init);
        let mut model = init.clone();
        for _ in 0..r.below(40) {
            let b = boundaries(&model);
            match r.below(3) {
                0 => {
                    let at = *r.pick(&b);
                    let s = rand_text(r, 6);
                    buf.insert(at, &s);
                    model.insert_str(at, &s);
                }
                1 => {
                    let (x, y) = (*r.pick(&b), *r.pick(&b));
                    let (a, c) = (x.min(y), x.max(y));
                    buf.delete(a, c);
                    model.replace_range(a..c, "");
                }
                _ => {
                    let (x, y) = (*r.pick(&b), *r.pick(&b));
                    let (a, c) = (x.min(y), x.max(y));
                    assert_eq!(buf.slice(a, c), &model[a..c]);
                }
            }
            assert_eq!(buf.text(), model);
            assert_eq!(buf.len(), model.len());
            let nl = model.matches('\n').count();
            assert_eq!(buf.line_count(), nl + 1);
            let l = r.below(nl + 2);
            let want = if l == 0 {
                Some(0)
            } else {
                model.match_indices('\n').nth(l - 1).map(|(i, _)| i + 1)
            };
            assert_eq!(buf.line_start(l), want, "line_start({l})");
            let at = *r.pick(&boundaries(&model));
            assert_eq!(
                buf.line_of(at),
                model[..at].matches('\n').count(),
                "line_of({at})"
            );
        }
    });
}

#[test]
fn soak_key_round_trip() {
    soak("key_round_trip", |r| {
        let keys: Vec<Key> = (0..r.below(40)).map(|_| round_trip_key(r)).collect();
        let text: String = keys.iter().map(|&k| notation(k)).collect();
        assert_eq!(parse_keys(&text), keys, "notation {text:?}");
        let bytes: Vec<u8> = keys.iter().flat_map(|&k| bytes_of(k)).collect();
        assert_eq!(decode_keys(&bytes), keys, "bytes {bytes:?}");
    });
}

#[test]
fn soak_key_decoders_no_panic() {
    soak("key_decoders_no_panic", |r| {
        let _ = decode_keys(&byte_input(r));
        let _ = parse_keys(&text_input(r));
    });
}

#[test]
fn soak_regex() {
    soak("regex", |r| {
        let pat = pattern_input(r);
        match Regex::new(&pat) {
            Err(e) => assert!(
                e.pos <= pat.len(),
                "error pos {} past pattern {pat:?}",
                e.pos
            ),
            Ok(re) => {
                let text = text_input(r);
                let spans = re.find_iter(&text);
                let mut prev_end = 0usize;
                for &(s, e) in &spans {
                    assert!(
                        s >= prev_end && s <= e && e <= text.len(),
                        "span {s}..{e} for {pat:?}"
                    );
                    assert!(
                        text.is_char_boundary(s) && text.is_char_boundary(e),
                        "span off boundary"
                    );
                    prev_end = e;
                }
                assert_eq!(
                    re.is_match(&text),
                    !spans.is_empty(),
                    "is_match vs find_iter for {pat:?}"
                );
                let s0 = *r.pick(&boundaries(&text));
                if let Some(groups) = re.find_at(&text, s0) {
                    let whole = groups.first().copied().flatten();
                    let (a, b) = whole.expect("match without a whole-match span");
                    assert!(
                        a >= s0 && a <= b && b <= text.len(),
                        "find_at span {a}..{b}"
                    );
                }
            }
        }
    });
}

#[test]
fn soak_regex_literal_model() {
    soak("regex_literal_model", |r| {
        let pat = literal_pattern(r);
        let alphabet = ['a', 'b', 'é', '\n', ' '];
        let text: String = (0..r.below(60))
            .map(|_| alphabet[r.below(alphabet.len())])
            .collect();
        let want: Vec<(usize, usize)> = text
            .match_indices(&pat)
            .map(|(i, m)| (i, i + m.len()))
            .collect();
        assert_eq!(
            Regex::new(&pat).unwrap().find_iter(&text),
            want,
            "pattern {pat:?} text {text:?}"
        );
    });
}

#[test]
fn soak_app_and_loaders() {
    soak("app_and_loaders", |r| {
        let name = *r.pick(&["x.logos", "notes.txt", "", "a.logos"]);
        let text = text_input(r);
        let (w, h) = (r.below(121), r.below(41));
        let mut app = App::with_text(name, &text, w, h);
        for _ in 0..r.below(64) {
            app.handle(any_key(r));
        }
        let _ = ansi(&app.render());
        assert_eq!(highlight(name, &text).len(), text.len());

        let path = std::env::temp_dir().join(format!("calliope-soak-{:016x}.txt", r.u64()));
        std::fs::write(&path, byte_input(r)).unwrap();
        let opened = App::open(&path, w, h);
        let _ = std::fs::remove_file(&path);
        if let Ok(mut app) = opened {
            let _ = app.render();
        }
    });
}
