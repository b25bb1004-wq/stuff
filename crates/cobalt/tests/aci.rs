// Agent-interface tests for the cobalt and logos binaries: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const COBALT: &str = env!("CARGO_BIN_EXE_cobalt");
const LOGOS: &str = env!("CARGO_BIN_EXE_logos");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const GOOD: &str = "fn add_const() -> int { 1 + 2 }\nfn main() -> int { add_const() }\n";
/// The type error is on line 3.
const BAD_CHECK: &str = "fn helper() -> int { 1 }\nfn main() -> int {\n    true + 1\n}\n";
const TRAP_RUN: &str = "fn main() {\n    let x = 10\n    let y = x / 0\n}\n";
const TESTS: &str = "test \"good\" { assert 1 == 1 }\ntest \"bad\" { assert 1 == 2, \"oops\" }\nprop \"bounded\" (x: int) { assert x < 100 }\n";

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cobalt_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn file(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.join(rel);
        fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(bin: &str, args: &[&str]) -> Output {
    Command::new(bin).args(args).output().unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The path with forward slashes, as the envelope spells every path.
fn fwd(path: &Path) -> String {
    s(path).replace('\\', "/")
}

/// Parse stdout as exactly one envelope and validate it.
fn envelope(output: &Output) -> Json {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(stdout.matches('\n').count(), 1, "not one line: {stdout:?}");
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    value
}

fn data(output: &Output, exit: i32) -> Json {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

fn fail(output: &Output, exit: i32, cmd: &str) -> Json {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    value.get("error").unwrap().clone()
}

fn error_code(error: &Json) -> &str {
    error.get("code").and_then(Json::as_str).unwrap()
}

fn names(manifest: &Json) -> Vec<&str> {
    manifest
        .get("commands")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|c| c.get("name").and_then(Json::as_str).unwrap())
        .collect()
}

fn describe(bin: &str) -> Json {
    let out = run(bin, &["--describe"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    manifest
}

// ---- cobalt ----

#[test]
fn cobalt_describe_is_a_valid_manifest() {
    let m = describe(COBALT);
    assert_eq!(names(&m), ["check", "fmt", "ir", "opt", "build"]);
}

#[test]
fn cobalt_check_json_is_empty_diagnostics() {
    let t = Temp::new("check");
    let good = t.file("good.logos", GOOD);
    let d = data(&run(COBALT, &["--json", "check", s(&good)]), 0);
    assert_eq!(d.get("count").and_then(Json::as_i64), Some(0));
    assert_eq!(
        d.get("diagnostics").and_then(Json::as_array).unwrap().len(),
        0
    );
}

#[test]
fn cobalt_check_compile_error_is_a_diagnostic_not_a_failure() {
    let t = Temp::new("checkerr");
    let bad = t.file("bad.logos", BAD_CHECK);
    let d = data(&run(COBALT, &["--json", "check", s(&bad)]), 1);
    assert_eq!(d.get("count").and_then(Json::as_i64), Some(1));
    assert_eq!(d.get("clean").and_then(Json::as_bool), Some(false));
    let first = &d.get("diagnostics").and_then(Json::as_array).unwrap()[0];
    assert_eq!(first.get("line").and_then(Json::as_i64), Some(3));
    assert_eq!(first.get("severity").and_then(Json::as_str), Some("error"));
}

#[test]
fn cobalt_build_compile_error_is_invalid_and_points_at_check() {
    let t = Temp::new("builderr");
    let bad = t.file("bad.logos", BAD_CHECK);
    let out_file = t.join("x.mbc");
    let error = fail(
        &run(COBALT, &["--json", "build", s(&bad), "-o", s(&out_file)]),
        1,
        "build",
    );
    assert_eq!(error_code(&error), "invalid");
    assert!(error
        .get("message")
        .and_then(Json::as_str)
        .unwrap()
        .contains(":3:"));
    assert!(error
        .get("hint")
        .and_then(Json::as_str)
        .unwrap()
        .contains("--json check"));
}

#[test]
fn cobalt_check_missing_file_is_not_found() {
    let t = Temp::new("checkmissing");
    let out = run(COBALT, &["--json", "check", s(&t.join("nope.logos"))]);
    assert_eq!(error_code(&fail(&out, 1, "check")), "not_found");
}

#[test]
fn cobalt_build_writes_file_and_reports_path_and_bytes() {
    let t = Temp::new("build");
    let good = t.file("good.logos", GOOD);
    let out_file = t.join("out file.mbc");
    let d = data(
        &run(COBALT, &["--json", "build", s(&good), "-o", s(&out_file)]),
        0,
    );
    assert_eq!(
        d.get("path").and_then(Json::as_str),
        Some(fwd(&out_file).as_str())
    );
    let size = fs::metadata(&out_file).unwrap().len();
    assert_eq!(d.get("bytes").and_then(Json::as_i64), Some(size as i64));
}

#[test]
fn cobalt_build_to_stdout_under_json_is_usage() {
    let t = Temp::new("buildstdout");
    let good = t.file("good.logos", GOOD);
    let out = run(COBALT, &["--json", "build", s(&good), "-o", "-"]);
    let error = fail(&out, 2, "build");
    assert_eq!(error_code(&error), "usage");
}

#[test]
fn cobalt_fmt_ir_and_opt_return_capped_text() {
    let t = Temp::new("text");
    let good = t.file("good.logos", GOOD);
    let fmt = data(&run(COBALT, &["--json", "fmt", s(&good)]), 0);
    assert!(fmt
        .get("text")
        .and_then(Json::as_str)
        .unwrap()
        .contains("fn main"));
    let ir = data(&run(COBALT, &["--json", "ir", s(&good)]), 0);
    assert!(ir.get("text").and_then(Json::as_str).is_some());
    let opt = data(&run(COBALT, &["--json", "opt", s(&good)]), 0);
    let stats = opt.get("stats").unwrap();
    for key in ["folded", "copies", "removed"] {
        assert!(stats.get(key).and_then(Json::as_i64).is_some(), "{key}");
    }
}

#[test]
fn cobalt_json_unknown_subcommand_is_usage() {
    let out = run(COBALT, &["--json", "nosuch"]);
    let error = fail(&out, 2, "");
    assert_eq!(error_code(&error), "usage");
}

#[test]
fn cobalt_text_mode_unchanged() {
    let t = Temp::new("cobalttext");
    let good = t.file("good.logos", GOOD);
    let check = run(COBALT, &["check", s(&good)]);
    assert_eq!(check.status.code(), Some(0));
    assert!(check.stdout.is_empty());
    let bad = t.file("bad.logos", BAD_CHECK);
    let err = run(COBALT, &["check", s(&bad)]);
    assert_eq!(err.status.code(), Some(1));
    assert!(String::from_utf8(err.stderr)
        .unwrap()
        .starts_with("cobalt: "));
    assert_eq!(
        String::from_utf8(run(COBALT, &["--version"]).stdout).unwrap(),
        "cobalt 0.1.0\n"
    );
}

// ---- logos ----

#[test]
fn logos_describe_is_a_valid_manifest() {
    let m = describe(LOGOS);
    assert_eq!(names(&m), ["run", "test"]);
}

#[test]
fn logos_run_json_returns_stdout_and_exit() {
    let t = Temp::new("run");
    let prog = t.file("ok.logos", "fn main() {\n    let x = 1 + 2\n}\n");
    let d = data(&run(LOGOS, &["--json", "run", s(&prog)]), 0);
    assert_eq!(d.get("exit").and_then(Json::as_i64), Some(0));
    assert_eq!(d.get("stop").and_then(Json::as_str), Some("exit"));
    assert_eq!(d.get("stdout").and_then(Json::as_str), Some(""));
    assert!(d.get("duration_ms").and_then(Json::as_i64).is_some());
}

#[test]
fn logos_run_trap_is_invalid_with_line_and_col() {
    let t = Temp::new("trap");
    let prog = t.file("trap.logos", TRAP_RUN);
    let out = run(LOGOS, &["--json", "run", s(&prog)]);
    let error = fail(&out, 1, "run");
    assert_eq!(error_code(&error), "invalid");
    let message = error.get("message").and_then(Json::as_str).unwrap();
    assert!(message.contains("DivideByZero at 3:13"), "{message}");
}

#[test]
fn logos_compile_error_is_invalid_with_line() {
    let t = Temp::new("parse");
    let prog = t.file("bad.logos", "fn main() {\n  let x = \n}\n");
    let out = run(LOGOS, &["--json", "run", s(&prog)]);
    let error = fail(&out, 2, "run");
    assert_eq!(error_code(&error), "invalid");
    let message = error.get("message").and_then(Json::as_str).unwrap();
    assert!(message.contains(":3:"), "{message}");
}

#[test]
fn logos_test_json_lists_each_result() {
    let t = Temp::new("tests");
    let prog = t.file("t.logos", TESTS);
    let d = data(&run(LOGOS, &["--json", "test", s(&prog)]), 1);
    let passed = d.get("passed").and_then(Json::as_i64).unwrap();
    let failed = d.get("failed").and_then(Json::as_i64).unwrap();
    assert_eq!(passed + failed, 3);
    assert!(failed >= 1);
    let results = d.get("results").and_then(Json::as_array).unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].get("name").and_then(Json::as_str), Some("good"));
    assert_eq!(results[0].get("status").and_then(Json::as_str), Some("ok"));
    assert_eq!(
        results[1].get("status").and_then(Json::as_str),
        Some("fail")
    );
}

#[test]
fn logos_missing_file_is_not_found() {
    let t = Temp::new("lmissing");
    let out = run(LOGOS, &["--json", "run", s(&t.join("nope.logos"))]);
    assert_eq!(error_code(&fail(&out, 2, "run")), "not_found");
}

#[test]
fn logos_json_unknown_subcommand_is_usage() {
    let out = run(LOGOS, &["--json", "nosuch"]);
    assert_eq!(error_code(&fail(&out, 2, "")), "usage");
}

#[test]
fn logos_text_mode_unchanged() {
    let t = Temp::new("logostext");
    let prog = t.file("t.logos", "test \"good\" { assert 1 == 1 }\n");
    let out = run(LOGOS, &["test", s(&prog)]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "ok good\n1 passed, 0 failed\n"
    );
}
