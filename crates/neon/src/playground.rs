//! Logos source compilation and execution for the browser playground.

use logos::eval::Trap;
use logos::ir::Body;

/// Parse, type-check, compile, and run `main` of Logos source `src` with a `fuel` budget.
pub fn run_logos(src: &str, fuel: u64) -> String {
    let ast = match logos::parser::parse(src) {
        Ok(ast) => ast,
        Err(d) => return format_diag(src, d),
    };
    let ir = match logos::check::check(&ast) {
        Ok(ir) => ir,
        Err(d) => return format_diag(src, d),
    };
    let Some(main_fn) = ir.funcs.iter().find(|f| f.name == "main") else {
        return String::from("error: 1:1: missing required function `fn main()`");
    };
    if main_fn.params != 0 || matches!(main_fn.body, Body::Extern(_)) {
        return String::from("error: 1:1: `main` must take no parameters and cannot be extern");
    }

    let compiled = cobalt::compile(&ir);
    let outcome = cobalt::run_main(&compiled, "", fuel);
    match outcome.result {
        Ok(()) => outcome.output,
        Err((trap, span)) => {
            let (line, col) = logos::line_col(src, span.start);
            let msg = format_trap(&trap);
            let trap_line = format!("trap: {line}:{col}: {msg}");
            if outcome.output.is_empty() {
                trap_line
            } else if outcome.output.ends_with('\n') {
                format!("{}{trap_line}", outcome.output)
            } else {
                format!("{}\n{trap_line}", outcome.output)
            }
        }
    }
}

fn format_diag(src: &str, d: logos::Diag) -> String {
    let (line, col) = logos::line_col(src, d.span.start);
    format!("error: {line}:{col}: {}", d.message)
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
    fn missing_and_bad_main_are_reported_as_errors() {
        let missing = run_logos("fn helper() -> int { 1 }", 1_000);
        assert!(missing.starts_with("error: "), "{missing}");

        let params = run_logos("fn main(x: int) { }", 1_000);
        assert!(params.starts_with("error: "), "{params}");
    }

    #[test]
    fn runtime_traps_preserve_prior_output() {
        let src = r#"
fn print(s: str) uses io = extern "io.print"

fn main() uses io {
    print("before trap\n")
    let x = 10 / 0
    print(str(x))
}
"#;
        let out = run_logos(src, 10_000);
        assert!(out.starts_with("before trap\n"), "{out}");
        assert!(out.contains("trap:"), "{out}");
        assert!(out.contains("division by zero"), "{out}");
    }
}
