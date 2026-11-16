#![allow(clippy::unwrap_used)]
//! Integration tests for the `athena` CLI binary.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("athena_cli_test_{prefix}_{pid}_{id}"));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn write_file(&self, name: &str, contents: &str) -> PathBuf {
        let file = self.path.join(name);
        fs::write(&file, contents).unwrap();
        file
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn athena_bin() -> &'static str {
    env!("CARGO_BIN_EXE_athena")
}

fn run(args: &[&str]) -> Output {
    Command::new(athena_bin()).args(args).output().unwrap()
}

fn run_stdin(args: &[&str], stdin_data: &str) -> Output {
    let mut child = Command::new(athena_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_data.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn help_and_version() {
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let help_out = stdout_str(&help);
    assert!(help_out.contains("check"));
    assert!(help_out.contains("fmt"));
    assert!(help_out.contains("test"));
    assert!(help_out.contains("EXAMPLES:"));

    let ver = run(&["--version"]);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(stdout_str(&ver).trim(), "athena 0.1.0");
}

#[test]
fn unknown_subcommand_exits_two() {
    let out = run(&["bogus"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr_str(&out).contains("athena:"));

    let empty = run(&[]);
    assert_eq!(empty.status.code(), Some(2));
    assert!(stderr_str(&empty).contains("athena:"));
}

#[test]
fn check_good_and_bad() {
    let dir = TempDir::new("check");
    let good = dir.write_file("good.logos", "fn add(a: int, b: int) -> int { a + b }\n");
    let out = run(&["check", good.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_str(&out));

    let stdin_ok = run_stdin(&["check", "-"], "fn id(x: int) -> int { x }\n");
    assert_eq!(stdin_ok.status.code(), Some(0));

    let bad = dir.write_file("bad.logos", "fn add(a: int) -> int { a + true }\n");
    let bad_path = bad.to_str().unwrap();
    let bad_out = run(&["check", bad_path]);
    assert_eq!(bad_out.status.code(), Some(1));
    let err = stderr_str(&bad_out);
    assert!(err.contains(&format!("{bad_path}:1:")), "stderr: {err}");
    assert!(err.contains(": error: "), "stderr: {err}");
    assert!(err.contains("athena:"), "stderr: {err}");

    let missing = run(&["check", dir.path.join("missing.logos").to_str().unwrap()]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr_str(&missing).contains("athena:"));
}

#[test]
fn fmt_stdout_write_and_comment_refusal() {
    let dir = TempDir::new("fmt");
    let unformatted = "fn   add(a:int,b:int)->int{a+b}\n";
    let file = dir.write_file("fmt.logos", unformatted);
    let path = file.to_str().unwrap();

    let out = run(&["fmt", path]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_str(&out));
    let printed = stdout_str(&out);
    assert_ne!(printed, unformatted);
    assert!(printed.contains("fn add(a: int, b: int) -> int"));
    assert_eq!(fs::read_to_string(&file).unwrap(), unformatted);

    let stdin_out = run_stdin(&["fmt", "-"], unformatted);
    assert_eq!(stdin_out.status.code(), Some(0));
    assert_eq!(stdout_str(&stdin_out), printed);

    let write_out = run(&["fmt", path, "--write"]);
    assert_eq!(
        write_out.status.code(),
        Some(0),
        "stderr: {}",
        stderr_str(&write_out)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), printed);

    let with_comment = "// keep me\nfn f() -> int { 1 }\n";
    let comment_file = dir.write_file("comment.logos", with_comment);
    let bad_out = run(&["fmt", comment_file.to_str().unwrap(), "--write"]);
    assert_eq!(bad_out.status.code(), Some(1));
    assert!(stderr_str(&bad_out).contains("athena:"));
    assert_eq!(fs::read_to_string(&comment_file).unwrap(), with_comment);

    let bad_stdin_write = run(&["fmt", "-", "--write"]);
    assert_eq!(bad_stdin_write.status.code(), Some(2));
    assert!(stderr_str(&bad_stdin_write).contains("athena:"));
}

#[test]
fn test_good_and_bad() {
    let dir = TempDir::new("test");
    let good_src = r#"fn add(a: int, b: int) -> int { a + b }

test "add works" {
  assert(add(2, 3) == 5)
}

prop "add commutes" (a: int, b: int) {
  if a >= -1000 && a <= 1000 && b >= -1000 && b <= 1000 {
    assert(add(a, b) == add(b, a))
  }
}
"#;
    let good = dir.write_file("good_tests.logos", good_src);
    let out = run(&["test", good.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_str(&out));
    let stdout = stdout_str(&out);
    assert!(stdout.contains("PASS add works"), "stdout: {stdout}");
    assert!(stdout.contains("PASS add commutes"), "stdout: {stdout}");

    let bad_src = r#"test "ok" {
  assert(1 == 1)
}

prop "bad prop" (x: int) {
  assert(x == x + 1)
}
"#;
    let bad = dir.write_file("bad_tests.logos", bad_src);
    let bad_out = run(&["test", bad.to_str().unwrap()]);
    assert_eq!(bad_out.status.code(), Some(1));
    let bad_stdout = stdout_str(&bad_out);
    assert!(bad_stdout.contains("PASS ok"), "stdout: {bad_stdout}");
    assert!(bad_stdout.contains("FAIL bad prop"), "stdout: {bad_stdout}");
    assert!(stderr_str(&bad_out).contains("athena:"));
}

#[test]
fn serve_names_a_missing_folder_and_a_missing_web_app() {
    let dir = TempDir::new("serve_missing");
    let out = Command::new(env!("CARGO_BIN_EXE_athena"))
        .args(["serve", "nosuchfolder"])
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stderr_str(&out), "athena: not a folder: nosuchfolder\n");
    // A real folder with no web app anywhere lists each place that was tried.
    let out = Command::new(env!("CARGO_BIN_EXE_athena"))
        .args(["serve", ".", "--web", "no-such-web"])
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = stderr_str(&out);
    assert!(err.contains("no web app; looked in: no-such-web"), "{err}");
    assert!(err.contains("target"), "{err}");
}
