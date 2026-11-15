//! Host-testable IDE services for Logos: diagnostics, syntax highlighting, and test execution.

pub use calliope::Style;
use logos::eval::{Outcome, Trap};

const PROP_SEED: u64 = 1;
const PROP_CASES: u32 = 100;

/// A 1-based source diagnostic produced by parsing or type-checking Logos code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// 1-based start line.
    pub line: u32,
    /// 1-based start column.
    pub col: u32,
    /// 1-based end line.
    pub end_line: u32,
    /// 1-based end column.
    pub end_col: u32,
    /// Human-readable error message.
    pub message: String,
}

/// Outcome of running a single Logos unit test or property test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    /// Name or description of the test or property.
    pub name: String,
    /// Whether the test passed without trapping.
    pub passed: bool,
    /// Failure detail or captured output.
    pub detail: String,
}

/// Parse and type-check Logos source `src`, returning any diagnostics (empty when clean).
pub fn check(src: &str) -> Vec<Diagnostic> {
    let ast = match logos::parser::parse(src) {
        Ok(ast) => ast,
        Err(d) => return vec![diag_from_logos(src, d)],
    };
    match logos::check::check(&ast) {
        Ok(_) => Vec::new(),
        Err(d) => vec![diag_from_logos(src, d)],
    }
}

/// Compute contiguous byte-range runs `(start, end, Style)` covering all of `src` in order.
pub fn highlight(src: &str) -> Vec<(usize, usize, Style)> {
    let styles = calliope::highlight("x.logos", src);
    let Some(&first) = styles.first() else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    let mut start = 0usize;
    let mut current = first;
    for (idx, &style) in styles.iter().enumerate().skip(1) {
        if style != current {
            runs.push((start, idx, current));
            start = idx;
            current = style;
        }
    }
    runs.push((start, styles.len(), current));
    runs
}

/// Compile `src` and run every unit test followed by every property test in source order.
pub fn run_tests(src: &str, fuel: u64) -> Result<Vec<TestResult>, Vec<Diagnostic>> {
    let ast = logos::parser::parse(src).map_err(|d| vec![diag_from_logos(src, d)])?;
    let ir = logos::check::check(&ast).map_err(|d| vec![diag_from_logos(src, d)])?;
    let compiled = cobalt::compile(&ir);

    let total = compiled.tests.len().saturating_add(compiled.props.len());
    let mut results = Vec::with_capacity(total);

    for (idx, (name, _)) in compiled.tests.iter().enumerate() {
        let outcome = cobalt::run_test(&compiled, idx, fuel);
        results.push(outcome_to_result(src, name, outcome));
    }

    for (idx, prop) in compiled.props.iter().enumerate() {
        let outcome = cobalt::run_prop(&compiled, idx, PROP_SEED, PROP_CASES, fuel);
        results.push(outcome_to_result(src, &prop.name, outcome));
    }

    Ok(results)
}

fn clamp_offset(src: &str, pos: u32) -> u32 {
    let mut idx = (pos as usize).min(src.len());
    while idx > 0 && !src.is_char_boundary(idx) {
        idx -= 1;
    }
    u32::try_from(idx).unwrap_or(u32::MAX)
}

fn diag_from_logos(src: &str, d: logos::Diag) -> Diagnostic {
    let start = clamp_offset(src, d.span.start);
    let end = clamp_offset(src, d.span.end.max(start));
    let (line, col) = logos::line_col(src, start);
    let (end_line, end_col) = logos::line_col(src, end);
    Diagnostic {
        line,
        col,
        end_line,
        end_col,
        message: d.message,
    }
}

fn outcome_to_result(src: &str, name: &str, outcome: Outcome) -> TestResult {
    match outcome.result {
        Ok(()) => TestResult {
            name: name.to_owned(),
            passed: true,
            detail: outcome.output,
        },
        Err((trap, span)) => {
            let start = clamp_offset(src, span.start);
            let (line, col) = logos::line_col(src, start);
            let trap_msg = format!("{line}:{col}: {}", format_trap(&trap));
            let trimmed = outcome.output.trim();
            let detail = if trimmed.is_empty() {
                trap_msg
            } else {
                format!("{trap_msg} | {trimmed}")
            };
            TestResult {
                name: name.to_owned(),
                passed: false,
                detail,
            }
        }
    }
}

fn format_trap(trap: &Trap) -> String {
    match trap {
        Trap::OutOfFuel => String::from("out of fuel (fuel limit reached)"),
        Trap::DivideByZero => String::from("division by zero"),
        Trap::Overflow => String::from("integer overflow"),
        Trap::IndexOutOfRange => String::from("index out of range"),
        Trap::StackOverflow => String::from("stack overflow"),
        Trap::AssertFailed(msg) => format!("assertion failed: {msg}"),
        Trap::NoExtern(name) => format!("unknown host function: {name}"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn utf8_multibyte_comment_and_string_highlight_preserve_byte_ranges() {
        let src = "// café — note\nlet s = \"αβγ\"\n";
        let runs = highlight(src);
        assert_eq!(runs.first().unwrap().0, 0);
        assert_eq!(runs.last().unwrap().1, src.len());
        for (start, end, _) in &runs {
            assert!(src.is_char_boundary(*start));
            assert!(src.is_char_boundary(*end));
        }
    }

    #[test]
    fn failing_property_reports_failure_detail() {
        let src = "prop \"bad prop\" (x: int) {\n  assert(x == x + 1)\n}\n";
        let results = run_tests(src, 10_000).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(!results[0].detail.is_empty());
    }
}
