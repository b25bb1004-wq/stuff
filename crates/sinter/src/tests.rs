//! Differential tests against `mica::eval` on `typed/*.mica`, leader GOOD/BAD/PROPS, and traps.
#![allow(clippy::unwrap_used)]

use crate::{compile, run_main, run_prop, run_test, Compiled};
use geode::{decode, encode, verify};
use mica::check::check;
use mica::eval::{self as oracle, Trap};
use mica::ir::Program;
use mica::parser::parse;
use std::fs;
use std::path::PathBuf;

const FUEL: u64 = 20_000_000;
const SEEDS: [u64; 5] = [1, 7, 42, 0x1234_5678, 0xDEAD_BEEF_CAFE_BABE];

pub(crate) fn check_and_compile(src: &str) -> (Program, Compiled) {
    let ast = parse(src).unwrap_or_else(|d| panic!("parse failed: {d:?}\n{src}"));
    let prog = check(&ast).unwrap_or_else(|d| panic!("check failed: {d:?}\n{src}"));
    let comp = compile(&prog);
    verify(&comp.module).unwrap_or_else(|e| panic!("geode::verify failed: {e:?}"));
    let bytes = encode(&comp.module);
    let decoded = decode(&bytes).unwrap_or_else(|e| panic!("geode::decode failed: {e:?}"));
    assert_eq!(decoded, comp.module, "encode/decode round-trip mismatch");
    assert_eq!(comp.spans.len(), comp.module.funcs.len());
    for (f, sp) in comp.module.funcs.iter().zip(comp.spans.iter()) {
        assert_eq!(f.code.len(), sp.len());
    }
    (prog, comp)
}

pub(crate) fn assert_diff_all(src: &str, inputs: &[&str]) {
    let (p, c) = check_and_compile(src);
    for &inp in inputs {
        let want = oracle::run_main(&p, inp, FUEL);
        let got = run_main(&c, inp, FUEL);
        assert_eq!(got, want, "run_main mismatch for input {inp:?}\n{src}");
    }
    for i in 0..p.tests.len() {
        let want = oracle::run_test(&p, i, FUEL);
        let got = run_test(&c, i, FUEL);
        assert_eq!(got, want, "run_test({i}) mismatch\n{src}");
    }
    for i in 0..p.props.len() {
        for &seed in &SEEDS {
            let want = oracle::run_prop(&p, i, seed, 60, FUEL);
            let got = run_prop(&c, i, seed, 60, FUEL);
            assert_eq!(got, want, "run_prop({i}, seed={seed}) mismatch\n{src}");
        }
    }
}

#[test]
fn differential_typed_corpus() {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("../mica/tests/typed");
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "mica"))
        .collect();
    entries.sort();
    assert!(entries.len() >= 20);
    for path in entries {
        let src = fs::read_to_string(&path).unwrap();
        assert_diff_all(&src, &["", "hello\n42\n", "5\nx\n-3\n"]);
    }
}

const LEADER_EVAL_GOOD: &[(&str, &str)] = &[
    (
        "fn fact(n: int) -> int { if n <= 1 { 1 } else { n * fact(n - 1) } }
fn main() uses io {
  var i = 0
  while i < 6 { say(str(fact(i)) + \" \"); i = i + 1 }
}",
        "",
    ),
    (
        "type P = { x: int, y: int }
fn bump(p: P) { p.x = p.x + 10 }
fn main() uses io {
  let a = P { x: 1, y: 2 }
  let b = a
  bump(b)
  var xs = [a, b]
  xs[0].y = 7
  say(str(a.x) + \",\" + str(a.y) + \",\" + str(b.y))
}",
        "",
    ),
    (
        "fn main() uses io {
  var xs = [1, 2, 3]
  var ys = xs[1..3]
  push(xs, 4)
  ys[0] = 99
  var sum = 0
  for x in xs {
    sum = sum + x
    if x == 1 { push(xs, 10) }
  }
  say(str(sum) + \" \" + str(len(xs)) + \" \" + str(ys[0]) + \" \" + str(xs[1]))
  match pop(xs) { some(v) => say(\" p\" + str(v)), none => say(\" none\") }
}",
        "",
    ),
    (
        "type Shape = Circle(int) | Rect(int, int) | Empty
fn area(s: Shape) -> int { match s { Circle(r) => 3 * r * r, Rect(w, h) => w * h, Empty => 0 } }
fn total(shapes: [Shape]) -> int {
  var sum = 0
  for s in shapes { sum = sum + area(s) }
  sum
}
fn main() uses io { [Circle(2), Rect(3, 4), Empty] |> total() |> str() |> say() }",
        "",
    ),
    (
        "fn line() -> str? uses io = extern \"io.read_line\"
fn main() uses io {
  var total = 0
  var go = true
  while go {
    match line() {
      some(s) => match parse_int(s) { some(n) => { total = total + n }, none => say(\"bad:\" + s + \";\") },
      none => { go = false },
    }
  }
  say(str(total))
}",
        "5\nx\n-3",
    ),
    (
        "fn main() uses io {
  let b = bytes(\"h\u{e9}\")
  say(str(len(b)) + \" \" + str(int(b[1])))
  match text([b'a', byte(255)]) { some(_) => say(\" ok\"), none => say(\" notutf8\") }
}",
        "",
    ),
    (
        "fn t(s: str) -> bool uses io { say(s); true }
fn f(s: str) -> bool uses io { say(s); false }
fn add(a: int, b: int) -> int { a + b }
fn main() uses io {
  if f(\"a\") && t(\"b\") { say(\"X\") }
  if t(\"c\") || f(\"d\") { say(\"Y\") }
  let r = 5 |> add(3) |> add(-1)
  say(str(r))
}",
        "",
    ),
    (
        "fn main() uses io {
  say(str(-7 / 2) + \" \" + str(-7 % 2) + \" \" + str(-8 >> 1) + \" \" + str(9223372036854775807 +% 1))
}",
        "",
    ),
    (
        "fn inc(x: int) -> int { x + 1 }
fn twice(f: fn(int) -> int, x: int) -> int { f(f(x)) }
fn main() uses io {
  let p = (1, \"a\")
  say(str(p.0) + p.1)
  if (1, \"a\") == p { say(\"eq\") }
  say(str(twice(inc, 5)))
}",
        "",
    ),
    (
        "fn down(n: int) -> int { if n == 0 { 0 } else { 1 + down(n - 1) } }
fn main() uses io { say(str(down(9000))) }",
        "",
    ),
    (
        "fn now() -> int uses time = extern \"time.now\"
fn random() -> int uses rand = extern \"rand.next\"
fn main() uses io, time, rand {
  let a = random()
  let b = random()
  say(str(now()))
  if a != b { say(\" differ\") }
}",
        "",
    ),
];

#[test]
fn differential_leader_eval_good_and_props() {
    for (body, inp) in LEADER_EVAL_GOOD {
        let src = format!("fn say(s: str) uses io = extern \"io.print\"\n{body}\n");
        assert_diff_all(&src, &[inp]);
    }
    let props_src = "fn add(a: int, b: int) -> int { a + b }
test \"passes\" { assert add(2, 2) == 4 }
test \"fails\" { assert add(2, 2) == 5, \"math\" }
prop \"small\" (x: int) { assert x < 500 }
prop \"never\" (x: int) where x > 0 && x < 0 { assert false }
prop \"doubling\" (x: int) where x > 0 { assert add(x, x) > x }
prop \"commutes\" (a: int, b: int) where a > -1000 && a < 1000 && b > -1000 && b < 1000 { assert add(a, b) == add(b, a) }
";
    let (p, c) = check_and_compile(props_src);
    for i in 0..p.tests.len() {
        assert_eq!(run_test(&c, i, FUEL), oracle::run_test(&p, i, FUEL));
    }
    for i in 0..p.props.len() {
        for &seed in &[1, 7, 9, 42, 99] {
            assert_eq!(
                run_prop(&c, i, seed, 500, FUEL),
                oracle::run_prop(&p, i, seed, 500, FUEL)
            );
        }
    }
}

const LEADER_CHECK_GOOD: &[&str] = &[
    "type Shape = Circle(int) | Rect(int, int)
fn say(s: str) uses io = extern \"io.print\"
fn area(s: Shape) -> int { match s { Circle(r) => 3 * r * r, Rect(w, h) => w * h } }
fn total(shapes: [Shape]) -> int { var sum = 0; for s in shapes { sum = sum + area(s) }; sum }
fn main() uses io { [Circle(2), Rect(3, 4)] |> total() |> str() |> say() }
test \"a rectangle\" { assert area(Rect(3, 4)) == 12 }
prop \"area is never negative\" (w: int, h: int) where w >= 0 && w < 1000 && h >= 0 && h < 1000 {
    assert area(Rect(w, h)) >= 0
}",
    "type Point = { x: int, y: int }
fn bump(p: Point) { var q = p; q.x = q.x +% 1; q.y = q.y -% 1 }
fn main() {
    let p = Point { x: 1, y: 2 }
    bump(p)
    var xs = [p, Point { x: 0, y: 0 }]
    xs[1].x = 5
    xs[0] = Point { x: 9, y: 9 }
    assert xs[1].x == 5
}",
    "layer audit
const limit: int = 0x10 * 4 + 0b11
fn say(s: str) uses io = extern \"io.print\"
fn line() -> str? uses io = extern \"io.read_line\"
fn log(s: str) uses io, audit { say(s) }
fn show(v: int?) uses io, audit {
    if v == none { return }
    match v { none => log(\"none\"), some(n) => if n > limit { log(\"big\") } else { log(\"small\") } }
}
fn main() uses io, audit {
    match line() { none => log(\"bye\"), some(t) => t |> parse_int() |> show() }
}",
    "fn twice(f: fn(int) -> int, x: int) -> int { f(f(x)) }
fn inc(x: int) -> int { x + 1 }
fn main() {
    var xs = [0; 8]
    for i in 0..len(xs) { xs[i] = twice(inc, i) }
    let ys: [int] = xs[2..5]
    for y in ys { assert y >= 2, \"too small\" }
    let e: [int] = []
    let u = ()
    var n = 0
    while true { n = n + 1; if n == 3 { continue }; if n > 5 { break } }
}",
    "fn pair(a: int) -> (int, bool) { (a, a > 0) }
fn same(a: (int, bool), b: (int, bool)) -> bool { a == b }
fn differ(a: int?, b: int?) -> bool { a != b }
fn both(a: bool, b: bool) -> bool { a && b || !a }
fn main() {
    assert same(pair(1), (1, true))
    assert differ(some(1), none)
    assert both(true, true)
    let t = pair(3)
    assert t.0 == 3 && t.1
}",
    "type Tree = Leaf | Node(Tree, int, Tree)
type Res = Ok(int) | Err(str)
fn sum(t: Tree) -> int { match t { Leaf => 0, Node(l, v, r) => sum(l) + v + sum(r) } }
fn grade(r: Res) -> str { match r { Ok(0) => \"zero\", Ok(_) => \"some\", Err(\"\") => \"empty\", Err(m) => m } }
fn sign(a: int) -> int { if a < 0 { -1 } else if a == 0 { 0 } else { 1 } }
test \"tree\" { assert sum(Node(Leaf, 3, Node(Leaf, 4, Leaf))) == 7 }",
    "fn main() {
    var xs: [int] = []
    push(xs, 3); push(xs, 4)
    let last = pop(xs)
    assert last == some(4)
    let s = \"ab\" + \"cd\"
    assert len(s) == 4
    let b = bytes(s)
    assert text(b) == some(\"abcd\")
    assert int(b[0]) == 97
    assert byte(98) == b[1]
    assert min(1, 2) + max(3, 4) + abs(-5) == 10
    let joined = [1, 2] + [3]
    assert len(joined) == 3
    assert str(42) == \"42\"
    assert parse_int(\"x\") == none
    assert b'a' < b'b' && \"a\" < \"b\"
}",
    "fn classify(n: int) -> str { match n { 0 => \"zero\", 1 => \"one\", _ => \"many\" } }
fn flip(b: bool) -> int { match b { true => 1, false => 0 } }
fn both(p: (bool, bool)) -> int { match p { (true, true) => 2, (true, false) => 1, _ => 0 } }
fn main() {
    let x: byte = b'z'
    let y = x - b'a'
    assert flip(true) + both((true, false)) == 2
    assert classify(int(y)) == \"many\"
}",
    "fn clock() -> int uses time = extern \"time.now\"
fn say(s: str) uses io = extern \"io.print\"
fn stamp() uses io, time { clock() |> str() |> say() }
test \"can use layers\" { stamp() }
prop \"abs is non-negative\" (a: int) where a > -1000000 { assert abs(a) >= 0 }
fn main() uses io, time, rand { stamp() }",
    "layer audit
layer metrics
fn post(e: str) uses chain = extern \"chain.emit\"
fn rec(e: str) uses chain, metrics { post(e) }
fn main() uses chain, audit, metrics { rec(\"x\") }",
    "fn say(s: str) uses io = extern \"io.print\"
fn quiet(s: str) { }
fn run(f: fn(str) -> unit uses io, s: str) uses io { f(s) }
fn main() uses io { run(say, \"a\"); run(quiet, \"b\") }",
    "fn f(a: int) -> int {
    let b = { let c = a * 2; c + 1 }
    let d = if b > 3 { b } else { 3 }
    var e = 0
    for i in 0..d { e = e + i }
    e
}",
    "const k: int = 2 * 3 + 1
fn seven() -> int { k }",
];

#[test]
fn differential_leader_check_good() {
    for src in LEADER_CHECK_GOOD {
        assert_diff_all(src, &["", "100\n", "abc\n"]);
    }
}

#[test]
fn every_trap_kind_matches_oracle_and_span() {
    let cases = [
        "fn say(s: str) uses io = extern \"io.print\"\nfn add(a: int, b: int) -> int { a + b }\nfn main() uses io { say(\"before\"); say(str(add(9223372036854775807, 1))) }\n",
        "fn neg(a: int) -> int { -a }\nfn main() { let _ = neg(-9223372036854775807 - 1) }\n",
        "fn mul(a: int, b: int) -> int { a * b }\nfn main() { let _ = mul(4611686018427387904, 2) }\n",
        "fn add(a: byte, b: byte) -> byte { a + b }\nfn main() { let _ = add(byte(250), byte(10)) }\n",
        "fn cv(a: int) -> byte { byte(a) }\nfn main() { let _ = cv(256) }\n",
        "fn d(a: int, b: int) -> int { a / b }\nfn main() { let _ = d(1, 0) }\n",
        "fn d(a: int, b: int) -> int { a % b }\nfn main() { let _ = d(1, 0) }\n",
        "fn d(a: int, b: int) -> int { a / b }\nfn main() { let _ = d(-9223372036854775807 - 1, -1) }\n",
        "fn s(a: int, b: int) -> int { a << b }\nfn main() { let _ = s(1, 64) }\n",
        "fn s(a: int, b: int) -> int { a >> b }\nfn main() { let _ = s(1, -1) }\n",
        "fn at(xs: [int], i: int) -> int { xs[i] }\nfn main() { let _ = at([1, 2, 3], 3) }\n",
        "fn at(xs: [int], i: int) -> int { xs[i] }\nfn main() { let _ = at([1, 2, 3], -1) }\n",
        "fn sl(xs: [int], b: int) -> [int] { xs[0..b] }\nfn main() { let _ = sl([1, 2, 3], 4) }\n",
        "fn sl(xs: [int], a: int) -> [int] { xs[a..1] }\nfn main() { let _ = sl([1, 2, 3], 2) }\n",
        "fn chk(x: int) { assert x == 2, \"x must be two\" }\nfn main() { chk(3) }\n",
        "fn rd(p: str) -> [byte]? uses fs = extern \"fs.read\"\nfn main() uses fs { let _ = rd(\"/etc\") }\n",
        "fn rec(n: int) -> int { rec(n + 1) }\nfn main() { let _ = rec(0) }\n",
    ];
    for src in cases {
        let (p, c) = check_and_compile(src);
        let want = oracle::run_main(&p, "", FUEL);
        let got = run_main(&c, "", FUEL);
        assert!(want.result.is_err());
        assert_eq!(got, want, "trap/span mismatch for:\n{src}");
    }
    let loop_src = "fn main() { let x = 1; while true {} }\n";
    let (p, c) = check_and_compile(loop_src);
    assert_eq!(run_main(&c, "", 0), oracle::run_main(&p, "", 0));
    assert_eq!(run_main(&c, "", 1), oracle::run_main(&p, "", 1));
    assert_eq!(
        run_main(&c, "", 100).result.map_err(|e| e.0),
        Err(Trap::OutOfFuel)
    );
}

#[test]
fn empty_program_and_out_of_bounds_indices_and_duplicate_names() {
    let (_, c_empty) = check_and_compile("type Unit2 = A\nconst k: int = 5\n");
    assert_eq!(c_empty.main, None);
    assert_eq!(run_main(&c_empty, "", FUEL).result, Ok(()));
    assert_eq!(
        run_test(&c_empty, 0, FUEL).result.map_err(|e| e.0),
        Err(Trap::IndexOutOfRange)
    );
    assert_eq!(
        run_prop(&c_empty, 0, 1, 10, FUEL).result.map_err(|e| e.0),
        Err(Trap::IndexOutOfRange)
    );

    let dup_src = r#"
fn foo() -> int { 42 }
test "foo" { assert foo() == 42 }
test "foo" { assert foo() + 1 == 43 }
prop "foo" (x: int) where x >= 0 && x < 100 { assert x + foo() >= 42 }
"#;
    assert_diff_all(dup_src, &[""]);
}

#[test]
fn indirect_extern_call_value_traps_with_span() {
    let src = r#"
fn rd(p: str) -> [byte]? uses fs = extern "fs.read"
fn apply(f: fn(str) -> [byte]? uses fs, s: str) -> [byte]? uses fs { f(s) }
fn main() uses fs { let _ = apply(rd, "/etc") }
"#;
    assert_diff_all(src, &[""]);
}
