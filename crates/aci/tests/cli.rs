//! CLI tests for the `aci` validator.
#![allow(clippy::unwrap_used)]

use aci::envelope::validate_envelope;
use aci::json::Json;
use std::io::Write;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_aci");

// The manifest example from docs/ai-computer/ACI.md, verbatim.
const MANIFEST_EXAMPLE: &str = r#"{"aci":1,"tool":"hades","version":"0.1.0","summary":"Monolith filesystem image tool",
 "commands":[{"name":"ls","summary":"List directory entries at PATH","effect":"read",
   "args":[{"name":"IMG","kind":"path","required":true},
           {"name":"PATH","kind":"text","required":true},
           {"name":"--limit","kind":"int","required":false,"default":"100"}],
   "returns":"items: [{name, kind, size}], total, more, next_offset",
   "example":"hades --json ls disk.img /notes"}]}"#;

const SUCCESS_EXAMPLE: &str =
    r#"{"ok":true,"tool":"hades","cmd":"ls","data":{"items":[],"total":0,"more":false}}"#;
const FAILURE_EXAMPLE: &str = r#"{"ok":false,"tool":"hades","cmd":"ls","error":{"code":"not_found","message":"no such path: /x","hint":"run: hades --json ls disk.img /"}}"#;

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn spec_manifest_example_passes_check_manifest() {
    let out = run_with_stdin(&["check-manifest", "-"], MANIFEST_EXAMPLE.as_bytes());
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
}

#[test]
fn spec_envelope_examples_pass_check_envelope() {
    for example in [SUCCESS_EXAMPLE, FAILURE_EXAMPLE] {
        let out = run_with_stdin(&["check-envelope", "-"], example.as_bytes());
        assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
    }
}

#[test]
fn describe_output_passes_check_manifest() {
    let describe = run(&["--describe"]);
    assert_eq!(describe.status.code(), Some(0));
    let out = run_with_stdin(&["check-manifest", "-"], &describe.stdout);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
}

#[test]
fn check_rejects_a_bad_manifest() {
    let out = run_with_stdin(&["check-manifest", "-"], br#"{"aci":2}"#);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr_of(&out).starts_with("aci: "),
        "stderr: {}",
        stderr_of(&out)
    );
}

#[test]
fn unknown_command_under_json_is_a_usage_envelope() {
    let out = run(&["--json", "bogus"]);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "stdout: {text}");
    let value = Json::parse(text.trim_end()).unwrap();
    assert_eq!(validate_envelope(&value), Ok(()));
    let code = value
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Json::as_str);
    assert_eq!(code, Some("usage"));
}

#[test]
fn version_prints_the_version() {
    let out = run(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "aci 0.1.0\n");
}
