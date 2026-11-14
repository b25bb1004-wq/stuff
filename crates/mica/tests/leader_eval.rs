//! Leader check for T19: the reference interpreter on programs whose output was worked out by
//! hand, every trap kind with the span of the trapping code, fuel, call depth, externs and props.
#![allow(clippy::unwrap_used)]
use mica::check::check;
use mica::eval::{run_main, run_prop, run_test, Outcome, Trap};
use mica::ir::Program;
use mica::parser::parse;

const SAY: &str = "fn say(s: str) uses io = extern \"io.print\"\n";
const BIG: u64 = 50_000_000;

fn build(src: &str) -> Program {
    let m = parse(src).unwrap_or_else(|d| panic!("parse: {d:?}\n{src}"));
    check(&m).unwrap_or_else(|d| panic!("check: {d:?}\n{src}"))
}

fn main_of(body: &str, input: &str, fuel: u64) -> (String, Outcome) {
    let src = format!("{SAY}{body}");
    let p = build(&src);
    let out = run_main(&p, input, fuel);
    assert_eq!(out, run_main(&p, input, fuel), "not deterministic");
    (src, out)
}

const GOOD: &[(&str, &str, &str, &str)] = &[
    (
        "factorial",
        "fn fact(n: int) -> int { if n <= 1 { 1 } else { n * fact(n - 1) } }
fn main() uses io {
  var i = 0
  while i < 6 { say(str(fact(i)) + \" \"); i = i + 1 }
}
",
        "",
        "1 1 2 6 24 120 ",
    ),
    (
        "records are shared",
        "type P = { x: int, y: int }
fn bump(p: P) { p.x = p.x + 10 }
fn main() uses io {
  let a = P { x: 1, y: 2 }
  let b = a
  bump(b)
  var xs = [a, b]
  xs[0].y = 7
  say(str(a.x) + \",\" + str(a.y) + \",\" + str(b.y))
}
",
        "",
        "11,7,7",
    ),
    (
        "arrays, slices copy, for re-reads len, pop",
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
}
",
        "",
        "20 5 99 2 p10",
    ),
    (
        "variants and pipes",
        "type Shape = Circle(int) | Rect(int, int) | Empty
fn area(s: Shape) -> int { match s { Circle(r) => 3 * r * r, Rect(w, h) => w * h, Empty => 0 } }
fn total(shapes: [Shape]) -> int {
  var sum = 0
  for s in shapes { sum = sum + area(s) }
  sum
}
fn main() uses io { [Circle(2), Rect(3, 4), Empty] |> total() |> str() |> say() }
",
        "",
        "24",
    ),
    (
        "read_line, parse_int, options",
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
}
",
        "5\nx\n-3",
        "bad:x;2",
    ),
    (
        "bytes and text",
        "fn main() uses io {
  let b = bytes(\"h\u{e9}\")
  say(str(len(b)) + \" \" + str(int(b[1])))
  match text([b'a', byte(255)]) { some(_) => say(\" ok\"), none => say(\" notutf8\") }
}
",
        "",
        "3 195 notutf8",
    ),
    (
        "left to right, short circuit, pipe arguments",
        "fn t(s: str) -> bool uses io { say(s); true }
fn f(s: str) -> bool uses io { say(s); false }
fn add(a: int, b: int) -> int { a + b }
fn main() uses io {
  if f(\"a\") && t(\"b\") { say(\"X\") }
  if t(\"c\") || f(\"d\") { say(\"Y\") }
  let r = 5 |> add(3) |> add(-1)
  say(str(r))
}
",
        "",
        "acY7",
    ),
    (
        "division, remainder, shifts, wrapping",
        "fn main() uses io {
  say(str(-7 / 2) + \" \" + str(-7 % 2) + \" \" + str(-8 >> 1) + \" \" + str(9223372036854775807 +% 1))
}
",
        "",
        "-3 -1 -4 -9223372036854775808",
    ),
    (
        "tuples and function values",
        "fn inc(x: int) -> int { x + 1 }
fn twice(f: fn(int) -> int, x: int) -> int { f(f(x)) }
fn main() uses io {
  let p = (1, \"a\")
  say(str(p.0) + p.1)
  if (1, \"a\") == p { say(\"eq\") }
  say(str(twice(inc, 5)))
}
",
        "",
        "1aeq7",
    ),
    (
        "deep recursion under the limit",
        "fn down(n: int) -> int { if n == 0 { 0 } else { 1 + down(n - 1) } }
fn main() uses io { say(str(down(9000))) }
",
        "",
        "9000",
    ),
    (
        "time and rand externs",
        "fn now() -> int uses time = extern \"time.now\"
fn random() -> int uses rand = extern \"rand.next\"
fn main() uses io, time, rand {
  let a = random()
  let b = random()
  say(str(now()))
  if a != b { say(\" differ\") }
}
",
        "",
        "0 differ",
    ),
];

#[test]
fn leader_hand_checked_programs() {
    for (name, body, input, want) in GOOD {
        let (_, out) = main_of(body, input, BIG);
        assert_eq!(
            out.result,
            Ok(()),
            "{name}: trapped, output {:?}",
            out.output
        );
        assert_eq!(out.output, *want, "{name}");
    }
}

/// (name, program, accepted trap kinds, text the trapping span must contain)
type Bad = (&'static str, &'static str, fn(&Trap) -> bool, &'static str);
const BAD: &[Bad] = &[
    (
        "add overflow",
        "fn add(a: int, b: int) -> int { a + b }
fn main() uses io { say(\"before\"); say(str(add(9223372036854775807, 1))) }
",
        |t| *t == Trap::Overflow,
        "+",
    ),
    (
        "negate min",
        "fn neg(a: int) -> int { -a }
fn main() { let m = neg(-9223372036854775807 - 1) }
",
        |t| *t == Trap::Overflow,
        "-a",
    ),
    (
        "mul overflow",
        "fn mul(a: int, b: int) -> int { a * b }
fn main() { let m = mul(4611686018427387904, 2) }
",
        |t| *t == Trap::Overflow,
        "*",
    ),
    (
        "byte add",
        "fn add(a: byte, b: byte) -> byte { a + b }
fn main() { let m = add(byte(250), byte(10)) }
",
        |t| *t == Trap::Overflow,
        "+",
    ),
    (
        "byte() out of range",
        "fn cv(a: int) -> byte { byte(a) }
fn main() { let m = cv(256) }
",
        |_| true,
        "byte",
    ),
    (
        "divide by zero",
        "fn d(a: int, b: int) -> int { a / b }
fn main() { let m = d(1, 0) }
",
        |t| *t == Trap::DivideByZero,
        "/",
    ),
    (
        "remainder by zero",
        "fn d(a: int, b: int) -> int { a % b }
fn main() { let m = d(1, 0) }
",
        |t| *t == Trap::DivideByZero,
        "%",
    ),
    (
        "min / -1",
        "fn d(a: int, b: int) -> int { a / b }
fn main() { let m = d(-9223372036854775807 - 1, -1) }
",
        |t| matches!(t, Trap::Overflow | Trap::DivideByZero),
        "/",
    ),
    (
        "shift by 64",
        "fn s(a: int, b: int) -> int { a << b }
fn main() { let m = s(1, 64) }
",
        |_| true,
        "<<",
    ),
    (
        "shift by -1",
        "fn s(a: int, b: int) -> int { a >> b }
fn main() { let m = s(1, -1) }
",
        |_| true,
        ">>",
    ),
    (
        "index past end",
        "fn at(xs: [int], i: int) -> int { xs[i] }
fn main() { let m = at([1, 2, 3], 3) }
",
        |t| *t == Trap::IndexOutOfRange,
        "[",
    ),
    (
        "negative index",
        "fn at(xs: [int], i: int) -> int { xs[i] }
fn main() { let m = at([1, 2, 3], -1) }
",
        |t| *t == Trap::IndexOutOfRange,
        "[",
    ),
    (
        "slice past end",
        "fn sl(xs: [int], b: int) -> [int] { xs[0..b] }
fn main() { let m = sl([1, 2, 3], 4) }
",
        |t| *t == Trap::IndexOutOfRange,
        "..",
    ),
    (
        "slice backwards",
        "fn sl(xs: [int], a: int) -> [int] { xs[a..1] }
fn main() { let m = sl([1, 2, 3], 2) }
",
        |t| *t == Trap::IndexOutOfRange,
        "..",
    ),
    (
        "assert with message",
        "fn chk(x: int) { assert x == 2, \"x must be two\" }
fn main() { chk(3) }
",
        |t| *t == Trap::AssertFailed("x must be two".into()),
        "assert",
    ),
    (
        "missing extern",
        "fn rd(p: str) -> [byte]? uses fs = extern \"fs.read\"
fn main() uses fs { let m = rd(\"/etc\") }
",
        |t| *t == Trap::NoExtern("fs.read".into()),
        "rd",
    ),
];

#[test]
fn leader_every_trap_has_its_kind_and_span() {
    for (name, body, ok, needle) in BAD {
        let (src, out) = main_of(body, "", BIG);
        let (trap, span) = out
            .result
            .clone()
            .err()
            .unwrap_or_else(|| panic!("{name}: did not trap"));
        assert!(ok(&trap), "{name}: wrong trap {trap:?}");
        let (s, e) = (span.start as usize, span.end as usize);
        assert!(s < e && e <= src.len(), "{name}: bad span {span:?}");
        let text = &src[s..e];
        assert!(
            text.contains(needle) && text.len() < 60,
            "{name}: span text {text:?} should be the trapping code ({needle})"
        );
    }
    let (_, out) = main_of(BAD[0].1, "", BIG);
    assert_eq!(out.output, "before", "output before a trap is kept");
}

#[test]
fn leader_fuel_and_depth() {
    let (_, out) = main_of("fn main() { while true { } }\n", "", 10_000);
    assert_eq!(out.result.map_err(|e| e.0), Err(Trap::OutOfFuel));

    let (_, out) = main_of(
        "fn r(n: int) -> int { r(n + 1) }\nfn main() { let m = r(0) }\n",
        "",
        BIG,
    );
    assert_eq!(out.result.map_err(|e| e.0), Err(Trap::StackOverflow));

    // A finite program: there is an exact fuel at which it passes, and one less fails.
    let body = GOOD[0].1;
    let mut lo = 0u64;
    let mut hi = 1_000_000u64;
    assert_eq!(main_of(body, "", hi).1.result, Ok(()));
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if main_of(body, "", mid).1.result.is_ok() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let (_, short) = main_of(body, "", lo);
    assert_eq!(short.result.map_err(|e| e.0), Err(Trap::OutOfFuel));
    assert!(hi > 50, "factorial loop needs more than {hi} instructions");
    eprintln!("factorial program needs fuel {hi}");
}

const PROPS: &str = "fn add(a: int, b: int) -> int { a + b }
test \"passes\" { assert add(2, 2) == 4 }
test \"fails\" { assert add(2, 2) == 5, \"math\" }
prop \"small\" (x: int) { assert x < 500 }
prop \"never\" (x: int) where x > 0 && x < 0 { assert false }
prop \"doubling\" (x: int) where x > 0 { assert add(x, x) > x }
prop \"commutes\" (a: int, b: int) where a > -1000 && a < 1000 && b > -1000 && b < 1000 { assert add(a, b) == add(b, a) }
";

#[test]
fn leader_tests_and_props() {
    let p = build(PROPS);
    assert_eq!(p.tests.len(), 2);
    assert_eq!(run_test(&p, 0, BIG).result, Ok(()));
    let f = run_test(&p, 1, BIG);
    assert_eq!(
        f.result.map_err(|e| e.0),
        Err(Trap::AssertFailed("math".into()))
    );

    let small = run_prop(&p, 0, 7, 200, BIG);
    assert!(
        matches!(&small.result, Err((Trap::AssertFailed(_), _))),
        "x < 500 must fail: {small:?}"
    );
    assert!(!small.output.is_empty(), "failure must name the case");
    assert_eq!(small, run_prop(&p, 0, 7, 200, BIG), "replay differs");

    assert_eq!(run_prop(&p, 1, 7, 200, BIG).result, Ok(()), "all skipped");
    let dbl = run_prop(&p, 2, 9, 500, BIG);
    assert_eq!(
        dbl.result.map_err(|e| e.0),
        Err(Trap::Overflow),
        "large ints must be drawn: x + x overflows"
    );
    for seed in 0..20 {
        assert_eq!(run_prop(&p, 3, seed, 100, BIG).result, Ok(()));
    }
}
