//! Agent-interface tests for calliope: --describe and --json (docs/ai-computer/ACI.md).
#![allow(clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn calliope(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_calliope"))
        .args(args)
        .output()
        .expect("failed to execute calliope")
}

/// A fresh folder under the system temp dir, named for the test.
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("calliope_aci_{label}_{}", std::process::id()));
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

#[test]
fn describe_is_a_valid_manifest() {
    let output = calliope(&["--describe"]);
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
    assert_eq!(names, ["grep", "edit", "FILE"]);
    let help = String::from_utf8_lossy(&calliope(&["--help"]).stdout).into_owned();
    assert!(help.contains("grep REGEX FILE"), "{help}");
    assert!(help.contains("edit SCRIPT FILE"), "{help}");
    assert!(help.contains("calliope FILE"), "{help}");
}

#[test]
fn json_grep_reports_line_and_col_across_files() {
    let dir = temp_dir("grep_two");
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    fs::write(&a, "alpha\n  fn one\n").unwrap();
    fs::write(&b, "fn two\r\nnothing\r\n").unwrap();
    let output = calliope(&[
        "--json",
        "grep",
        "fn [a-z]+",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let value = envelope(&output);
    assert_eq!(value.get("cmd").unwrap().as_str(), Some("grep"));
    let d = data(&value);
    assert_eq!(d.get("total"), Some(&Json::Int(2)));
    assert_eq!(d.get("more"), Some(&Json::Bool(false)));
    let rows = items(d);
    let first = &rows[0];
    assert_eq!(first.get("line"), Some(&Json::Int(2)));
    assert_eq!(first.get("col"), Some(&Json::Int(3)));
    assert_eq!(first.get("text").unwrap().as_str(), Some("  fn one"));
    let second = &rows[1];
    assert_eq!(second.get("line"), Some(&Json::Int(1)));
    assert_eq!(second.get("col"), Some(&Json::Int(1)));
    assert_eq!(second.get("text").unwrap().as_str(), Some("fn two"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_grep_path_with_space_uses_forward_slashes() {
    let dir = temp_dir("grep path").join("sub dir");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.txt");
    fs::write(&file, "hello\n").unwrap();
    let output = calliope(&["--json", "grep", "hel", file.to_str().unwrap()]);
    let value = envelope(&output);
    let path = items(data(&value))[0]
        .get("path")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("sub dir/note.txt"), "{path}");
    let _ = fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn json_grep_limit_pages_the_matches() {
    let dir = temp_dir("grep_limit");
    let file = dir.join("three.txt");
    fs::write(&file, "x1\nx2\nx3\n").unwrap();
    let value = envelope(&calliope(&[
        "--json",
        "grep",
        "--limit",
        "1",
        "x",
        file.to_str().unwrap(),
    ]));
    let d = data(&value);
    assert_eq!(items(d).len(), 1);
    assert_eq!(d.get("total"), Some(&Json::Int(3)));
    assert_eq!(d.get("more"), Some(&Json::Bool(true)));
    assert_eq!(d.get("next_offset"), Some(&Json::Int(1)));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_grep_text_is_cut_on_a_character_boundary() {
    let dir = temp_dir("grep_cut");
    let file = dir.join("long.txt");
    // 199 two-byte characters then the match: 398 bytes, the cut lands at 400 bytes.
    let line = format!("{}é Z{}", "é".repeat(199), "z".repeat(300));
    fs::write(&file, format!("{line}\n")).unwrap();
    let value = envelope(&calliope(&["--json", "grep", "Z", file.to_str().unwrap()]));
    let item = &items(data(&value))[0];
    let text = item.get("text").unwrap().as_str().unwrap();
    assert!(text.len() <= 400, "{}", text.len());
    assert_eq!(item.get("truncated"), Some(&Json::Bool(true)));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_grep_no_match_is_ok_with_exit_one() {
    let dir = temp_dir("grep_none");
    let file = dir.join("a.txt");
    fs::write(&file, "abc\n").unwrap();
    let output = calliope(&["--json", "grep", "zzz", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let value = envelope(&output);
    assert_eq!(data(&value).get("total"), Some(&Json::Int(0)));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let dir = temp_dir("bad_input");
    let file = dir.join("a.txt");
    fs::write(&file, "abc\n").unwrap();
    let f = file.to_str().unwrap();

    let regex = calliope(&["--json", "grep", "(", f]);
    assert_eq!(error_code(&envelope(&regex)), "invalid");
    assert_eq!(regex.status.code(), Some(2));

    let missing = dir.join("missing.txt");
    let gone = calliope(&["--json", "grep", "a", missing.to_str().unwrap()]);
    assert_eq!(error_code(&envelope(&gone)), "not_found");
    assert_eq!(gone.status.code(), Some(1));

    let script = dir.join("script.txt");
    fs::write(&script, "goto 1 1\nbogus command\n").unwrap();
    let bad = calliope(&["--json", "edit", script.to_str().unwrap(), f]);
    let value = envelope(&bad);
    assert_eq!(error_code(&value), "invalid");
    let message = value
        .get("error")
        .unwrap()
        .get("message")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(message.contains("script line 2"), "{message}");
    assert_eq!(bad.status.code(), Some(1));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_edit_inserts_and_saves_keeping_crlf() {
    let dir = temp_dir("edit_crlf");
    let file = dir.join("notes.txt");
    fs::write(&file, "one\r\ntwo\r\n").unwrap();
    let script = dir.join("script.txt");
    fs::write(&script, "goto 2 1\ninsert new\\n\nsave\n").unwrap();
    let output = calliope(&[
        "--json",
        "edit",
        script.to_str().unwrap(),
        file.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let value = envelope(&output);
    assert_eq!(value.get("cmd").unwrap().as_str(), Some("edit"));
    let d = data(&value);
    assert_eq!(d.get("changed"), Some(&Json::Bool(true)));
    assert_eq!(d.get("saved"), Some(&Json::Bool(true)));
    let before = d.get("lines_before").unwrap().as_i64().unwrap();
    let after = d.get("lines_after").unwrap().as_i64().unwrap();
    assert_eq!(after, before + 1);
    assert_eq!(
        d.get("text").unwrap().as_str(),
        Some("one\r\nnew\r\ntwo\r\n")
    );
    let on_disk = fs::read(&file).unwrap();
    assert_eq!(on_disk, b"one\r\nnew\r\ntwo\r\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_editor_is_unsupported_and_returns_at_once() {
    let dir = temp_dir("editor");
    let file = dir.join("somefile.txt");
    fs::write(&file, "x\n").unwrap();
    let output = calliope(&["--json", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let value = envelope(&output);
    assert_eq!(error_code(&value), "unsupported");
    let hint = value
        .get("error")
        .unwrap()
        .get("hint")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(hint.contains("calliope --json edit SCRIPT FILE"), "{hint}");
    assert!(hint.contains("calliope --json grep REGEX FILE"), "{hint}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = calliope(&["--json", "nosuch"]);
    assert_eq!(output.status.code(), Some(2));
    let value = envelope(&output);
    assert_eq!(error_code(&value), "usage");
    assert_eq!(value.get("cmd").unwrap().as_str(), Some(""));
}

#[test]
fn text_grep_output_is_unchanged() {
    let dir = temp_dir("text_grep");
    let file = dir.join("a.txt");
    fs::write(&file, "hello world\n").unwrap();
    let output = calliope(&["grep", "wor", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    let expected = format!("{}:1:7: hello world\n", file.display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    let _ = fs::remove_dir_all(&dir);
}
