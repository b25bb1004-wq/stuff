//! Calliope linear-time regex matcher demo.

use std::fmt::Write as _;

use calliope::Regex;

use super::{check_input, Demo, Output};

const MAX_DISPLAY_MATCHES: usize = 256;

const DEFAULT_REGEX: &str = "\
[A-Za-z_][A-Za-z0-9_]*\\([0-9]+\\)
fn main() {
  let a = calc(42)
  let b = step(7) + calc(108)
}";

/// Registered Calliope linear-time regex demo.
pub const DEMO: Demo = Demo {
    id: "calliope",
    title: "Calliope Regex",
    default_input: DEFAULT_REGEX,
    run,
};

/// Compile the first line as a Calliope regex and match it against the remaining lines.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let (raw_pat, text) = match input.find('\n') {
        Some(idx) => (&input[..idx], &input[idx + 1..]),
        None => (input, ""),
    };
    let pat = raw_pat.strip_suffix('\r').unwrap_or(raw_pat);
    if pat.is_empty() {
        return Err(String::from("first line must be a non-empty regex pattern"));
    }

    let re = Regex::new(pat).map_err(|e| format!("regex error at byte {}: {}", e.pos, e.msg))?;
    let spans = re.find_iter(text);
    let line_starts = compute_line_starts(text);

    let mut out = String::new();
    let _ = writeln!(out, "pattern: {pat}");
    for (start, end) in spans.iter().copied().take(MAX_DISPLAY_MATCHES) {
        let (line, col) = byte_to_line_col(text, &line_starts, start);
        let slice = text.get(start..end).unwrap_or("");
        let escaped = slice.replace('\r', "\\r").replace('\n', "\\n");
        let _ = writeln!(out, "{line}:{col} {escaped}");
    }
    if spans.len() > MAX_DISPLAY_MATCHES {
        let _ = writeln!(
            out,
            "... ({} more matches)",
            spans.len() - MAX_DISPLAY_MATCHES
        );
    }
    let _ = writeln!(out, "matches: {}", spans.len());

    Ok(Output::Text(out))
}

fn compute_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn byte_to_line_col(text: &str, starts: &[usize], offset: usize) -> (usize, usize) {
    let line_idx = starts.partition_point(|&s| s <= offset).saturating_sub(1);
    let line_start = starts.get(line_idx).copied().unwrap_or(0);
    let col = text
        .get(line_start..offset)
        .map_or(1, |prefix| prefix.chars().count() + 1);
    (line_idx + 1, col)
}
