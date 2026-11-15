//! CLI integration tests for the `cobalt` binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_path(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("cobalt_cli_test_{pid}_{id}_{name}"))
}

fn run_cobalt(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cobalt"))
        .args(args)
        .output()
        .unwrap()
}

fn run_cobalt_stdin(args: &[&str], stdin_data: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cobalt"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin_data).unwrap();
    child.wait_with_output().unwrap()
}

const GOOD_LOGOS: &str = "fn add_const() -> int { 1 + 2 }\nfn main() -> int { add_const() }\n";
const BAD_LOGOS: &str = "fn main() -> int { true + 1 }\n";

#[test]
fn help_lists_subcommands_and_examples() {
    let out = run_cobalt(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ir "), "stdout: {stdout}");
    assert!(stdout.contains("opt "), "stdout: {stdout}");
    assert!(stdout.contains("build "), "stdout: {stdout}");
    assert!(stdout.contains("EXAMPLES:"), "stdout: {stdout}");
}

#[test]
fn version_prints_cobalt_0_1_0() {
    let out = run_cobalt(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "cobalt 0.1.0");
}

#[test]
fn ir_good_file_and_stdin() {
    let path = temp_path("good_ir.logos");
    fs::write(&path, GOOD_LOGOS).unwrap();

    let out = run_cobalt(&["ir", path.to_str().unwrap()]);
    let _ = fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fn #"), "stdout: {stdout}");
    assert!(stdout.contains("main fn#"), "stdout: {stdout}");
    assert!(stdout.contains("Bin "), "stdout: {stdout}");
    assert!(stdout.contains("Ret "), "stdout: {stdout}");

    let stdin_out = run_cobalt_stdin(&["ir", "-"], GOOD_LOGOS.as_bytes());
    assert_eq!(stdin_out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&stdin_out.stdout),
        stdout,
        "stdin IR should match file IR"
    );
}

#[test]
fn opt_good_file_prints_optimised_ir_and_stats() {
    let path = temp_path("good_opt.logos");
    fs::write(&path, GOOD_LOGOS).unwrap();

    let out = run_cobalt(&["opt", path.to_str().unwrap()]);
    let _ = fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fn #"), "stdout: {stdout}");
    assert!(stdout.contains("stats.folded "), "stdout: {stdout}");
    assert!(stdout.contains("stats.copies "), "stdout: {stdout}");
    assert!(stdout.contains("stats.removed "), "stdout: {stdout}");
    assert!(
        stdout.contains("stats.folded 1"),
        "expected constant fold in: {stdout}"
    );
}

#[test]
fn build_good_file_produces_verified_mbc() {
    let in_path = temp_path("good_build.logos");
    let out_path = temp_path("good_build.mbc");
    fs::write(&in_path, GOOD_LOGOS).unwrap();

    let out = run_cobalt(&[
        "build",
        in_path.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = fs::read(&out_path).unwrap();
    let _ = fs::remove_file(&in_path);
    let _ = fs::remove_file(&out_path);

    let module = elysium::decode(&bytes).unwrap();
    elysium::verify(&module).unwrap();

    let stdout_build = run_cobalt_stdin(&["build", "-", "-o", "-"], GOOD_LOGOS.as_bytes());
    assert_eq!(stdout_build.status.code(), Some(0));
    assert_eq!(stdout_build.stdout, bytes);
}

#[test]
fn bad_inputs_exit_1_with_cobalt_prefix() {
    let bad_path = temp_path("bad.logos");
    let out_path = temp_path("bad_out.mbc");
    fs::write(&bad_path, BAD_LOGOS).unwrap();

    for args in [
        vec!["ir", bad_path.to_str().unwrap()],
        vec!["opt", bad_path.to_str().unwrap()],
        vec![
            "build",
            bad_path.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ],
    ] {
        let out = run_cobalt(&args);
        assert_eq!(out.status.code(), Some(1), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("cobalt:"), "stderr: {stderr}");
    }

    let missing = temp_path("does_not_exist.logos");
    let out_missing = run_cobalt(&["ir", missing.to_str().unwrap()]);
    assert_eq!(out_missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_missing.stderr).contains("cobalt:"));

    let good_path = temp_path("good_unwritable.logos");
    fs::write(&good_path, GOOD_LOGOS).unwrap();
    let bad_dir_out = temp_path("no_such_dir").join("out.mbc");
    let out_unwritable = run_cobalt(&[
        "build",
        good_path.to_str().unwrap(),
        "-o",
        bad_dir_out.to_str().unwrap(),
    ]);
    assert_eq!(out_unwritable.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_unwritable.stderr).contains("cobalt:"));

    let _ = fs::remove_file(&good_path);
    let _ = fs::remove_file(&bad_path);
    let _ = fs::remove_file(&out_path);
}

#[test]
fn usage_errors_exit_2_with_cobalt_prefix() {
    let unknown = run_cobalt(&["bogus"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("cobalt:"));

    let no_args = run_cobalt(&[]);
    assert_eq!(no_args.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&no_args.stderr).contains("cobalt:"));

    let missing_out = run_cobalt(&["build", "foo.logos"]);
    assert_eq!(missing_out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_out.stderr).contains("cobalt:"));

    let missing_file = run_cobalt(&["ir"]);
    assert_eq!(missing_file.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_file.stderr).contains("cobalt:"));
}
