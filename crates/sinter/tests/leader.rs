//! Leader check for T18: Sinter's Geode output against the `mica::eval` oracle on the corpus and
//! on programs the worker has not seen; verify and round trip; trap spans; the `mica` binary.
#![allow(clippy::unwrap_used)]
use mica::check::check;
use mica::eval;
use mica::ir::Program;
use mica::parser::parse;
use std::io::Write;
use std::process::{Command, Stdio};

const F: u64 = 50_000_000;
const SAY: &str = "fn say(s: str) uses io = extern \"io.print\"\n";

fn build(src: &str) -> Program {
    let m = parse(src).unwrap_or_else(|d| panic!("parse: {d:?}\n{src}"));
    check(&m).unwrap_or_else(|d| panic!("check: {d:?}\n{src}"))
}

/// Compile, verify, round trip, and compare main, every test and every prop with the oracle.
fn same(name: &str, src: &str, input: &str) -> eval::Outcome {
    let p = build(src);
    let c = sinter::compile(&p);
    geode::verify(&c.module).unwrap_or_else(|e| panic!("{name}: verify {e:?}"));
    assert_eq!(
        geode::decode(&geode::encode(&c.module)),
        Ok(c.module.clone()),
        "{name}: round trip"
    );
    assert_eq!(c.tests.len(), p.tests.len(), "{name}: tests");
    assert_eq!(c.props.len(), p.props.len(), "{name}: props");
    let want = eval::run_main(&p, input, F);
    let got = sinter::run_main(&c, input, F);
    assert_eq!(got, want, "{name}: main differs from the oracle");
    for i in 0..p.tests.len() {
        assert_eq!(
            sinter::run_test(&c, i, F),
            eval::run_test(&p, i, F),
            "{name}: test {i}"
        );
    }
    for i in 0..p.props.len() {
        for seed in [1u64, 2, 0xdead, 77, 1 << 40] {
            assert_eq!(
                sinter::run_prop(&c, i, seed, 60, F),
                eval::run_prop(&p, i, seed, 60, F),
                "{name}: prop {i} seed {seed}"
            );
        }
    }
    want
}

#[test]
fn leader_corpus_matches_oracle() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../mica/tests/typed");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        same(&path.display().to_string(), &src, "line one\n42\n");
        n += 1;
    }
    assert!(n >= 24, "only {n} corpus programs");
}

/// Programs the worker has not seen, with outputs worked out by hand.
const MINE: &[(&str, &str, &str, &str)] = &[
    (
        "nested loops with break and continue",
        "fn main() uses io {
  var i = 0
  while i < 5 {
    i = i + 1
    if i == 2 { continue }
    for j in 0..10 {
      if j == i { break }
      if j % 2 == 1 { continue }
      say(str(i * 10 + j) + \" \")
    }
  }
}
",
        "",
        "10 30 32 40 42 50 52 54 ",
    ),
    (
        "early return from a loop, mutual recursion",
        "fn find(xs: [int], v: int) -> int {
  for i in 0..len(xs) { if xs[i] == v { return i } }
  -1
}
fn even(n: int) -> bool { if n == 0 { true } else { odd(n - 1) } }
fn odd(n: int) -> bool { if n == 0 { false } else { even(n - 1) } }
fn main() uses io {
  say(str(find([4, 8, 15, 16], 15)) + str(find([1], 9)))
  if even(1000) && odd(7) { say(\" ok\") }
}
",
        "",
        "2-1 ok",
    ),
    (
        "arrays of arrays alias, repeat literal",
        "fn main() uses io {
  var row = [0; 3]
  var grid = [row, row]
  grid[0][1] = 5
  row[2] = 9
  var copy = row[0..3]
  copy[0] = 7
  say(str(grid[1][1]) + str(grid[1][2]) + str(row[0]) + str(len(grid)))
}
",
        "",
        "5902",
    ),
    (
        "nested options, tuples and variants in match",
        "type T = Leaf(int) | Node(T, T)
fn sum(t: T) -> int { match t { Leaf(v) => v, Node(a, b) => sum(a) + sum(b) } }
fn pick(o: (int, bool)?) -> str {
  match o { none => \"n\", some((0, _)) => \"z\", some((v, true)) => str(v), some((_, false)) => \"f\" }
}
fn main() uses io {
  say(str(sum(Node(Leaf(1), Node(Leaf(2), Leaf(3))))))
  say(pick(none) + pick(some((0, true))) + pick(some((5, true))) + pick(some((5, false))))
}
",
        "",
        "6nz5f",
    ),
    (
        "constants, strings compared and joined, bytes",
        "const base: int = 1 << 4
fn main() uses io {
  var s = \"\"
  for i in 0..3 { s = s + str(base + i) + \",\" }
  if \"abc\" < \"abd\" && !(\"b\" < \"a\") { s = s + \"lt\" }
  let b = b'a' + byte(2)
  say(s + str(int(b)) + str(len(\"\u{1F600}\")))
}
",
        "",
        "16,17,18,lt994",
    ),
    (
        "function values and wrapping arithmetic",
        "fn sq(x: int) -> int { x *% x }
fn neg(x: int) -> int { 0 -% x }
fn apply(fs: [fn(int) -> int], x: int) -> int {
  var v = x
  for f in fs { v = f(v) }
  v
}
fn main() uses io {
  say(str(apply([sq, neg, sq], 3)) + \" \" + str(sq(4294967296)) + \" \" + str(neg(-9223372036854775807 - 1)))
}
",
        "",
        "81 0 -9223372036854775808",
    ),
];

#[test]
fn leader_unseen_programs_match_oracle_and_hand_output() {
    for (name, body, input, want) in MINE {
        let src = format!("{SAY}{body}");
        let out = same(name, &src, input);
        assert_eq!(out.result, Ok(()), "{name}: oracle trapped: {out:?}");
        assert_eq!(out.output, *want, "{name}: oracle output");
    }
}

#[test]
fn leader_traps_and_spans_match_oracle() {
    let progs = [
        "fn f(a: int) -> int { a + 9223372036854775807 }\nfn main() uses io { say(\"x\"); say(str(f(1))) }\n",
        "fn f(a: int, b: int) -> int { a % b }\nfn main() { let v = f(3, 0) }\n",
        "fn f(xs: [int]) -> int { xs[len(xs)] }\nfn main() { let v = f([1]) }\n",
        "fn f(n: int) { assert n > 0, \"positive\" }\nfn main() { f(0) }\n",
        "fn f(n: int) -> int { f(n) + 1 }\nfn main() { let v = f(0) }\n",
        "fn main() { var i = 0\n while true { i = i +% 1 } }\n",
        "fn w(p: str, d: [byte]) -> bool uses fs = extern \"fs.write\"\nfn main() uses io, fs { say(\"a\"); let ok = w(\"p\", [b'x']) }\n",
        "fn f(a: int) -> byte { byte(a) }\nfn main() { let v = f(-1) }\n",
        "fn f(a: int, s: int) -> int { a << s }\nfn main() { let v = f(1, 64) }\n",
    ];
    for (i, body) in progs.iter().enumerate() {
        let src = format!("{SAY}{body}");
        let p = build(&src);
        let c = sinter::compile(&p);
        let fuel = if i == 5 { 100_000 } else { F };
        let want = eval::run_main(&p, "", fuel);
        assert!(want.result.is_err(), "program {i} must trap in the oracle");
        let got = sinter::run_main(&c, "", fuel);
        assert_eq!(got.output, want.output, "program {i}: output");
        let (wt, ws) = want.result.unwrap_err();
        let (gt, gs) = got.result.unwrap_err();
        assert_eq!(gt, wt, "program {i}: trap kind");
        if wt != eval::Trap::OutOfFuel {
            assert_eq!(gs, ws, "program {i}: span");
        }
    }
}

fn mica(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mica"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn leader_binary_run_and_test() {
    let dir = std::env::temp_dir().join(format!("sinter-leader-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = |n: &str, s: &str| {
        let p = dir.join(n);
        std::fs::write(&p, s).unwrap();
        p.display().to_string()
    };
    let echo = file(
        "echo.mica",
        &format!(
            "{SAY}fn line() -> str? uses io = extern \"io.read_line\"\nfn main() uses io {{\n  match line() {{ some(s) => say(\"got \" + s), none => say(\"eof\") }}\n}}\n"
        ),
    );
    assert_eq!(
        mica(&["run", &echo], "hi\n"),
        (0, "got hi".into(), String::new())
    );
    assert_eq!(mica(&["run", &echo], "").1, "eof");

    let trap = file(
        "trap.mica",
        &format!("{SAY}fn main() uses io {{\n  say(\"a\")\n  assert 1 == 2\n}}\n"),
    );
    let (code, out, err) = mica(&["run", &trap], "");
    assert_eq!((code, out.as_str()), (1, "a"));
    assert!(
        err.contains("trap") && err.contains("4:3"),
        "stderr {err:?}"
    );

    let spin = file("spin.mica", "fn main() { while true { } }\n");
    let (code, _, err) = mica(&["run", &spin, "--fuel", "1000"], "");
    assert_eq!(code, 1, "{err}");

    let bad = file("bad.mica", "fn main() { let x = }\n");
    let (code, _, err) = mica(&["run", &bad], "");
    assert_eq!(code, 2);
    assert!(err.contains("bad.mica:1:"), "stderr {err:?}");

    let tests = file(
        "t.mica",
        "fn add(a: int, b: int) -> int { a + b }\ntest \"two\" { assert add(1, 1) == 2 }\ntest \"bad\" { assert add(1, 1) == 3 }\nprop \"sym\" (a: int, b: int) where a > -9 && a < 9 && b > -9 && b < 9 { assert add(a, b) == add(b, a) }\n",
    );
    let (code, out, _) = mica(&["test", &tests], "");
    assert_eq!(code, 1);
    assert!(out.contains("ok two") && out.contains("FAIL bad") && out.contains("ok sym"));
    assert_eq!(out.trim_end().lines().last(), Some("2 passed, 1 failed"));
    let good = file(
        "g.mica",
        "test \"one\" { assert 1 == 1 }\nprop \"p\" (x: int) { assert x == x }\n",
    );
    let (code, out, _) = mica(&["test", &good, "--seed", "9", "--cases", "50"], "");
    assert_eq!(
        (code, out.trim_end().lines().last()),
        (0, Some("2 passed, 0 failed"))
    );
    std::fs::remove_dir_all(&dir).ok();
}
