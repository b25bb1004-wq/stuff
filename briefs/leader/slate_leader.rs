//! Leader check for T54: regex against real Python `re` (re.ASCII) on generated patterns and
//! texts (which patterns are errors, search from 0 and from a later position, finditer spans);
//! buffer and multi-cursor editor against shadow models with a fresh seed; undo/redo tree walk.
//! Group spans are compared only for patterns with no quantifier right after a group (captures
//! of repeated empty groups are engine-specific); the overall match span is always compared.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use slate::{Buffer, Cursor, Editor, Regex};
use std::fmt::Write as _;
use std::process::Command;

const PY: &str = r#"
import sys, re
out = []
for line in open(sys.argv[1], encoding="utf-8").read().split("\n"):
    if not line: continue
    p, t, k = line.split("\t")
    p = bytes.fromhex(p).decode(); t = bytes.fromhex(t).decode() if t != "-" else ""; k = int(k)
    b = lambda i: len(t[:i].encode())
    def groups(m):
        if m is None: return "none"
        return ",".join("x" if m.span(g)[0] < 0 else f"{b(m.span(g)[0])}-{b(m.span(g)[1])}" for g in range(m.re.groups + 1))
    try: rx = re.compile(p, re.ASCII)
    except Exception: out.append("E"); continue
    it = ",".join(f"{b(m.start())}-{b(m.end())}" for m in rx.finditer(t))
    out.append(f"M {groups(rx.search(t))} | {groups(rx.search(t, k))} | {it}")
open(sys.argv[2], "w").write("\n".join(out) + "\n")
"#;

fn atom(r: &mut Rng, depth: u32) -> String {
    match r.below(if depth > 2 { 6 } else { 9 }) {
        0 | 1 => r.pick(&["a", "b", "-", "é", "1", " "]).to_string(),
        2 => r
            .pick(&[
                "\\.", "\\*", "\\(", "\\)", "\\[", "\\|", "\\-", "\\n", "\\t", "\\^",
            ])
            .to_string(),
        3 => ".".to_string(),
        4 => r
            .pick(&[
                "[ab]", "[^a]", "[a-c\\n]", "[]a]", "[\\d_]", "[^\\s]", "[é-]",
            ])
            .to_string(),
        5 => r
            .pick(&["\\d", "\\w", "\\s", "\\D", "\\W", "\\S"])
            .to_string(),
        6 => format!("({})", alt(r, depth + 1)),
        7 => format!("(?:{})", alt(r, depth + 1)),
        _ => r.pick(&["^", "$", "\\b", "\\B"]).to_string(),
    }
}

fn piece(r: &mut Rng, depth: u32) -> String {
    let a = atom(r, depth);
    let anchor = matches!(a.as_str(), "^" | "$" | "\\b" | "\\B");
    if anchor || r.chance(1, 2) {
        return a;
    }
    let q = *r.pick(&[
        "*", "+", "?", "{2}", "{1,}", "{1,3}", "{,2}", "*?", "+?", "??", "{1,2}?", "{0}",
    ]);
    a + q
}

fn alt(r: &mut Rng, depth: u32) -> String {
    let mut branches = Vec::new();
    for _ in 0..1 + r.below(if depth > 1 { 2 } else { 3 }) {
        let mut s = String::new();
        for _ in 0..r.below(4) {
            s.push_str(&piece(r, depth));
        }
        branches.push(s);
    }
    branches.join("|")
}

fn pattern(r: &mut Rng) -> String {
    let mut p = alt(r, 0);
    if r.chance(1, 12) {
        p = match r.below(6) {
            0 => format!("*{p}"),
            1 => format!("({p}"),
            2 => format!("{p})"),
            3 => format!("{p}a{{3,2}}"),
            4 => format!("{p}[a"),
            _ => format!("{p}\\q"),
        };
    }
    p
}

fn text(r: &mut Rng) -> String {
    (0..r.below(12))
        .map(|_| *r.pick(&['a', 'a', 'b', '-', '_', '\n', ' ', 'é', '1']))
        .collect()
}

fn hex(s: &str) -> String {
    if s.is_empty() {
        return "-".into();
    }
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn fmt_groups(g: Option<Vec<Option<(usize, usize)>>>, only_whole: bool) -> String {
    match g {
        None => "none".into(),
        Some(v) => v
            .iter()
            .take(if only_whole { 1 } else { usize::MAX })
            .map(|x| match x {
                Some((s, e)) => format!("{s}-{e}"),
                None => "x".into(),
            })
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn whole_only(line: &str) -> String {
    // Keep just group 0 of each "groups" field of a Python result line.
    let parts: Vec<&str> = line[2..].split(" | ").collect();
    let g0 = |s: &str| s.split(',').next().unwrap().to_string();
    format!("M {} | {} | {}", g0(parts[0]), g0(parts[1]), parts[2])
}

#[test]
fn leader_regex_matches_python_re() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("t54");
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Rng::new(0x54_2026_1115);
    let cases: Vec<(String, String, usize)> = (0..4000)
        .map(|_| {
            let p = pattern(&mut r);
            let t = text(&mut r);
            let n = t.chars().count();
            (p, t, r.below(n + 1))
        })
        .collect();
    let mut input = String::new();
    for (p, t, k) in &cases {
        writeln!(input, "{}\t{}\t{k}", hex(p), hex(t)).unwrap();
    }
    std::fs::write(dir.join("in.txt"), input).unwrap();
    std::fs::write(dir.join("re.py"), PY).unwrap();
    let o = Command::new("python")
        .arg(dir.join("re.py"))
        .arg(dir.join("in.txt"))
        .arg(dir.join("out.txt"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let want = std::fs::read_to_string(dir.join("out.txt")).unwrap();
    let want: Vec<&str> = want.lines().collect();
    assert_eq!(want.len(), cases.len());
    let (mut bad, mut errors, mut matched) = (Vec::new(), 0, 0);
    for (i, (p, t, k)) in cases.iter().enumerate() {
        let got = match Regex::new(p) {
            Err(_) => "E".to_string(),
            Ok(rx) => {
                let only_whole = [")*", ")+", ")?", "){"].iter().any(|q| p.contains(q));
                let kb = t.char_indices().nth(*k).map_or(t.len(), |(b, _)| b);
                let it = rx
                    .find_iter(t)
                    .iter()
                    .map(|(s, e)| format!("{s}-{e}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let s = format!(
                    "M {} | {} | {it}",
                    fmt_groups(rx.find_at(t, 0), only_whole),
                    fmt_groups(rx.find_at(t, kb), only_whole)
                );
                if only_whole && want[i] != "E" {
                    if s != whole_only(want[i]) {
                        bad.push(format!(
                            "{p:?} on {t:?} k={k}\n  python {}\n  slate  {s}",
                            want[i]
                        ));
                    }
                    matched += 1;
                    continue;
                }
                s
            }
        };
        if want[i] == "E" {
            errors += 1;
        } else {
            matched += 1;
        }
        if got != want[i] {
            bad.push(format!(
                "{p:?} on {t:?} k={k}\n  python {}\n  slate  {got}",
                want[i]
            ));
        }
    }
    eprintln!("regex: {matched} compiled, {errors} errors in Python");
    assert!(
        errors > 100 && matched > 3000,
        "generator off: {errors} {matched}"
    );
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        cases.len(),
        bad[..bad.len().min(10)].join("\n")
    );
    let t = std::time::Instant::now();
    assert!(!Regex::new("(a*)*b").unwrap().is_match(&"a".repeat(30_000)));
    assert!(t.elapsed().as_secs_f64() < 1.5, "pathological case slow");
}

fn boundary(s: &str, r: &mut Rng) -> usize {
    let b: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
    b[r.below(b.len())]
}

#[test]
fn leader_buffer_shadow_model() {
    let mut r = Rng::new(0x54_b0ff);
    let bits = ["a", "bc", "\n", "é", "€x", "\n\n", "😀", "line\n"];
    for _ in 0..300 {
        let start: String = (0..r.below(6)).map(|_| *r.pick(&bits)).collect();
        let (mut b, mut s) = (Buffer::new(&start), start.clone());
        for _ in 0..60 {
            if r.chance(1, 2) || s.is_empty() {
                let at = boundary(&s, &mut r);
                let ins: String = (0..1 + r.below(3)).map(|_| *r.pick(&bits)).collect();
                b.insert(at, &ins);
                s.insert_str(at, &ins);
            } else {
                let (x, y) = (boundary(&s, &mut r), boundary(&s, &mut r));
                let (x, y) = (x.min(y), x.max(y));
                b.delete(x, y);
                s.replace_range(x..y, "");
            }
            assert_eq!(b.text(), s);
            assert_eq!((b.len(), b.is_empty()), (s.len(), s.is_empty()));
            let lines = s.matches('\n').count() + 1;
            assert_eq!(b.line_count(), lines);
            let starts: Vec<usize> = std::iter::once(0)
                .chain(s.match_indices('\n').map(|(i, _)| i + 1))
                .collect();
            for (l, &st) in starts.iter().enumerate() {
                assert_eq!(b.line_start(l), Some(st), "{s:?} line {l}");
            }
            assert_eq!(b.line_start(lines), None);
            let at = boundary(&s, &mut r);
            assert_eq!(
                b.line_of(at),
                s[..at].matches('\n').count(),
                "{s:?} at {at}"
            );
            let (x, y) = (boundary(&s, &mut r), boundary(&s, &mut r));
            assert_eq!(b.slice(x.min(y), x.max(y)), s[x.min(y)..x.max(y)]);
        }
    }
    let mut b = Buffer::new("start");
    for i in 0..1000 {
        b.insert(b.len(), if i % 2 == 0 { "x" } else { "é" });
    }
    assert!(
        b.pieces() <= 3,
        "typing at the end made {} pieces",
        b.pieces()
    );
}

/// Reference multi-cursor edit: returns new text and new (empty) cursor positions.
fn ref_edit(s: &str, cur: &[(usize, usize)], typed: Option<&str>) -> (String, Vec<usize>) {
    let mut out = s.to_string();
    let mut pos = Vec::new();
    let mut shift: isize = 0;
    for &(a, h) in cur {
        let (mut x, y) = (a.min(h), a.max(h));
        if typed.is_none() && x == y && x > 0 {
            x = s[..x].char_indices().last().unwrap().0;
        }
        let ins = typed.unwrap_or("");
        let (xs, ys) = ((x as isize + shift) as usize, (y as isize + shift) as usize);
        out.replace_range(xs..ys, ins);
        pos.push(xs + ins.len());
        shift += ins.len() as isize - (y - x) as isize;
    }
    pos.dedup();
    (out, pos)
}

#[test]
fn leader_editor_cursors_and_undo_tree() {
    let mut r = Rng::new(0x54_ed17);
    for _ in 0..300 {
        let start: String = (0..r.below(30))
            .map(|_| *r.pick(&['a', 'b', ' ', '\n', 'é']))
            .collect();
        let mut e = Editor::new(&start);
        let mut history = vec![start.clone()];
        for _ in 0..12 {
            let s = e.buffer.text();
            // Non-overlapping, non-touching cursors with at least one char between them.
            let mut b: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
            b.retain(|_| r.chance(1, 3));
            let mut cur = Vec::new();
            let mut i = 0;
            while i < b.len() {
                let wide = i + 1 < b.len() && r.chance(1, 2);
                let (a, h) = if wide { (b[i], b[i + 1]) } else { (b[i], b[i]) };
                cur.push(if r.chance(1, 2) { (a, h) } else { (h, a) });
                i += if wide { 3 } else { 2 };
            }
            if cur.is_empty() {
                cur.push((0, 0));
            }
            e.set_cursors(
                cur.iter()
                    .map(|&(a, h)| Cursor { anchor: a, head: h })
                    .collect(),
            );
            let sorted: Vec<(usize, usize)> =
                e.cursors().iter().map(|c| (c.anchor, c.head)).collect();
            let typed = if r.chance(2, 3) {
                Some(*r.pick(&["x", "", "yz", "\n", "é"]))
            } else {
                None
            };
            let (want, pos) = ref_edit(&s, &sorted, typed);
            match typed {
                Some(t) => e.type_text(t),
                None => e.backspace(),
            }
            assert_eq!(e.buffer.text(), want, "{s:?} {sorted:?} {typed:?}");
            let got: Vec<usize> = e.cursors().iter().map(|c| c.head).collect();
            assert!(e.cursors().iter().all(|c| c.anchor == c.head));
            assert_eq!(got, pos, "{s:?} {sorted:?} {typed:?}");
            if want != *history.last().unwrap() {
                history.push(want);
            }
        }
        // Undo to the root gives the start text; redo to the end gives the last text.
        let mut steps = 0;
        while e.undo() {
            steps += 1;
            assert!(steps < 100);
        }
        assert_eq!(e.buffer.text(), start);
        while e.redo() {}
        assert_eq!(&e.buffer.text(), history.last().unwrap());
    }
    // Branching: edit, undo, different edit -> two branches, both reachable.
    let mut e = Editor::new("base");
    e.set_cursors(vec![Cursor { anchor: 4, head: 4 }]);
    e.type_text("1");
    assert!(e.undo());
    assert_eq!(e.buffer.text(), "base");
    e.set_cursors(vec![Cursor { anchor: 4, head: 4 }]);
    e.type_text("2");
    assert_eq!(e.buffer.text(), "base2");
    assert!(e.undo());
    assert_eq!(e.branches(), 2);
    assert!(e.redo_branch(0));
    assert_eq!(e.buffer.text(), "base1");
    assert!(e.undo() && e.redo_branch(1));
    assert_eq!(e.buffer.text(), "base2");
}
