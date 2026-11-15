//! Tests for the Logos reference interpreter.
#![allow(clippy::unwrap_used)]

use super::{run_main, run_prop, run_test, Outcome, Trap};
use crate::check::check;
use crate::ir::Program;
use crate::parser::parse;
use std::fs;
use std::path::PathBuf;

const FUEL: u64 = 1_000_000;

fn compile(src: &str) -> Program {
    let m = parse(src).unwrap_or_else(|d| panic!("parse failed: {:?}", d));
    check(&m).unwrap_or_else(|d| panic!("check failed: {:?}", d))
}

fn eval_main(src: &str, input: &str) -> Outcome {
    let p = compile(src);
    run_main(&p, input, FUEL)
}

#[test]
fn typed_corpus_all_run() {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("tests");
    dir.push("typed");
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "logos"))
        .collect();
    entries.sort();
    assert!(entries.len() >= 20);
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        let p = compile(&src);
        if p.main.is_some() {
            let out = run_main(&p, "hello\n42\n", FUEL);
            if let Err((trap, _)) = &out.result {
                assert!(
                    matches!(trap, Trap::NoExtern(_)),
                    "typed/{} main trapped unexpectedly: {:?}",
                    name,
                    out
                );
            }
        }
        for i in 0..p.tests.len() {
            let out = run_test(&p, i, FUEL);
            assert!(
                out.result.is_ok(),
                "typed/{} test {} failed: {:?}",
                name,
                i,
                out
            );
        }
        for i in 0..p.props.len() {
            let out = run_prop(&p, i, 0x1234_5678, 50, FUEL);
            assert!(
                out.result.is_ok(),
                "typed/{} prop {} failed: {:?}",
                name,
                i,
                out
            );
        }
    }
}

#[test]
fn prog_01_factorial() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn fact(n: int) -> int {
    if n <= 1 { 1 } else { n * fact(n - 1) }
}
fn main() uses io {
    print(str(fact(6)))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "720");
}

#[test]
fn prog_02_string_building() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var s = "hi"
    s = s + " " + "logos" + " #" + str(7)
    print(s + " len=" + str(len(s)))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "hi logos #7 len=11");
}

#[test]
fn prog_03_match_on_variants() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
type Shape = Circle(int) | Rect(int, int) | Point
fn area(s: Shape) -> int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
        Point => 0,
    }
}
fn main() uses io {
    let xs = [Circle(2), Rect(3, 4), Point]
    var total = 0
    for s in xs { total = total + area(s) }
    print(str(total))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "24");
}

#[test]
fn prog_04_records_shared_and_mutated() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
type Counter = { val: int }
fn bump(c: Counter, by: int) {
    c.val = c.val + by
}
fn main() uses io {
    let a = Counter { val: 10 }
    let b = a
    bump(b, 5)
    a.val = a.val + 2
    print(str(a.val) + "," + str(b.val))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "17,17");
}

#[test]
fn prog_05_arrays_shared_and_mutated() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn append_double(xs: [int], v: int) {
    var zs = xs
    push(zs, v * 2)
}
fn main() uses io {
    let xs = [1, 2]
    var ys = xs
    append_double(ys, 3)
    xs[0] = 9
    let last = pop(ys)
    match last {
        some(v) => print(str(xs[0]) + "," + str(len(xs)) + "," + str(v)),
        none => print("empty"),
    }
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "9,2,6");
}

#[test]
fn prog_06_options_and_parse_int() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn show(o: int?) -> str {
    match o {
        some(x) => "some(" + str(x) + ")",
        none => "none",
    }
}
fn main() uses io {
    print(show(parse_int("-42")) + " " + show(parse_int("abc")))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "some(-42) none");
}

#[test]
fn prog_07_slices_array_and_str() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let xs = [10, 20, 30, 40]
    let sub = xs[1..3]
    sub[0] = 99
    let s = "abcdef"
    print(str(xs[1]) + "," + str(sub[0]) + "," + str(sub[1]) + "," + s[1..4])
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "20,99,30,bcd");
}

#[test]
fn prog_08_pipes() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn add(a: int, b: int) -> int { a + b }
fn mul(a: int, b: int) -> int { a * b }
fn main() uses io {
    3 |> add(4) |> mul(5) |> str() |> print()
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "35");
}

#[test]
fn prog_09_loops_break_continue() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    var sum = 0
    for i in 0..10 {
        if i % 2 == 0 { continue }
        if i > 7 { break }
        sum = sum + i
    }
    print(str(sum))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "16");
}

#[test]
fn prog_10_first_class_functions() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn inc(x: int) -> int { x + 1 }
fn sq(x: int) -> int { x * x }
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
fn main() uses io {
    print(str(apply(sq, apply(inc, 4))))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "25");
}

#[test]
fn prog_11_io_read_line_and_time_and_rand() {
    let src = r#"
fn print(s: str) uses io = extern "io.print"
fn read_line() -> str? uses io = extern "io.read_line"
fn now() -> int uses time = extern "time.now"
fn random() -> int uses rand = extern "rand.next"
fn main() uses io, time, rand {
    print(str(now()) + "|")
    var done = false
    while !done {
        match read_line() {
            some(line) => print("[" + line + "]"),
            none => { done = true },
        }
    }
    print("|" + str(random()))
}
"#;
    let out1 = eval_main(src, "alpha\nbeta\n");
    let out2 = eval_main(src, "alpha\nbeta\n");
    assert_eq!(out1.result, Ok(()));
    assert_eq!(out1.output, out2.output);
    assert!(out1.output.starts_with("0|[alpha][beta]|"));
}

#[test]
fn prog_12_wrapping_and_bitwise_shifts() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let max_i = 9223372036854775807
    let wrapped = max_i +% 1
    let arith_shr = (-16) >> 2
    let shl = 1 << 4
    print(str(wrapped) + "," + str(arith_shr) + "," + str(shl))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "-9223372036854775808,-4,16");
}

#[test]
fn prog_13_bytes_and_text_utf8() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let bs = bytes("AB")
    bs[1] = bs[1] + byte(1)
    let ok = match text(bs) { some(s) => s, none => "?" }
    let bad = match text([byte(255)]) { some(s) => s, none => "bad" }
    print(ok + "," + bad + "," + str(int(bs[0])))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "AC,bad,65");
}

#[test]
fn prog_14_short_circuit_avoids_trap() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let a = false && (1 / 0 == 0)
    let b = true || (1 / 0 == 0)
    if !a && b { print("ok") }
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "ok");
}

#[test]
fn prog_15_tuples_new_fill_min_max_abs() {
    let out = eval_main(
        r#"
fn print(s: str) uses io = extern "io.print"
fn main() uses io {
    let pair = (min(-5, 3), max(-5, 3))
    let xs = [abs(-7); 3]
    print(str(pair.0) + "," + str(pair.1) + "," + str(xs[2]) + "," + str(len(xs)))
}
"#,
        "",
    );
    assert_eq!(out.result, Ok(()));
    assert_eq!(out.output, "-5,3,7,3");
}

#[test]
fn trap_overflow() {
    let src = r#"
fn main() {
    let x = 9223372036854775807
    let _y = x + 1
}
"#;
    let out = eval_main(src, "");
    let Err((trap, span)) = out.result else {
        panic!("expected Overflow trap");
    };
    assert_eq!(trap, Trap::Overflow);
    assert_eq!(&src[span.start as usize..span.end as usize], "x + 1");
}

#[test]
fn trap_divide_by_zero() {
    let src = r#"
fn main() {
    let _x = 10 / 0
}
"#;
    let out = eval_main(src, "");
    let Err((trap, span)) = out.result else {
        panic!("expected DivideByZero trap");
    };
    assert_eq!(trap, Trap::DivideByZero);
    assert_eq!(&src[span.start as usize..span.end as usize], "10 / 0");
}

#[test]
fn trap_index_out_of_range() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3]
    let _x = xs[5]
}
"#;
    let out = eval_main(src, "");
    let Err((trap, span)) = out.result else {
        panic!("expected IndexOutOfRange trap");
    };
    assert_eq!(trap, Trap::IndexOutOfRange);
    assert_eq!(&src[span.start as usize..span.end as usize], "xs[5]");
}

#[test]
fn trap_assert_failed() {
    let src = r#"
fn main() {
    assert 1 == 2, "math broke"
}
"#;
    let out = eval_main(src, "");
    let Err((trap, span)) = out.result else {
        panic!("expected AssertFailed trap");
    };
    assert_eq!(trap, Trap::AssertFailed("math broke".to_string()));
    assert_eq!(
        &src[span.start as usize..span.end as usize],
        "assert 1 == 2, \"math broke\""
    );
}

#[test]
fn trap_out_of_fuel() {
    let p = compile(
        r#"
fn main() {
    while true {}
}
"#,
    );
    let out = run_main(&p, "", 50);
    assert!(matches!(out.result, Err((Trap::OutOfFuel, _))));
}

#[test]
fn trap_stack_overflow() {
    let p = compile(
        r#"
fn rec(n: int) -> int { rec(n + 1) }
fn main() { let _ = rec(0) }
"#,
    );
    let out = run_main(&p, "", 10_000_000);
    assert!(matches!(out.result, Err((Trap::StackOverflow, _))));
}

#[test]
fn trap_no_extern() {
    let src = r#"
fn read_file(path: str) -> [byte]? uses fs = extern "fs.read"
fn main() uses fs {
    let _ = read_file("a.txt")
}
"#;
    let out = eval_main(src, "");
    assert!(matches!(out.result, Err((Trap::NoExtern(ref s), _)) if s == "fs.read"));
}

#[test]
fn prop_failure_found_and_replayed_with_same_seed() {
    let p = compile(
        r#"
prop "small positive" (x: int) where x > 0 {
    assert x < 500, "too big"
}
"#,
    );
    let seed = 0xDEAD_BEEF_CAFE_BABEu64;
    let out1 = run_prop(&p, 0, seed, 200, FUEL);
    let out2 = run_prop(&p, 0, seed, 200, FUEL);
    assert!(matches!(
        out1.result,
        Err((Trap::AssertFailed(ref m), _)) if m == "too big"
    ));
    assert_eq!(out1, out2);
    assert!(out1.output.contains("case "));
}

#[test]
fn trap_array_repeat_over_cap_before_allocating() {
    // 4e9 elements would be tens of GB; the cap traps before any allocation.
    let p = compile(
        r#"
fn main() {
    let xs = [0; 4000000000];
    let _ = xs;
}
"#,
    );
    let out = run_main(&p, "", 1_000);
    assert!(matches!(out.result, Err((Trap::Overflow, _))));
}
