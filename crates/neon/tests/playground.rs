//! Tests for `neon::playground::run_logos`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use neon::playground::run_logos;

const EXAMPLE_HELLO: &str = r#"
fn print(s: str) uses io = extern "io.print"

fn main() uses io {
    print("hello from logos\n")
}
"#;

const EXAMPLE_LOOP: &str = r#"
fn print(s: str) uses io = extern "io.print"

fn main() uses io {
    var sum = 0
    for i in 1..11 {
        sum = sum + i * i
    }
    print("sum of squares 1..10 = " + str(sum) + "\n")
}
"#;

const EXAMPLE_ERROR: &str = r#"
fn main() {
    let x: int = "not an int"
}
"#;

#[test]
fn examples_produce_expected_output() {
    assert_eq!(run_logos(EXAMPLE_HELLO, 100_000), "hello from logos\n");
    assert_eq!(
        run_logos(EXAMPLE_LOOP, 100_000),
        "sum of squares 1..10 = 385\n"
    );
    let err = run_logos(EXAMPLE_ERROR, 100_000);
    assert!(
        err.starts_with("error: "),
        "expected diagnostic error line, got: {err}"
    );
}

#[test]
fn syntax_error_reports_line_one() {
    let out = run_logos("fn main( { }", 100_000);
    assert!(
        out.starts_with("error: 1:"),
        "expected `error: 1:...`, got: {out}"
    );
}

#[test]
fn endless_loop_stops_at_fuel_limit() {
    let src = r#"
fn main() {
    var i = 0
    while true {
        i = i + 1
    }
}
"#;
    let out = run_logos(src, 1_000);
    assert!(
        out.contains("fuel"),
        "expected fuel trap message, got: {out}"
    );
}

#[test]
fn random_bytes_never_panic() {
    forall(200, 0x7101_0605, |rng| {
        let bytes = rng.bytes(128);
        let src = String::from_utf8_lossy(&bytes);
        let _ = run_logos(&src, 5_000);
    });
}
