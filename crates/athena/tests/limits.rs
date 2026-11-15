//! Input-size cap on the Athena wasm boundary, and the debugger status line in `web/debug.js`.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use athena::service::{check, check_input_len, highlight, MAX_INPUT_BYTES};

const MAIN_LOGOS: &str = "fn main() -> int {\n  var x = 1\n  x + 2\n}\n";

fn extract_block<'a>(src: &'a str, start_marker: &str, open: char, close: char) -> &'a str {
    let start = src.find(start_marker).unwrap() + start_marker.len();
    let mut depth = 0usize;
    let mut begin = None;
    for (idx, ch) in src[start..].char_indices() {
        if ch == open {
            if depth == 0 {
                begin = Some(start + idx);
            }
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return &src[begin.unwrap()..=start + idx];
            }
        }
    }
    panic!("unclosed block after {start_marker}");
}

#[test]
fn oversized_input_is_refused_with_a_message() {
    assert_eq!(MAX_INPUT_BYTES, 64 * 1024);
    assert!(check_input_len(MAX_INPUT_BYTES).is_ok());
    let err = check_input_len(MAX_INPUT_BYTES + 1).unwrap_err();
    assert!(err.starts_with("input too large"), "{err}");
    assert!(err.contains("65537 bytes"), "{err}");
    assert!(err.contains("65536"), "{err}");
}

#[test]
fn normal_input_still_checks_and_highlights() {
    assert!(check_input_len(MAIN_LOGOS.len()).is_ok());
    assert!(check(MAIN_LOGOS).is_empty());
    assert!(!highlight(MAIN_LOGOS).is_empty());
}

#[test]
fn every_source_export_refuses_oversized_input_first() {
    let wasm_src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("wasm.rs"),
    )
    .unwrap();
    for export in [
        "athena_check",
        "athena_highlight",
        "athena_test",
        "athena_definition",
        "athena_hover",
        "athena_debug_start",
        "athena_debug_step",
        "athena_format",
    ] {
        let marker = format!("pub extern \"C\" fn {export}(");
        let body = extract_block(&wasm_src, &marker, '{', '}');
        assert!(
            body.contains("check_input_len("),
            "{export} must call check_input_len before reading its input"
        );
    }
}

#[test]
fn debugger_status_line_follows_session_state() {
    let js = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("web")
            .join("debug.js"),
    )
    .unwrap();
    let apply = extract_block(&js, "function apply(st)", '{', '}');
    assert!(
        apply.contains("deps.setStatus(`Paused at ${session.file}:${st.line}`"),
        "a pause must set the editor status line"
    );
    let end = extract_block(&js, "function endSession(msg, isError)", '{', '}');
    assert!(
        end.contains("deps.setStatus(msg"),
        "a stop or end must set the status line"
    );
    let drive = extract_block(&js, "function drive(mode)", '{', '}');
    assert!(
        drive.contains("deps.setStatus(`Running"),
        "a running session must set the status line"
    );
    let start = extract_block(&js, "function startSession()", '{', '}');
    assert!(
        start.contains("endSession("),
        "a new session must end the old one first"
    );
}
