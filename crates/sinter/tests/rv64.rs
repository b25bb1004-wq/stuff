//! Differential and subset tests for `sinter::rv64` against Geode and Quartz.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use geode::{Heap, Host, Limits, Trap as GTrap, Value, Vm};
use mica::check::check;
use mica::ir::Program;
use mica::parser::parse;
use quartz::machine::Exit;
use sinter::compile;
use sinter::rv64::{compile_elf, emit_asm, run, CodegenError};

const STEPS: u64 = 5_000_000;
const FUEL: u64 = 10_000_000;

struct DummyHost;
impl Host for DummyHost {
    fn call(&mut self, name: &str, _: &[Value], _: &mut Heap) -> Result<Value, GTrap> {
        Err(GTrap::Host(String::from(name)))
    }
}

fn parse_ir(src: &str) -> Program {
    let ast = parse(src).unwrap_or_else(|d| panic!("parse failed: {d:?}\n{src}"));
    check(&ast).unwrap_or_else(|d| panic!("check failed: {d:?}\n{src}"))
}

fn geode_eval_entry(p: &Program, entry: &str) -> Result<i64, u32> {
    let c = compile(p);
    let mut vm = Vm::new(c.module.clone(), &[]).unwrap();
    let mut host = DummyHost;
    let limits = Limits {
        fuel: FUEL,
        max_heap_bytes: usize::MAX / 2,
        max_depth: 10_000,
    };
    match vm.call(entry, &[], &mut host, limits) {
        Ok(Value::Int(v)) => Ok(v),
        Ok(other) => panic!("expected Int return from '{entry}', got {other:?}"),
        Err(GTrap::Overflow) => Err(1),
        Err(GTrap::DivideByZero) => Err(2),
        Err(GTrap::AssertFailed(_)) => Err(3),
        Err(GTrap::StackOverflow) => Err(4),
        Err(other) => panic!("unexpected geode trap: {other:?}"),
    }
}

fn assert_matches_geode(src: &str, entry: &str) {
    let ir = parse_ir(src);
    let expected = geode_eval_entry(&ir, entry);
    let (exit, out) = run(&ir, entry, STEPS);
    match expected {
        Ok(v) => {
            assert_eq!(
                exit,
                Exit::Pass,
                "expected Pass for {entry}, out={out:?}\n{src}"
            );
            assert_eq!(out, format!("{v}\n"), "UART mismatch for {entry}\n{src}");
        }
        Err(code) => {
            assert_eq!(
                exit,
                Exit::Fail(code),
                "expected Fail({code}) for {entry}, got {exit:?}, out={out:?}\n{src}"
            );
            assert_eq!(out, format!("trap {code}\n"), "trap UART mismatch\n{src}");
        }
    }
}

#[test]
fn programs_25_plus_match_geode() {
    let cases: &[(&str, &str)] = &[
        // 1. Constant zero
        ("fn main() -> int { 0 }", "main"),
        // 2. Positive and negative literals
        ("fn main() -> int { -1234567890123 }", "main"),
        // 3. i64::MAX
        ("fn main() -> int { 9223372036854775807 }", "main"),
        // 4. i64::MIN
        ("fn main() -> int { -9223372036854775807 - 1 }", "main"),
        // 5. Fibonacci (recursive)
        (
            "fn fib(n: int) -> int { if n <= 1 { n } else { fib(n - 1) + fib(n - 2) } }
             fn main() -> int { fib(12) }",
            "main",
        ),
        // 6. Iterative Fibonacci in while loop
        (
            "fn main() -> int {
                var a = 0
                var b = 1
                var i = 0
                while i < 20 {
                    let t = a + b
                    a = b
                    b = t
                    i = i + 1
                }
                a
            }",
            "main",
        ),
        // 7. GCD (Euclidean algorithm)
        (
            "fn gcd(a: int, b: int) -> int { if b == 0 { a } else { gcd(b, a % b) } }
             fn main() -> int { gcd(1071, 462) * 100 + gcd(48, 18) }",
            "main",
        ),
        // 8. Factorial normal
        (
            "fn fact(n: int) -> int { if n <= 1 { 1 } else { n * fact(n - 1) } }
             fn main() -> int { fact(12) }",
            "main",
        ),
        // 9. Factorial overflow trap
        (
            "fn fact(n: int) -> int { if n <= 1 { 1 } else { n * fact(n - 1) } }
             fn main() -> int { fact(25) }",
            "main",
        ),
        // 10. For loop range sum
        (
            "fn main() -> int {
                var s = 0
                for i in 1..101 { s = s + i }
                s
            }",
            "main",
        ),
        // 11. Loop with break and continue
        (
            "fn main() -> int {
                var s = 0
                var i = 0
                while i < 20 {
                    i = i + 1
                    if i % 2 == 0 { continue }
                    if i > 15 { break }
                    s = s + i
                }
                s
            }",
            "main",
        ),
        // 12. Nested function calls
        (
            "fn sq(x: int) -> int { x * x }
             fn sum_sq(a: int, b: int) -> int { sq(a) + sq(b) }
             fn main() -> int { sum_sq(3, 4) + sum_sq(-5, 12) }",
            "main",
        ),
        // 13. Mutual recursion
        (
            "fn is_even(n: int) -> bool { if n == 0 { true } else { is_odd(n - 1) } }
             fn is_odd(n: int) -> bool { if n == 0 { false } else { is_even(n - 1) } }
             fn main() -> int {
                var c = 0
                if is_even(14) { c = c + 1 }
                if is_odd(15) { c = c + 2 }
                if !is_odd(10) { c = c + 4 }
                c
             }",
            "main",
        ),
        // 14. Short-circuit && prevents div-by-zero
        (
            "fn main() -> int {
                let d = 0
                if d != 0 && (10 / d) > 1 { 1 } else { 42 }
             }",
            "main",
        ),
        // 15. Short-circuit || prevents overflow / div-by-zero
        (
            "fn main() -> int {
                let d = 0
                if d == 0 || (10 / d) > 1 { 77 } else { 0 }
             }",
            "main",
        ),
        // 16. Bool results converted through Int
        (
            "fn b2i(b: bool) -> int { if b { 1 } else { 0 } }
             fn cmp(a: int, b: int) -> int {
                b2i(a == b) * 1 + b2i(a != b) * 2 + b2i(a < b) * 4 +
                b2i(a <= b) * 8 + b2i(a > b) * 16 + b2i(a >= b) * 32
             }
             fn main() -> int { cmp(3, 5) * 100 + cmp(5, 5) * 10 + cmp(7, 5) }",
            "main",
        ),
        // 17. Negative division and remainder truncation toward zero
        (
            "fn main() -> int { (-7 / 2) * 1000 + (-7 % 2) * 100 + (7 / -2) * 10 + (7 % -2) }",
            "main",
        ),
        // 18. 8 parameters
        (
            "fn sum8(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int) -> int {
                a + b * 2 + c * 3 + d * 4 + e * 5 + f * 6 + g * 7 + h * 8
             }
             fn main() -> int { sum8(1, 2, 3, 4, 5, 6, 7, 8) }",
            "main",
        ),
        // 19. Unit-returning helper function with assert inside
        (
            "fn check_pos(x: int) { assert x > 0 }
             fn main() -> int { check_pos(9); 99 }",
            "main",
        ),
        // 20. Trap 1: Add overflow
        (
            "fn add(a: int, b: int) -> int { a + b }
             fn main() -> int { add(9223372036854775807, 1) }",
            "main",
        ),
        // 21. Trap 1: Sub overflow
        (
            "fn sub(a: int, b: int) -> int { a - b }
             fn main() -> int { sub(-9223372036854775807 - 1, 1) }",
            "main",
        ),
        // 22. Trap 1: Neg overflow on i64::MIN
        (
            "fn neg(x: int) -> int { -x }
             fn main() -> int { neg(-9223372036854775807 - 1) }",
            "main",
        ),
        // 23. Trap 1: Mul overflow
        (
            "fn mul(a: int, b: int) -> int { a * b }
             fn main() -> int { mul(4611686018427387904, 2) }",
            "main",
        ),
        // 24. Trap 2: Divide by zero
        (
            "fn div(a: int, b: int) -> int { a / b }
             fn main() -> int { div(10, 0) }",
            "main",
        ),
        // 25. Trap 2: Remainder by zero
        (
            "fn rem(a: int, b: int) -> int { a % b }
             fn main() -> int { rem(10, 0) }",
            "main",
        ),
        // 26. i64::MIN / -1 trap
        (
            "fn div(a: int, b: int) -> int { a / b }
             fn main() -> int { div(-9223372036854775807 - 1, -1) }",
            "main",
        ),
        // 27. i64::MIN % -1 trap
        (
            "fn rem(a: int, b: int) -> int { a % b }
             fn main() -> int { rem(-9223372036854775807 - 1, -1) }",
            "main",
        ),
        // 28. Trap 3: Failed assert
        ("fn main() -> int { assert 2 + 2 == 5, \"bad\"; 0 }", "main"),
        // 29. Trap 4: Stack overflow (> 10_000 depth)
        (
            "fn rec(n: int) -> int { rec(n + 1) }
             fn main() -> int { rec(0) }",
            "main",
        ),
        // 30. Deep recursion just under limit (9_998) succeeds
        (
            "fn down(n: int) -> int { if n == 0 { 0 } else { 1 + down(n - 1) } }
             fn main() -> int { down(9998) }",
            "main",
        ),
        // 31. Deep recursion at exact overflow boundary (9_999) traps
        (
            "fn down(n: int) -> int { if n == 0 { 0 } else { 1 + down(n - 1) } }
             fn main() -> int { down(9999) }",
            "main",
        ),
        // 32. Custom entry point name (not main)
        (
            "fn helper(x: int) -> int { x * 3 + 1 }
             fn entry_fn() -> int { helper(13) }",
            "entry_fn",
        ),
        // 33. Bool equality and inequality
        (
            "fn eqb(a: bool, b: bool) -> bool { a == b }
             fn neb(a: bool, b: bool) -> bool { a != b }
             fn main() -> int {
                if eqb(true, true) && neb(true, false) && !eqb(false, true) { 123 } else { 0 }
             }",
            "main",
        ),
    ];

    assert!(cases.len() >= 25);
    for (src, entry) in cases {
        assert_matches_geode(src, entry);
    }
}

#[test]
fn large_frame_more_than_260_regs() {
    let mut vars = String::from("var acc = 0\n");
    for i in 0..270 {
        vars.push_str(&format!("let v{i} = {i}\nacc = acc + v{i}\n"));
    }
    let src = format!("fn main() -> int {{\n{vars}acc\n}}");
    assert_matches_geode(&src, "main");
}

#[test]
fn trap_also_matches_sinter_run_main() {
    let cases = [
        ("fn main() { let _ = 9223372036854775807 + 1 }", 1u32),
        ("fn main() { let _ = 1 / 0 }", 2u32),
        ("fn main() { assert false }", 3u32),
        ("fn r() { r() }\nfn main() { r() }", 4u32),
    ];
    for (src, code) in cases {
        let ir = parse_ir(src);
        assert_eq!(
            emit_asm(&ir, "main"),
            Err(CodegenError::NoEntry("main".into()))
        );
        let out = sinter::run_main(&compile(&ir), "", FUEL);
        assert!(out.result.is_err());

        let int_src = format!("{src}\nfn run_entry() -> int {{ main(); 0 }}");
        let int_ir = parse_ir(&int_src);
        let (exit, uart) = run(&int_ir, "run_entry", STEPS);
        assert_eq!(exit, Exit::Fail(code));
        assert_eq!(uart, format!("trap {code}\n"));
    }

    let inf = parse_ir("fn main() -> int { while true {}; 0 }");
    let (exit, uart) = run(&inf, "main", 5000);
    assert_eq!(exit, Exit::OutOfSteps);
    assert_eq!(uart, "");
}

#[test]
fn unsupported_constructs_return_named_errors() {
    let unsupported_cases: &[(&str, &str)] = &[
        // Types
        ("fn f(x: byte) -> int { 0 }\nfn main() -> int { 0 }", "Byte"),
        ("fn f(x: str) -> int { 0 }\nfn main() -> int { 0 }", "Str"),
        (
            "fn f(x: [int]) -> int { 0 }\nfn main() -> int { 0 }",
            "Array",
        ),
        (
            "fn f(x: (int, int)) -> int { 0 }\nfn main() -> int { 0 }",
            "Tuple",
        ),
        ("fn f(x: int?) -> int { 0 }\nfn main() -> int { 0 }", "Opt"),
        (
            "type P = { x: int }\nfn f(p: P) -> int { p.x }\nfn main() -> int { 0 }",
            "Named",
        ),
        (
            "fn f(g: fn(int) -> int) -> int { g(1) }\nfn main() -> int { 0 }",
            "Fn",
        ),
        ("fn f(u: unit) -> int { 0 }\nfn main() -> int { 0 }", "Unit"),
        // > 8 params
        (
            "fn f(a:int,b:int,c:int,d:int,e:int,g:int,h:int,i:int,j:int) -> int { a }
             fn main() -> int { 0 }",
            "parameters",
        ),
        // Extern
        (
            "fn p(s: str) uses io = extern \"io.print\"\nfn main() -> int { 0 }",
            "extern",
        ),
        // Wrapping and bitwise operators
        ("fn main() -> int { 1 +% 2 }", "AddWrap"),
        ("fn main() -> int { 1 -% 2 }", "SubWrap"),
        ("fn main() -> int { 1 *% 2 }", "MulWrap"),
        ("fn main() -> int { 1 & 2 }", "BitAnd"),
        ("fn main() -> int { 1 | 2 }", "BitOr"),
        ("fn main() -> int { 1 ^ 2 }", "BitXor"),
        ("fn main() -> int { 1 << 2 }", "Shl"),
        ("fn main() -> int { 8 >> 1 }", "Shr"),
        // Unsupported instructions / literals
        ("fn main() -> int { int(b'a') }", "Byte"),
        ("fn main() -> int { let s = \"hi\"; 0 }", "Str"),
        (
            "fn id(x: int) -> int { x }\nfn main() -> int { let f = id; f(1) }",
            "Fn",
        ),
        ("fn main() -> int { abs(-5) }", "Builtin"),
        ("fn main() -> int { let a = [1, 2]; 0 }", "Array"),
        ("fn main() -> int { let a = [0; 3]; 0 }", "Array"),
        ("fn main() -> int { let t = (1, 2); t.0 }", "Tuple"),
        ("fn main() -> int { let o = some(1); 0 }", "Opt"),
        ("fn main() -> int { let o: int? = none; 0 }", "Opt"),
    ];

    for (src, needle) in unsupported_cases {
        let ir = parse_ir(src);
        let err = emit_asm(&ir, "main").expect_err(&format!("expected Unsupported for:\n{src}"));
        match err {
            CodegenError::Unsupported(msg) => {
                assert!(
                    msg.contains(needle),
                    "expected error '{msg}' to mention '{needle}' for:\n{src}"
                );
            }
            other => panic!("expected Unsupported, got {other:?} for:\n{src}"),
        }
        assert!(compile_elf(&ir, "main").is_err());
    }
}

#[test]
fn random_integer_programs_match_geode_property() {
    forall(30, 0xCAFE_BABE, |r| {
        let src = gen_random_int_prog(r);
        assert_matches_geode(&src, "main");
    });
}

fn gen_random_int_prog(r: &mut Rng) -> String {
    let e1 = gen_expr(r, 3);
    let e2 = gen_expr(r, 3);
    let cond = gen_bool(r, 2);
    format!(
        "fn helper(x: int, y: int) -> int {{
            if {cond} {{ x + y }} else {{ x - y }}
        }}
        fn main() -> int {{
            var a = {e1}
            let b = {e2}
            var i = 0
            while i < 4 {{
                a = a + helper(i, b)
                i = i + 1
            }}
            a
        }}"
    )
}

fn gen_expr(r: &mut Rng, depth: u32) -> String {
    if depth == 0 || r.chance(1, 3) {
        let vals = [
            0i64,
            1,
            -1,
            2,
            -3,
            7,
            -100,
            1000,
            i64::MAX / 4,
            i64::MIN / 4,
        ];
        return format!("{}", r.pick(&vals));
    }
    let a = gen_expr(r, depth - 1);
    let b = gen_expr(r, depth - 1);
    match r.below(6) {
        0 => format!("({a} + {b})"),
        1 => format!("({a} - {b})"),
        2 => format!("({a} * {b})"),
        3 => format!("({a} / {b})"),
        4 => format!("({a} % {b})"),
        _ => format!("(-{a})"),
    }
}

fn gen_bool(r: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        let a = gen_expr(r, 1);
        let b = gen_expr(r, 1);
        let op = r.pick(&["==", "!=", "<", "<=", ">", ">="]);
        return format!("({a} {op} {b})");
    }
    let l = gen_bool(r, depth - 1);
    let ri = gen_bool(r, depth - 1);
    match r.below(3) {
        0 => format!("({l} && {ri})"),
        1 => format!("({l} || {ri})"),
        _ => format!("(!{l})"),
    }
}
