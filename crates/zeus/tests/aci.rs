// Agent-interface tests for the zeusctl binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_zeusctl");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zeusctl_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .current_dir(workspace_root())
        .args(args)
        .output()
        .unwrap()
}

/// The built zeus ELF the boot tests need; None means the test skips, as the CLI tests do.
fn zeus_elf() -> Option<PathBuf> {
    let path = workspace_root()
        .join("target")
        .join("zeus-bin")
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("zeus");
    if path.exists() {
        Some(path)
    } else {
        eprintln!("skipping zeusctl boot test: missing {}", path.display());
        None
    }
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
    assert_eq!(names, ["run"]);
}

#[test]
fn json_run_bounded_reports_stop_and_steps() {
    let Some(zeus) = zeus_elf() else { return };
    let value = data(&run(&[
        "--json",
        "run",
        "--zeus",
        s(&zeus),
        "--steps",
        "1000",
    ]));
    assert_eq!(field(&value, "stop").as_str(), Some("step_limit"));
    assert!(value.get("exit").is_none(), "no exit code at step limit");
    assert!(field(&value, "steps").as_i64().is_some());
    assert!(field(&value, "duration_ms").as_i64().is_some());
    assert!(field(&value, "stdout").as_str().is_some());
}

#[test]
fn json_run_default_guest_exits_with_pass() {
    let Some(zeus) = zeus_elf() else { return };
    let value = data(&run(&["--json", "run", "--zeus", s(&zeus)]));
    assert_eq!(field(&value, "stop").as_str(), Some("exit"));
    assert_eq!(field(&value, "exit"), &Json::Int(0));
    let stdout = field(&value, "stdout").as_str().unwrap();
    assert!(stdout.contains("[lithium] lithium: halt"), "{stdout}");
}

#[test]
fn json_run_missing_zeus_is_not_found() {
    let t = Temp::new("missing");
    let missing = t.0.join("no_such_zeus");
    assert_eq!(
        fail(&run(&["--json", "run", "--zeus", s(&missing)]), 1, "run"),
        "not_found"
    );
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    assert_eq!(
        fail(&run(&["--json", "run", "--steps", "0"]), 2, "run"),
        "usage"
    );
    assert_eq!(
        fail(&run(&["--json", "run", "--steps", "abc"]), 2, "run"),
        "usage"
    );
    assert_eq!(
        fail(&run(&["--json", "run", "--guest", "foo"]), 2, "run"),
        "usage"
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
fn text_mode_is_unchanged() {
    let version = run(&["--version"]);
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "zeusctl 0.1.0"
    );
    let Some(zeus) = zeus_elf() else { return };
    let out = run(&["run", "--zeus", s(&zeus), "--steps", "1000"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[lines.len() - 2].starts_with("exit: OutOfSteps"),
        "{text}"
    );
    assert!(lines[lines.len() - 1].starts_with("steps: "), "{text}");
}
