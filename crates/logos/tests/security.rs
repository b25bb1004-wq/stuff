//! Security review tests for `logos` lexer, parser, and type checker.
#![allow(clippy::unwrap_used)]

use logos::check::check;
use logos::lexer::lex;
use logos::parser::parse;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

fn assert_bounded_parse_check(src: &str) {
    let start = Instant::now();
    if let Ok(m) = parse(src) {
        let _ = check(&m);
    }
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "processing took too long: {:?}",
        start.elapsed()
    );
}

#[test]
fn deep_paren_nesting_rejected_without_stack_overflow() {
    let mut src = String::from("fn main() { let x = ");
    for _ in 0..500 {
        src.push('(');
    }
    src.push('1');
    for _ in 0..500 {
        src.push(')');
    }
    src.push_str(" }");
    assert!(parse(&src).is_err());
}

#[test]
fn deep_block_nesting_rejected_without_stack_overflow() {
    let mut src = String::from("fn main() ");
    for _ in 0..300 {
        src.push('{');
    }
    for _ in 0..300 {
        src.push('}');
    }
    assert!(parse(&src).is_err());
}

#[test]
fn deep_type_and_pattern_nesting_rejected() {
    let mut ty_src = String::from("fn f(x: ");
    for _ in 0..300 {
        ty_src.push('[');
    }
    ty_src.push_str("int");
    for _ in 0..300 {
        ty_src.push(']');
    }
    ty_src.push_str(") {}");
    assert!(parse(&ty_src).is_err());

    let mut pat_src = String::from("fn f(x: int?) -> int { match x { ");
    for _ in 0..300 {
        pat_src.push_str("some(");
    }
    pat_src.push('_');
    for _ in 0..300 {
        pat_src.push(')');
    }
    pat_src.push_str(" => 1, _ => 2 } }");
    assert!(parse(&pat_src).is_err());
}

#[test]
fn huge_and_edge_integer_literals_no_panic() {
    for lit in [
        "99999999999999999999999999999999999999999999999999",
        "0xffffffffffffffffffffffffffffffffffffffffffffffff",
        "0b11111111111111111111111111111111111111111111111111111111111111111",
        "9223372036854775808",
        "0x8000000000000000",
        "0x",
        "0b",
        "123abc",
    ] {
        let src = format!("fn main() {{ let x = {lit} }}");
        assert!(parse(&src).is_err(), "expected error for literal {lit}");
    }
    let min_src = "fn main() { let x = -9223372036854775808 }";
    let m = parse(min_src).unwrap();
    assert!(check(&m).is_ok());

    let div_min = "const x: int = -9223372036854775808 / -1";
    let m2 = parse(div_min).unwrap();
    assert!(check(&m2).is_err());
}

#[test]
fn long_identifiers_and_lines_bounded() {
    let ident = "a".repeat(100_000);
    let comment = "/".repeat(100_000);
    let src = format!("{comment}\nfn {ident}() {{ let {ident}_2 = 1 }}");
    let start = Instant::now();
    let m = parse(&src).unwrap();
    check(&m).unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn unterminated_strings_bytes_escapes_and_comments() {
    for bad in [
        "\"",
        "\"unterminated",
        "\"line\nbreak\"",
        "\"trailing\\",
        "\"\\x\"",
        "\"\\x4\"",
        "\"\\xff\"",
        "\"\\u\"",
        "\"\\u{\"",
        "\"\\u{}\"",
        "\"\\u{110000}\"",
        "\"\\u{d800}\"",
        "b'",
        "b''",
        "b'a",
        "b'\\",
        "b'\\x'",
        "b'\\x4",
        "b'é'",
        "b'\n'",
    ] {
        assert!(lex(bad).is_err(), "expected lex error for {bad:?}");
    }
    let comment_eof = "fn main() {} // trailing comment without newline";
    assert!(parse(comment_eof).is_ok());
}

#[test]
fn many_errors_and_garbage_inputs_no_panic() {
    let mut garbage = String::new();
    for i in 0..10_000 {
        let _ = write!(garbage, "@#$%^&*`~ \\ ! ? : ; , . {} ", i);
    }
    assert_bounded_parse_check(&garbage);

    let utf8_hostile = "fn main() { let x = \"🦀\" ; \u{0000} \u{ffff} 🚀 }";
    assert!(parse(utf8_hostile).is_err());
}

#[test]
fn max_depth_200_parens_survives_parse_and_check() {
    let mut src = String::from("fn main() { let x = ");
    for _ in 0..150 {
        src.push('(');
    }
    src.push('1');
    for _ in 0..150 {
        src.push(')');
    }
    src.push_str(" }");
    let m = parse(&src).unwrap();
    check(&m).unwrap();
}

#[test]
fn deep_binary_chain_no_stack_overflow() {
    let mut src = String::from("fn main() { let x = 0");
    for _ in 0..50_000 {
        src.push_str(" + 1");
    }
    src.push_str(" }");
    assert!(parse(&src).is_err());
}

#[test]
fn deep_pipe_chain_no_stack_overflow() {
    let mut src = String::from("fn f(x: int) -> int { x }\nfn main() { let x = 0");
    for _ in 0..50_000 {
        src.push_str(" |> f");
    }
    src.push_str(" }");
    assert!(parse(&src).is_err());
}

#[test]
fn deep_postfix_chain_no_stack_overflow() {
    let mut src = String::from("type R = { a: R }\nfn f(r: R) { let x = r");
    for _ in 0..50_000 {
        src.push_str(".a");
    }
    src.push_str(" }");
    assert!(parse(&src).is_err());
}

#[test]
fn deep_option_type_no_stack_overflow() {
    let mut src = String::from("fn f(x: int");
    for _ in 0..50_000 {
        src.push('?');
    }
    src.push_str(") {}");
    assert!(parse(&src).is_err());
}

#[test]
fn exhaustive_tuple_bool_no_hang() {
    let n = 22usize;
    let tys = vec!["bool"; n].join(", ");
    let mut arms = String::new();
    for i in 0..n {
        let mut pat = Vec::with_capacity(n);
        for j in 0..n {
            pat.push(if i == j { "true" } else { "_" });
        }
        let _ = writeln!(arms, "        ({}) => {},", pat.join(", "), i);
    }
    let src =
        format!("fn f(x: ({tys})) -> int {{\n    match x {{\n{arms}        _ => 0,\n    }}\n}}");
    let m = parse(&src).unwrap();
    let start = Instant::now();
    check(&m).unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "exhaustiveness check hung for {:?}",
        start.elapsed()
    );
}

#[test]
fn const_str_doubling_bounded_alloc() {
    let mut src = format!("const s0: str = \"{}\"\n", "a".repeat(1024));
    for i in 1..=17 {
        let _ = writeln!(src, "const s{i}: str = s{} + s{}", i - 1, i - 1);
    }
    src.push_str("fn main() -> int { len(s17) }\n");
    let m = parse(&src).unwrap();
    assert!(
        check(&m).is_err(),
        "constant string doubling past limit must be rejected instead of allocating > 64 MiB"
    );
}

#[test]
fn many_locals_and_funcs_bounded_time() {
    let mut locals_src = String::from("fn main() -> int {\n    let v0 = 0\n");
    for i in 1..15_000 {
        let _ = writeln!(locals_src, "    let v{i} = v{}", i - 1);
    }
    locals_src.push_str("    v14999\n}\n");
    let start = Instant::now();
    let m = parse(&locals_src).unwrap();
    check(&m).unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "many locals took {:?}",
        start.elapsed()
    );

    let mut funcs_src = String::new();
    for i in 0..15_000 {
        let _ = writeln!(funcs_src, "fn f{i}() -> int {{ {i} }}");
    }
    let start2 = Instant::now();
    let m2 = parse(&funcs_src).unwrap();
    check(&m2).unwrap();
    assert!(
        start2.elapsed() < Duration::from_secs(1),
        "many funcs took {:?}",
        start2.elapsed()
    );
}

#[test]
fn exhaustive_no_wildcard_bounded() {
    let n = 44usize;
    let tys = vec!["bool"; n].join(", ");
    let mut arms = String::new();
    for i in 0..(n / 2) {
        let mut pat = vec!["_"; n];
        pat[2 * i] = "true";
        pat[2 * i + 1] = "true";
        let _ = writeln!(arms, "        ({}) => {},", pat.join(", "), i);
    }
    let src = format!("fn f(x: ({tys})) -> int {{\n    match x {{\n{arms}    }}\n}}");
    let m = parse(&src).unwrap();
    let start = Instant::now();
    assert!(check(&m).is_err());
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "exhaustive_no_wildcard took {:?}",
        start.elapsed()
    );
}

#[test]
fn many_chained_calls_and_labels_bounded_time() {
    let n = 5_000usize;
    let mut chain_src = String::new();
    for i in 0..n {
        if i + 1 < n {
            let _ = writeln!(chain_src, "fn c{i}() -> int {{ c{}() }}", i + 1);
        } else {
            let _ = writeln!(chain_src, "fn c{i}() -> int {{ 1 }}");
        }
    }
    let start = Instant::now();
    let m = parse(&chain_src).unwrap();
    check(&m).unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "chained calls took {:?}",
        start.elapsed()
    );

    let mut labels_src = String::from("fn main() -> int {\n    var x = 0\n");
    for _ in 0..15_000 {
        labels_src.push_str("    if x == 0 { x = 1 } else { x = 2 }\n");
    }
    labels_src.push_str("    x\n}\n");
    let start2 = Instant::now();
    let m2 = parse(&labels_src).unwrap();
    check(&m2).unwrap();
    assert!(
        start2.elapsed() < Duration::from_secs(1),
        "many labels took {:?}",
        start2.elapsed()
    );
}
