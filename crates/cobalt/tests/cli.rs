//! CLI tests for `cobalt check` and `cobalt fmt`, and the exit-code and prefix rules.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_file(name: &str, content: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("cobalt_cli_{}_{id}_{name}", std::process::id()));
    fs::write(&path, content).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cobalt"))
        .args(args)
        .output()
        .unwrap()
}

fn run_stdin(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cobalt"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn code(out: &Output) -> Option<i32> {
    out.status.code()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const GOOD: &str = "fn add_const() -> int { 1 + 2 }\nfn main() -> int { add_const() }\n";
const BAD: &str = "fn main() -> int { true + 1 }\n";
const MESSY: &str = "fn main()->int{1+2}\n";

#[test]
fn check_good_file_exits_0_and_prints_nothing() {
    let f = temp_file("check_good.logos", GOOD);
    let out = run(&["check", f.to_str().unwrap()]);
    assert_eq!(code(&out), Some(0), "stderr: {}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn check_bad_file_exits_1_with_prefix_and_position() {
    let f = temp_file("check_bad.logos", BAD);
    let out = run(&["check", f.to_str().unwrap()]);
    assert_eq!(code(&out), Some(1));
    let err = text(&out.stderr);
    assert!(err.starts_with("cobalt: "), "stderr: {err}");
    assert!(err.contains(":1:"), "stderr: {err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn check_reads_stdin_with_dash() {
    let out = run_stdin(&["check", "-"], BAD);
    assert_eq!(code(&out), Some(1));
    assert!(text(&out.stderr).starts_with("cobalt: <stdin>:1:"));
    let ok = run_stdin(&["check", "-"], GOOD);
    assert_eq!(code(&ok), Some(0));
}

#[test]
fn check_missing_file_is_a_runtime_error_exit_1() {
    let out = run(&["check", "/no/such/cobalt_file.logos"]);
    assert_eq!(code(&out), Some(1));
    assert!(text(&out.stderr).starts_with("cobalt: "));
}

#[test]
fn fmt_prints_canonical_source_and_is_idempotent() {
    let f = temp_file("fmt_messy.logos", MESSY);
    let out = run(&["fmt", f.to_str().unwrap()]);
    assert_eq!(code(&out), Some(0), "stderr: {}", text(&out.stderr));
    let once = text(&out.stdout);
    let ast = logos::parser::parse(MESSY).unwrap();
    assert_eq!(once, logos::print::print(&ast));
    assert_ne!(once, MESSY);
    let again = temp_file("fmt_again.logos", &once);
    let out2 = run(&["fmt", again.to_str().unwrap()]);
    assert_eq!(text(&out2.stdout), once);
}

#[test]
fn fmt_refuses_comments_and_keeps_the_file() {
    let src = "// keep me\nfn main() -> int { 1 }\n";
    let f = temp_file("fmt_comment.logos", src);
    let out = run(&["fmt", f.to_str().unwrap()]);
    assert_eq!(code(&out), Some(1));
    let err = text(&out.stderr);
    assert!(err.starts_with("cobalt: "), "stderr: {err}");
    assert!(err.contains("comments"), "stderr: {err}");
    assert!(out.stdout.is_empty());
    assert_eq!(fs::read_to_string(&f).unwrap(), src);
}

#[test]
fn fmt_parse_error_exits_1() {
    let out = run_stdin(&["fmt", "-"], "fn main( {\n");
    assert_eq!(code(&out), Some(1));
    assert!(text(&out.stderr).starts_with("cobalt: <stdin>:"));
}

#[test]
fn usage_errors_exit_2_with_prefix() {
    for args in [
        vec!["check"],
        vec!["fmt"],
        vec!["check", "a.logos", "b.logos"],
        vec!["fmt", "--bogus"],
        vec!["frobnicate"],
    ] {
        let out = run(&args);
        assert_eq!(code(&out), Some(2), "args {args:?}");
        assert!(text(&out.stderr).starts_with("cobalt: "), "args {args:?}");
    }
}

#[test]
fn help_and_version_exit_0() {
    let help = run(&["--help"]);
    assert_eq!(code(&help), Some(0));
    let help_text = text(&help.stdout);
    assert!(help_text.contains("check FILE.logos"));
    assert!(help_text.contains("fmt FILE.logos"));
    let version = run(&["--version"]);
    assert_eq!(code(&version), Some(0));
    assert_eq!(text(&version.stdout).trim(), "cobalt 0.1.0");
}
