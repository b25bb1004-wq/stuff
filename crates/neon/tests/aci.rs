// Agent-interface tests for the titan binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_titan");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("titan_aci_{name}_{}_{id}", std::process::id()));
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .current_dir(workspace_root())
        .args(args)
        .output()
        .unwrap()
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

fn script(t: &Temp, name: &str, text: &str) -> PathBuf {
    let path = t.join(name);
    fs::write(&path, text).unwrap();
    path
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
    assert_eq!(names, ["boot"]);
}

#[test]
fn json_boot_script_reports_stdout_and_stop() {
    let t = Temp::new("script");
    let file = script(&t, "smallest.txt", "echo titan-ok\n");
    let value = data(&run(&["--json", "boot", "--script", s(&file)]));
    assert_eq!(field(&value, "stop").as_str(), Some("halt"));
    assert!(value.get("exit").is_none(), "guest did not exit");
    let stdout = field(&value, "stdout").as_str().unwrap();
    assert!(stdout.contains("titan-ok"), "{stdout}");
    assert!(field(&value, "duration_ms").as_i64().is_some());
}

#[test]
fn json_boot_interactive_is_unsupported() {
    let out = run(&["--json", "boot"]);
    assert_eq!(out.status.code(), Some(1));
    let value = envelope(&out);
    let error = value.get("error").unwrap();
    assert_eq!(
        error.get("code").and_then(Json::as_str),
        Some("unsupported")
    );
    let hint = error.get("hint").and_then(Json::as_str).unwrap();
    assert!(hint.contains("--script"), "{hint}");
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let t = Temp::new("bad");
    let missing = t.join("missing.txt");
    assert_eq!(
        fail(
            &run(&["--json", "boot", "--script", s(&missing)]),
            1,
            "boot"
        ),
        "not_found"
    );
    let file = script(&t, "ok.txt", "echo x\n");
    let missing_kernel = t.join("no_kernel.elf");
    assert_eq!(
        fail(
            &run(&[
                "--json",
                "boot",
                "--kernel",
                s(&missing_kernel),
                "--script",
                s(&file)
            ]),
            1,
            "boot"
        ),
        "not_found"
    );
    assert_eq!(
        fail(&run(&["--json", "boot", "--script"]), 2, "boot"),
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
    let t = Temp::new("text");
    let file = script(&t, "smallest.txt", "echo titan-ok\n");
    let out = run(&["boot", "--script", s(&file)]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("titan-ok"), "{text}");
    assert!(!text.starts_with('{'), "text mode printed JSON: {text}");
}

#[test]
fn boot_works_outside_the_repo_with_an_absolute_kernel() {
    let t = Temp::new("outside");
    let kernel =
        workspace_root().join("target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel");
    let file = script(&t, "hi.txt", "echo hi from outside\n");
    let out = Command::new(BIN)
        .current_dir(&t.0)
        .args([
            "--json",
            "boot",
            "--kernel",
            s(&kernel),
            "--script",
            s(&file),
        ])
        .output()
        .unwrap();
    let d = data(&out);
    let stdout = d.get("stdout").and_then(Json::as_str).unwrap();
    assert!(stdout.contains("hi from outside"), "{stdout}");
}

#[test]
fn boot_without_a_kernel_names_both_places_it_looked() {
    let t = Temp::new("nokernel");
    let file = script(&t, "hi.txt", "echo hi\n");
    let out = Command::new(BIN)
        .current_dir(&t.0)
        .args(["--json", "boot", "--script", s(&file)])
        .output()
        .unwrap();
    assert_eq!(fail(&out, 1, "boot"), "not_found");
    let value = envelope(&out);
    let message = value
        .get("error")
        .unwrap()
        .get("message")
        .and_then(Json::as_str)
        .unwrap();
    assert!(message.contains("target/titan-kernel"), "{message}");
    assert!(message.contains("share"), "{message}");
    assert!(message.contains("cargo build"), "{message}");
}
