//! Lithium RV64 assembly kernel demo.

use std::fmt::Write as _;

use silicon::machine::Exit;

use super::{check_input, Demo, Output};

const MAX_STEPS: u64 = 2_000_000;

/// Registered Lithium assembly kernel demo.
pub const DEMO: Demo = Demo {
    id: "lithium",
    title: "Lithium Kernel",
    default_input: "2000000",
    run,
};

/// Boot the Lithium RV64 assembly kernel for up to `steps` (1..=2_000_000) and return its UART transcript.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(String::from("expected step count (1..=2000000)"));
    }
    let steps: u64 = trimmed
        .parse()
        .map_err(|_| format!("invalid step count `{trimmed}` (expected 1..={MAX_STEPS})"))?;
    if steps == 0 || steps > MAX_STEPS {
        return Err(format!("step count must be in 1..={MAX_STEPS}"));
    }

    let (exit, uart) = lithium::run(steps);
    let mut out = String::new();
    let uart_text = String::from_utf8_lossy(&uart);
    if uart_text.is_empty() {
        out.push_str("uart: (empty)\n");
    } else {
        out.push_str(&uart_text);
        if !uart_text.ends_with('\n') {
            out.push('\n');
        }
    }

    let exit_str = match exit {
        Exit::Pass => String::from("Pass"),
        Exit::Fail(code) => format!("Fail({code})"),
        Exit::OutOfSteps => format!("OutOfSteps ({steps} steps)"),
    };
    let _ = writeln!(out, "exit: {exit_str}");
    Ok(Output::Text(out))
}
