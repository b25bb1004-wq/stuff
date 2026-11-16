// Agent-interface tests for the lithium binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_lithium");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lithium_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
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

/// The data of a successful envelope; checks the exit code is 0.
fn data(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

/// The error code of a failed envelope, checking the exit code and that the command name is `cmd`.
fn fail(output: &Output, exit: i32, cmd: &str) -> String {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    value
        .get("error")
        .unwrap()
        .get("code")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("no {key} in {value:?}"))
}

#[test]
fn describe_is_a_valid_manifest() {
    let out = run(&["--describe"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let names: Vec<&str> = field(&manifest, "commands")
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.get("name").and_then(Json::as_str).unwrap())
        .collect();
    assert_eq!(names, ["run", "image"]);
}

#[test]
fn json_run_bounded_reports_stop_and_steps() {
    let value = data(&run(&["--json", "run", "--steps", "100"]));
    assert_eq!(field(&value, "stop").as_str(), Some("step_limit"));
    assert_eq!(field(&value, "steps"), &Json::Int(100));
    assert!(field(&value, "duration_ms").as_i64().is_some());
    assert!(value.get("exit").is_none(), "no exit code at step limit");
    assert!(field(&value, "stdout").as_str().is_some());
}

#[test]
fn json_run_full_boot_exits_with_pass() {
    let value = data(&run(&["--json", "run"]));
    assert_eq!(field(&value, "stop").as_str(), Some("exit"));
    assert_eq!(field(&value, "exit"), &Json::Int(0));
    assert!(!field(&value, "stdout").as_str().unwrap().is_empty());
}

#[test]
fn json_image_writes_the_binary() {
    let t = Temp::new("image");
    let out = t.join("kernel.bin");
    let value = data(&run(&["--json", "image", "-o", s(&out)]));
    assert_eq!(
        fs::read(&out).unwrap().len() as i64,
        field(&value, "bytes").as_i64().unwrap()
    );
    assert_eq!(
        field(&value, "path")
            .as_str()
            .map(|p| p.ends_with("kernel.bin")),
        Some(true)
    );
}

#[test]
fn json_image_needs_out() {
    assert_eq!(fail(&run(&["--json", "image"]), 2, "image"), "usage");
    assert_eq!(
        fail(&run(&["--json", "image", "-o", "-"]), 2, "image"),
        "usage"
    );
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let t = Temp::new("bad");
    assert_eq!(
        fail(&run(&["--json", "run", "--steps", "abc"]), 1, "run"),
        "invalid"
    );
    assert_eq!(
        fail(&run(&["--json", "run", "--steps", "0"]), 1, "run"),
        "invalid"
    );
    let missing = t.join("no_such_dir").join("out.bin");
    assert_eq!(
        fail(&run(&["--json", "image", "-o", s(&missing)]), 1, "image"),
        "io"
    );
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let out = run(&["--json", "nosuch"]);
    assert_eq!(out.status.code(), Some(2));
    let value = envelope(&out);
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(""));
    assert_eq!(
        value
            .get("error")
            .unwrap()
            .get("code")
            .and_then(Json::as_str),
        Some("usage")
    );
}

#[test]
fn json_path_with_spaces_and_backslashes() {
    let t = Temp::new("spaces");
    let dir = t.join("a b");
    fs::create_dir_all(&dir).unwrap();
    let out = dir.join("kernel.bin");
    let value = data(&run(&["--json", "image", "-o", s(&out)]));
    let path = field(&value, "path").as_str().unwrap();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("a b/kernel.bin"), "{path}");
}

#[test]
fn json_run_answers_with_steps() {
    // `run` has no console mode: under --json it always answers.
    let value = data(&run(&["--json", "run", "--steps", "10"]));
    assert_eq!(field(&value, "steps"), &Json::Int(10));
}

#[test]
fn text_mode_is_unchanged() {
    let out = run(&["run", "--steps", "100"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[lines.len() - 2], "exit: out_of_steps");
    assert_eq!(lines[lines.len() - 1], "steps: 100");
}
