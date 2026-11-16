// Agent-interface tests for the bench binary: --describe and --json (docs/ai-computer/ACI.md).
// bench takes no file paths, so the path, not_found and --out rules do not apply to it.
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_bench");

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
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

/// The error code and error object of a failed envelope, checking the exit code and command name.
fn fail(output: &Output, exit: i32, cmd: &str) -> (String, Json) {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    let error = value.get("error").unwrap();
    let code = error.get("code").unwrap().as_str().unwrap().to_string();
    (code, error.clone())
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("no {key} in {value:?}"))
}

#[test]
fn describe_is_a_valid_manifest() {
    let output = run(&["--describe"]);
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
    assert_eq!(names, ["bench"]);
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    assert!(help.contains("bench"), "{help}");
}

#[test]
fn json_run_lists_suite_rates() {
    // mercury is the fastest suite (under a second); a full run takes about 24 s.
    let value = data(&run(&["--json", "--only", "mercury"]));
    let suites = field(&value, "suites").as_array().unwrap();
    assert_eq!(suites.len(), 1);
    assert_eq!(field(&suites[0], "name").as_str(), Some("mercury"));
    let results = field(&suites[0], "results").as_array().unwrap();
    assert!(!results.is_empty());
    for result in results {
        assert!(field(result, "name").as_str().is_some());
        assert!(field(result, "unit").as_str().is_some());
        match field(result, "value") {
            Json::Num(v) => assert!(v.is_finite() && *v >= 0.0, "{v}"),
            Json::Int(v) => assert!(*v >= 0, "{v}"),
            other => panic!("value is not a number: {other:?}"),
        }
    }
}

#[test]
fn json_repeated_suite_runs_once() {
    let value = data(&run(&["--json", "--only", "mercury", "--only", "mercury"]));
    let suites = field(&value, "suites").as_array().unwrap();
    assert_eq!(suites.len(), 1);
}

#[test]
fn json_unknown_suite_is_usage() {
    let output = run(&["--json", "--only", "titan"]);
    let (code, error) = fail(&output, 2, "");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("run: bench --describe")
    );
}

#[test]
fn json_missing_option_value_is_usage() {
    let (code, _) = fail(&run(&["--json", "--only"]), 2, "");
    assert_eq!(code, "usage");
}

#[test]
fn json_unknown_flag_is_usage() {
    let (code, _) = fail(&run(&["--json", "nosuch"]), 2, "");
    assert_eq!(code, "usage");
}

#[test]
fn json_help_returns_usage_text() {
    let value = data(&run(&["--json", "--help"]));
    let usage = field(&value, "usage").as_str().unwrap();
    assert!(usage.starts_with("usage: bench"), "{usage}");
}

#[test]
fn text_mode_unchanged() {
    let output = run(&["--only", "mercury"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("cores: "), "{stdout}");
    assert!(
        stdout.contains("| suite | benchmark | rate | unit |"),
        "{stdout}"
    );
    assert!(stdout.contains("| mercury | parse_request |"), "{stdout}");

    let bad = run(&["--only", "titan"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("error: unknown suite: titan"));
}

#[test]
fn json_accepts_the_manifest_command_name() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_bench"))
        .args(["--json", "bench", "--only", "mercury"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with("{\"ok\":true,\"tool\":\"bench\",\"cmd\":\"bench\""),
        "{text}"
    );
}
