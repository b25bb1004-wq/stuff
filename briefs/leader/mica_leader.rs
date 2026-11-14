//! Leader-run check: programs written from docs/MICA.md, rejects, and a fuzz run with a new seed.
//! `STRATA_SEED` and `STRATA_CASES` change the run without editing the file.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use mica::parser::parse;
use mica::print::print;

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const GOOD: &[&str] = &[
    // layers, externs, consts, pipes, options
    "layer audit\n\
     const limit: int = 0x10 * 4 + 0b11\n\
     fn print(s: str) uses io = extern \"io.print\"\n\
     fn read_line() -> str? uses io = extern \"io.read_line\"\n\
     fn log(s: str) uses io, audit {\n  print(s)\n}\n\
     fn main() uses io, audit {\n\
       let line = read_line()\n\
       match line {\n    none => log(\"bye\"),\n    some(t) => t |> parse_int() |> show(),\n  }\n}\n\
     fn show(v: int?) uses io, audit {\n  if v == none { return }\n  log(\"got\")\n}\n",
    // records, nested types, mutation through places, while with break and continue
    "type Point = { x: int, y: int, }\n\
     type Grid = { cells: [[byte]], size: (int, int) }\n\
     fn bump(p: Point) {\n  p.x = p.x +% 1\n  p.y = p.y -% 1\n}\n\
     fn walk(g: Grid) -> int {\n\
       var i = 0\n  var n = 0\n\
       while i < g.size.0 {\n\
         i = i + 1\n\
         if g.cells[i][0] == b'\\x00' { continue }\n\
         if g.cells[i][0] == b'\\n' { break }\n\
         n = n + int(g.cells[i][0])\n  }\n  n\n}\n",
    // variants with payloads, nested patterns, tuples, else-if chains
    "type Tree = Leaf | Node(Tree, int, Tree)\n\
     type Res = Ok(int) | Err(str)\n\
     fn sum(t: Tree) -> int {\n  match t {\n    Leaf => 0,\n    Node(l, v, r) => sum(l) + v + sum(r),\n  }\n}\n\
     fn pair(a: int) -> (int, bool) {\n  (a, a > 0)\n}\n\
     fn sign(a: int) -> int {\n  if a < 0 { -1 } else if a == 0 { 0 } else { 1 }\n}\n\
     fn grade(r: Res) -> str {\n  match r {\n    Ok(0) => \"zero\",\n    Ok(_) => \"some\",\n    Err(\"\") => \"empty\",\n    Err(m) => m,\n  }\n}\n",
    // function values, for loops over ranges and arrays, repeat-lists, slices
    "fn twice(f: fn(int) -> int, x: int) -> int {\n  f(f(x))\n}\n\
     fn inc(x: int) -> int {\n  x + 1\n}\n\
     fn main() {\n\
       var xs = [0; 8]\n\
       for i in 0..len(xs) {\n    xs[i] = twice(inc, i)\n  }\n\
       let ys: [int] = xs[2..5]\n\
       for y in ys {\n    assert y >= 2, \"too small\"\n  }\n\
       let e: [int] = []\n  let u = ()\n}\n",
    // tests, props with where, unicode escapes, every operator once
    "test \"ops\" {\n\
       let a = 1 + 2 - 3 * 4 / 5 % 6\n\
       let b = (a << 2) >> 1 & 7 | 8 ^ 9\n\
       let c = !true || false && a != b\n\
       let s = \"tab\\t \\u{1F600} \\x41 \\\"q\\\"\"\n\
       assert c == c\n}\n\
     prop \"commutes\" (a: int, b: int) where a > -1000 && a < 1000 && b > -1000 && b < 1000 {\n\
       assert a + b == b + a\n}\n",
    // newline rules: trailing operators, leading pipes and dots, semicolons, comments
    "fn f(a: int,\n      b: int) -> int { // trailing comment\n\
       let x = a +\n    b\n\
       let y = x\n    |> g()\n\
       let z = [\n    1,\n    2,\n  ]\n\
       let w = z\n    .len_field\n\
       let q = 1; let r = 2\n\
       a\n    && b\n}\n",
];

const BAD: &[&str] = &[
    "fn f() {\n  let x = 1 < 2 < 3\n}\n",
    "fn f() {\n  if P { x: 1 } {\n  }\n}\n",
    "fn f() {\n  let x = 9223372036854775808\n}\n",
    "fn f() {\n  let x = (1 + 2\n}\n",
    "type lower = A\n",
    "fn Upper() {\n}\n",
    "fn f() {\n  let = 3\n}\n",
    "fn f() {\n  let s = \"open\n}\n",
    "fn f() {\n  let b = b'ab'\n}\n",
    "fn f() /* no block comments */ {\n}\n",
    "prop \"p\" () {\n}\n",
    "fn f() -> {\n}\n",
    "const x = 1\n",
    "fn f() {\n  1 + + \n}\n",
    "fn f() {\n  match x {\n    1 =>\n  }\n}\n",
];

fn stable(src: &str) {
    let m = parse(src).unwrap_or_else(|d| panic!("rejected valid program: {d:?}\n{src}"));
    let p1 = print(&m);
    let m2 = parse(&p1).unwrap_or_else(|d| panic!("printed text does not parse: {d:?}\n{p1}"));
    let p2 = print(&m2);
    assert_eq!(p1, p2, "print is not a fixed point for\n{src}");
}

#[test]
fn leader_good_programs_round_trip() {
    for src in GOOD {
        stable(src);
    }
}

#[test]
fn leader_bad_programs_rejected_with_in_range_span() {
    for src in BAD {
        let d = match parse(src) {
            Ok(_) => panic!("accepted invalid program:\n{src}"),
            Err(d) => d,
        };
        assert!(
            d.span.start <= d.span.end && d.span.end as usize <= src.len(),
            "{d:?}"
        );
        assert!(!d.message.is_empty());
    }
}

const TOKENS: &[&str] = &[
    "fn", "type", "let", "var", "if", "else", "match", "while", "for", "in", "return", "{", "}",
    "(", ")", "[", "]", ",", ":", ";", "\n", "=", "=>", "->", "|>", ".", "..", "+", "-%", "*",
    "<<", "==", "&&", "||", "!", "?", "x", "Foo", "1", "0xff", "b'a'", "\"s\"", "some", "none",
    "true", "test", "prop", "where", "uses", "extern", "_", "int", "str", "|", "// c",
];

#[test]
fn leader_fuzz_never_panics() {
    let seed = env("STRATA_SEED", 0x61ca_2026_1114);
    let cases = env("STRATA_CASES", 4000) as u32;
    forall(cases, seed, |rng| {
        let b = rng.bytes(300);
        if let Ok(s) = core::str::from_utf8(&b) {
            let _ = parse(s);
        }
        let s = String::from_utf8_lossy(&b);
        let _ = parse(&s);
    });
    forall(cases, seed ^ 1, |rng| {
        let n = rng.below(120);
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(rng.pick(TOKENS));
            s.push(' ');
        }
        if let Ok(m) = parse(&s) {
            stable(&print(&m));
        }
    });
    forall(cases, seed ^ 2, |rng: &mut Rng| {
        let src: Vec<char> = rng.pick(GOOD).chars().collect();
        let mut v = src.clone();
        for _ in 0..1 + rng.below(3) {
            let i = rng.below(v.len());
            match rng.below(3) {
                0 => {
                    v.remove(i);
                }
                1 => v.insert(
                    i,
                    *rng.pick(&['(', '}', '"', '\n', '|', '9', 'x', '\\', '\'']),
                ),
                _ => v[i] = *rng.pick(&['{', ')', ',', '=', ' ', 'A', '-']),
            }
        }
        let s: String = v.into_iter().collect();
        if let Ok(m) = parse(&s) {
            stable(&print(&m));
        }
    });
}

#[test]
fn leader_deep_nesting_is_a_diag() {
    for (open, close) in [("(", ")"), ("[", "]"), ("{", "}"), ("some(", ")")] {
        let src = format!(
            "fn f() {{\n  let x = {}1{}\n}}\n",
            open.repeat(20_000),
            close.repeat(20_000)
        );
        assert!(parse(&src).is_err());
    }
    let src = format!("fn f() {{\n  let x = {}1\n}}\n", "!".repeat(20_000));
    assert!(parse(&src).is_err());
}
