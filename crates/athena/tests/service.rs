//! Host tests for `athena::service`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use athena::service::{check, highlight, run_tests, Style};

const MAIN_LOGOS: &str = r#"fn print(s: str) uses io = extern "io.print"

fn main() uses io {
  var sum = 0
  for i in 1..6 {
    sum = sum + i
    print("step " + str(i) + ": sum=" + str(sum) + "\n")
  }
}
"#;

const MATH_LOGOS: &str = r#"fn add(a: int, b: int) -> int {
  a + b
}

fn max2(a: int, b: int) -> int {
  if a > b { a } else { b }
}

test "add works" {
  assert(add(2, 3) == 5)
}

test "max2 works" {
  assert(max2(7, 3) == 7)
  assert(max2(-2, 4) == 4)
}

prop "max2 >= both" (a: int, b: int) {
  let m = max2(a, b)
  assert(m >= a)
  assert(m >= b)
}
"#;

const BROKEN_LOGOS: &str = "fn main() {\n  let x: int = \"not an int\"\n}\n";

#[test]
fn check_clean_type_error_and_syntax_error() {
    assert!(check(MAIN_LOGOS).is_empty());
    assert!(check(MATH_LOGOS).is_empty());

    let type_diags = check(BROKEN_LOGOS);
    assert_eq!(type_diags.len(), 1, "{type_diags:?}");
    assert_eq!(type_diags[0].line, 2);
    assert!(type_diags[0].col >= 3, "{type_diags:?}");
    assert!(!type_diags[0].message.is_empty());

    let syntax_diags = check("fn ( {\n");
    assert_eq!(syntax_diags.len(), 1, "{syntax_diags:?}");
    assert!(syntax_diags[0].line >= 1);
    assert!(syntax_diags[0].col >= 1);
}

#[test]
fn highlight_runs_cover_all_bytes_and_classify_tokens() {
    assert!(highlight("").is_empty());

    let src = "fn main() {\n  // note\n  let s = \"hi\"\n}\n";
    let runs = highlight(src);
    assert!(!runs.is_empty());
    assert_eq!(runs.first().unwrap().0, 0);
    assert_eq!(runs.last().unwrap().1, src.len());
    for pair in runs.windows(2) {
        assert_eq!(pair[0].1, pair[1].0);
    }
    for &(start, end, _) in &runs {
        assert!(start < end);
    }

    let has_style = |needle: &str, expected: Style| {
        let pos = src.find(needle).unwrap();
        let end = pos + needle.len();
        runs.iter()
            .any(|&(s, e, st)| s <= pos && end <= e && st == expected)
    };
    assert!(has_style("fn", Style::Keyword));
    assert!(has_style("// note", Style::Comment));
    assert!(has_style("\"hi\"", Style::String));
}

#[test]
fn run_tests_on_math_failing_and_broken() {
    let ok = run_tests(MATH_LOGOS, 1_000_000).unwrap();
    assert_eq!(ok.len(), 3, "{ok:?}");
    assert_eq!(ok[0].name, "add works");
    assert_eq!(ok[1].name, "max2 works");
    assert_eq!(ok[2].name, "max2 >= both");
    assert!(ok.iter().all(|r| r.passed), "{ok:?}");

    let fail_src = r#"
test "always fails" {
  assert(1 == 2)
}
"#;
    let failed = run_tests(fail_src, 100_000).unwrap();
    assert_eq!(failed.len(), 1);
    assert!(!failed[0].passed);
    assert!(
        failed[0].detail.contains("assertion failed"),
        "{:?}",
        failed[0]
    );

    let err = run_tests(BROKEN_LOGOS, 100_000).unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].line, 2);
}

#[test]
fn random_bytes_never_panic() {
    forall(200, 0xA7E4_A001, |rng| {
        let bytes = rng.bytes(256);
        let src = String::from_utf8_lossy(&bytes);
        let _ = check(&src);
        let runs = highlight(&src);
        if src.is_empty() {
            assert!(runs.is_empty());
        } else {
            assert_eq!(runs.first().unwrap().0, 0);
            assert_eq!(runs.last().unwrap().1, src.len());
            for pair in runs.windows(2) {
                assert_eq!(pair[0].1, pair[1].0);
            }
        }
        let _ = run_tests(&src, 1_000);
    });
}
