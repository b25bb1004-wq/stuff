//! Integration tests for the `mica` CLI binary (`mica run` and `mica test`).
#![allow(clippy::unwrap_used)]

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn temp_mica_file(name: &str, content: &str) -> PathBuf {
    let mut path = env::temp_dir();
    path.push(format!("sinter_cli_{}_{}.mica", std::process::id(), name));
    fs::write(&path, content).unwrap();
    path
}

fn run_cli(args: &[&str], stdin_data: Option<&str>) -> (i32, String, String) {
    let exe = env!("CARGO_BIN_EXE_mica");
    let mut cmd = Command::new(exe);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(input) = stdin_data {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn cli_run_with_stdin_and_exit_zero() {
    let f = temp_mica_file(
        "run_ok",
        r#"fn print(s: str) uses io = extern "io.print"
fn read_line() -> str? uses io = extern "io.read_line"
fn main() uses io {
    match read_line() {
        some(s) => print("got:" + s),
        none => print("eof"),
    }
}
"#,
    );
    let (code, stdout, stderr) = run_cli(&["run", f.to_str().unwrap()], Some("world\n"));
    let _ = fs::remove_file(&f);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "got:world");
    assert!(stderr.is_empty());
}

#[test]
fn cli_run_trap_prints_line_col_and_exits_1() {
    let f = temp_mica_file(
        "run_trap",
        "fn main() {\n    let x = 10\n    let y = x / 0\n}\n",
    );
    let (code, stdout, stderr) = run_cli(&["run", f.to_str().unwrap()], None);
    let _ = fs::remove_file(&f);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(
        stderr.starts_with("trap: DivideByZero at 3:13"),
        "unexpected stderr: {stderr:?}"
    );
}

#[test]
fn cli_run_fuel_limit_traps() {
    let f = temp_mica_file("run_fuel", "fn main() {\n    while true {}\n}\n");
    let (code, _, stderr) = run_cli(&["run", f.to_str().unwrap(), "--fuel", "25"], None);
    let _ = fs::remove_file(&f);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("trap: OutOfFuel at "),
        "unexpected stderr: {stderr:?}"
    );
}

#[test]
fn cli_parse_and_check_errors_exit_2() {
    let f_parse = temp_mica_file("parse_err", "fn main( {\n}\n");
    let p_str = f_parse.to_str().unwrap().to_owned();
    let (code, _, stderr) = run_cli(&["run", &p_str], None);
    let _ = fs::remove_file(&f_parse);
    assert_eq!(code, 2);
    assert!(
        stderr.starts_with(&format!("{p_str}:1:")),
        "unexpected stderr: {stderr:?}"
    );

    let f_check = temp_mica_file("check_err", "fn main() {\n    let x: int = true\n}\n");
    let c_str = f_check.to_str().unwrap().to_owned();
    let (code, _, stderr) = run_cli(&["run", &c_str], None);
    let _ = fs::remove_file(&f_check);
    assert_eq!(code, 2);
    assert!(
        stderr.starts_with(&format!("{c_str}:2:")),
        "unexpected stderr: {stderr:?}"
    );
}

#[test]
fn cli_test_pass_and_fail_cases() {
    let f_pass = temp_mica_file(
        "test_pass",
        r#"fn add(a: int, b: int) -> int { a + b }
test "two plus two" { assert add(2, 2) == 4 }
prop "commutes" (a: int, b: int) where a > -1000 && a < 1000 && b > -1000 && b < 1000 {
    assert add(a, b) == add(b, a)
}
"#,
    );
    let (code, stdout, stderr) = run_cli(
        &[
            "test",
            f_pass.to_str().unwrap(),
            "--seed",
            "7",
            "--cases",
            "50",
        ],
        None,
    );
    let _ = fs::remove_file(&f_pass);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("ok two plus two\n"), "stdout: {stdout:?}");
    assert!(stdout.contains("ok commutes\n"), "stdout: {stdout:?}");
    assert!(
        stdout.trim_end().ends_with("2 passed, 0 failed"),
        "stdout: {stdout:?}"
    );

    let f_fail = temp_mica_file(
        "test_fail",
        r#"test "good" { assert 1 == 1 }
test "bad" { assert 1 == 2, "oops" }
prop "bounded" (x: int) { assert x < 100 }
"#,
    );
    let (code, stdout, _) = run_cli(
        &[
            "test",
            f_fail.to_str().unwrap(),
            "--seed",
            "1",
            "--cases",
            "100",
        ],
        None,
    );
    let _ = fs::remove_file(&f_fail);
    assert_eq!(code, 1);
    assert!(stdout.contains("ok good\n"), "stdout: {stdout:?}");
    assert!(stdout.contains("FAIL bad:"), "stdout: {stdout:?}");
    assert!(stdout.contains("FAIL bounded:"), "stdout: {stdout:?}");
    assert!(
        stdout.trim_end().ends_with("1 passed, 2 failed"),
        "stdout: {stdout:?}"
    );
}
