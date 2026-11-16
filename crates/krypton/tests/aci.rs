#![cfg(feature = "std")]
#![allow(missing_docs, clippy::unwrap_used)]
// Agent-interface tests for the krypton binary: --describe and --json (docs/ai-computer/ACI.md).

use aci::json::Json;
use std::process::{Command, Output};

fn krypton(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_krypton"))
        .args(args)
        .output()
        .expect("failed to execute krypton")
}

/// Parse stdout as exactly one envelope and validate it.
fn envelope(output: &Output) -> Json {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(
        stdout.matches('\n').count(),
        1,
        "more than one line: {stdout:?}"
    );
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    value
}

fn data(value: &Json) -> &Json {
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap()
}

fn error_code(value: &Json) -> &str {
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)));
    value
        .get("error")
        .unwrap()
        .get("code")
        .unwrap()
        .as_str()
        .unwrap()
}

#[test]
fn describe_is_a_valid_manifest() {
    let output = krypton(&["--describe"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let names: Vec<&str> = manifest
        .get("commands")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.get("name").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(names, ["-c", "script", "interactive"]);
    // Each way of running is also in --help.
    let help = String::from_utf8_lossy(&krypton(&["--help"]).stdout).into_owned();
    assert!(help.contains("-c SCRIPT"), "{help}");
    assert!(help.contains("FILE [ARGS...]"), "{help}");
    assert!(help.contains("Interactive shell"), "{help}");
}

#[test]
fn json_c_success_is_an_envelope() {
    let value = envelope(&krypton(&["--json", "-c", "echo hello"]));
    assert_eq!(value.get("tool").unwrap().as_str(), Some("krypton"));
    assert_eq!(value.get("cmd").unwrap().as_str(), Some("-c"));
    let d = data(&value);
    assert_eq!(d.get("exit"), Some(&Json::Int(0)));
    assert_eq!(d.get("stdout").unwrap().as_str(), Some("hello\n"));
    assert_eq!(d.get("stderr").unwrap().as_str(), Some(""));
    assert!(matches!(d.get("duration_ms"), Some(Json::Int(n)) if *n >= 0));
}

#[test]
fn json_file_success_is_an_envelope() {
    let dir = std::env::temp_dir().join(format!("krypton_aci_file_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("script.sh");
    std::fs::write(&script, "echo \"$1-$2\"\necho oops >&2\n").unwrap();
    let output = krypton(&["--json", script.to_str().unwrap(), "a", "b"]);
    let value = envelope(&output);
    assert_eq!(value.get("cmd").unwrap().as_str(), Some("script"));
    let d = data(&value);
    assert_eq!(d.get("stdout").unwrap().as_str(), Some("a-b\n"));
    assert_eq!(d.get("stderr").unwrap().as_str(), Some("oops\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let syntax = krypton(&["--json", "-c", "if then"]);
    let value = envelope(&syntax);
    assert_eq!(error_code(&value), "invalid");
    let error = value.get("error").unwrap();
    assert!(error
        .get("message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("line 1"));
    assert!(error
        .get("hint")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("line 1"));
    assert_eq!(syntax.status.code(), Some(2));

    let missing = krypton(&["--json", r"C:\no such dir\missing-krypton.sh"]);
    let value = envelope(&missing);
    assert_eq!(error_code(&value), "not_found");
    assert_eq!(missing.status.code(), Some(1));
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = krypton(&["--json", "--bogus"]);
    let value = envelope(&output);
    assert_eq!(error_code(&value), "usage");
    assert_eq!(value.get("cmd").unwrap().as_str(), Some(""));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn json_interactive_is_unsupported() {
    let output = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .arg("--json")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let value = envelope(&output);
    assert_eq!(error_code(&value), "unsupported");
    let hint = value
        .get("error")
        .unwrap()
        .get("hint")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(hint.contains("-c"), "{hint}");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn json_nonzero_exit_is_still_ok() {
    let output = krypton(&["--json", "-c", "exit 3"]);
    assert_eq!(output.status.code(), Some(3));
    let value = envelope(&output);
    assert_eq!(data(&value).get("exit"), Some(&Json::Int(3)));
}

#[test]
fn json_big_output_is_capped() {
    let script = format!("printf '%s' '{}'", "x".repeat(20000));
    let output = krypton(&["--json", "-c", &script]);
    let value = envelope(&output);
    let d = data(&value);
    assert_eq!(d.get("stdout").unwrap().as_str().map(str::len), Some(16384));
    assert_eq!(d.get("stdout_truncated"), Some(&Json::Bool(true)));
    assert_eq!(d.get("stdout_bytes"), Some(&Json::Int(20000)));
    assert_eq!(d.get("stdout_lossy"), None);
}

#[test]
fn json_non_utf8_output_is_lossy() {
    let output = krypton(&["--json", "-c", r"printf '\377'"]);
    let value = envelope(&output);
    let d = data(&value);
    assert_eq!(d.get("stdout").unwrap().as_str(), Some("\u{fffd}"));
    assert_eq!(d.get("stdout_lossy"), Some(&Json::Bool(true)));
    assert_eq!(d.get("stdout_truncated"), None);
}

#[test]
fn json_captures_external_program_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_krypton"))
        .args(["--json", "-c", "\"$KRYPTON\" -c 'echo inner'; echo outer"])
        .env("KRYPTON", env!("CARGO_BIN_EXE_krypton"))
        .output()
        .unwrap();
    let value = envelope(&output);
    assert_eq!(
        data(&value).get("stdout").unwrap().as_str(),
        Some("inner\nouter\n")
    );
}

#[test]
fn text_mode_unchanged() {
    let output = krypton(&["-c", "echo hi; echo err >&2; exit 4"]);
    assert_eq!(output.stdout, b"hi\n");
    assert_eq!(output.stderr, b"err\n");
    assert_eq!(output.status.code(), Some(4));
}
