//! Agent-interface tests for athena: --describe and --json (docs/ai-computer/ACI.md).
#![allow(clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn athena(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_athena"))
        .args(args)
        .output()
        .expect("failed to execute athena")
}

/// A fresh folder under the system temp dir, named for the test.
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("athena_aci_{label}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
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

fn items(d: &Json) -> &[Json] {
    d.get("items").unwrap().as_array().unwrap()
}

const GOOD: &str = "fn add(a: int, b: int) -> int { a + b }\n";
const BAD: &str = "fn add(a: int) -> int { a + true }\n";

#[test]
fn describe_is_a_valid_manifest() {
    let output = athena(&["--describe"]);
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
    assert_eq!(names, ["check", "fmt", "test", "serve"]);
    let help = String::from_utf8_lossy(&athena(&["--help"]).stdout).into_owned();
    assert!(help.contains("check FILE.logos"), "{help}");
    assert!(help.contains("fmt FILE.logos"), "{help}");
    assert!(help.contains("test FILE.logos"), "{help}");
    assert!(help.contains("serve [FOLDER]"), "{help}");
}

#[test]
fn json_check_clean_and_known_error() {
    let dir = temp_dir("check");
    let good = dir.join("good.logos");
    fs::write(&good, GOOD).unwrap();
    let out = athena(&["--json", "check", good.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let d = data(&envelope(&out)).clone();
    assert_eq!(d.get("clean"), Some(&Json::Bool(true)));
    assert_eq!(d.get("count"), Some(&Json::Int(0)));
    assert!(d.get("diagnostics").unwrap().as_array().unwrap().is_empty());

    // One blank line, then the error: the diagnostic is on line 2.
    let bad = dir.join("bad.logos");
    fs::write(&bad, format!("\n{BAD}")).unwrap();
    let out = athena(&["--json", "check", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let value = envelope(&out);
    let d = data(&value);
    assert_eq!(d.get("clean"), Some(&Json::Bool(false)));
    let diags = d.get("diagnostics").unwrap().as_array().unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(d.get("count"), Some(&Json::Int(1)));
    let first = &diags[0];
    assert_eq!(first.get("line"), Some(&Json::Int(2)));
    assert_eq!(first.get("severity").unwrap().as_str(), Some("error"));
    assert!(first.get("message").unwrap().as_str().is_some());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_check_path_with_space_and_backslashes() {
    let dir = temp_dir("check path").join("sub dir");
    fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.logos");
    fs::write(&bad, BAD).unwrap();
    let value = envelope(&athena(&["--json", "check", bad.to_str().unwrap()]));
    let path = data(&value).get("diagnostics").unwrap().as_array().unwrap()[0]
        .get("path")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("sub dir/bad.logos"), "{path}");
    let _ = fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn json_test_reports_passed_and_failed() {
    let dir = temp_dir("test");
    let file = dir.join("tests.logos");
    fs::write(
        &file,
        r#"test "ok" {
  assert(1 == 1)
}

prop "bad prop" (x: int) {
  assert(x == x + 1)
}
"#,
    )
    .unwrap();
    let out = athena(&["--json", "test", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let value = envelope(&out);
    let d = data(&value);
    assert_eq!(d.get("passed"), Some(&Json::Int(1)));
    assert_eq!(d.get("failed"), Some(&Json::Int(1)));
    let results = d.get("results").unwrap().as_array().unwrap();
    let statuses: Vec<(&str, &str)> = results
        .iter()
        .map(|r| {
            (
                r.get("name").unwrap().as_str().unwrap(),
                r.get("status").unwrap().as_str().unwrap(),
            )
        })
        .collect();
    assert!(statuses.contains(&("ok", "passed")), "{statuses:?}");
    assert!(statuses.contains(&("bad prop", "failed")), "{statuses:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_fmt_reports_changed_and_writes_with_flag() {
    let dir = temp_dir("fmt");
    let file = dir.join("messy.logos");
    let messy = "fn   add(a:int,b:int)->int{a+b}\n";
    fs::write(&file, messy).unwrap();
    let f = file.to_str().unwrap();

    let value = envelope(&athena(&["--json", "fmt", f]));
    let d = data(&value);
    let item = &items(d)[0];
    assert_eq!(item.get("changed"), Some(&Json::Bool(true)));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        messy,
        "no --write, no change"
    );

    let value = envelope(&athena(&["--json", "fmt", f, "--write"]));
    assert_eq!(
        items(data(&value))[0].get("changed"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "fn add(a: int, b: int) -> int {\n    a + b\n}\n"
    );
    let value = envelope(&athena(&["--json", "fmt", f]));
    assert_eq!(
        items(data(&value))[0].get("changed"),
        Some(&Json::Bool(false))
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let dir = temp_dir("bad_input");
    let commented = dir.join("commented.logos");
    fs::write(&commented, "// keep me\nfn f() -> int { 1 }\n").unwrap();
    let refused = athena(&["--json", "fmt", commented.to_str().unwrap()]);
    assert_eq!(error_code(&envelope(&refused)), "invalid");
    assert_eq!(refused.status.code(), Some(1));

    let missing = dir.join("missing.logos");
    let gone = athena(&["--json", "check", missing.to_str().unwrap()]);
    assert_eq!(error_code(&envelope(&gone)), "not_found");
    assert_eq!(gone.status.code(), Some(1));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_serve_missing_folder_is_not_found() {
    let out = athena(&["--json", "serve", "no-such-folder-athena-aci"]);
    assert_eq!(out.status.code(), Some(1));
    let value = envelope(&out);
    assert_eq!(error_code(&value), "not_found");
    assert_eq!(value.get("cmd").unwrap().as_str(), Some("serve"));
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let out = athena(&["--json", "nosuch"]);
    assert_eq!(out.status.code(), Some(2));
    let value = envelope(&out);
    assert_eq!(error_code(&value), "usage");
    assert_eq!(value.get("cmd").unwrap().as_str(), Some(""));
}

#[test]
fn text_mode_unchanged() {
    let dir = temp_dir("text");
    let good = dir.join("good_tests.logos");
    fs::write(
        &good,
        "fn add(a: int, b: int) -> int { a + b }\n\ntest \"add works\" {\n  assert(add(2, 3) == 5)\n}\n",
    )
    .unwrap();
    let out = athena(&["test", good.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.starts_with("PASS add works"), "{stdout}");
    assert!(
        stdout.ends_with("summary: 1 passed, 0 failed\n"),
        "{stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}
