// Agent-interface tests for the iris binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_iris");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const EVENTS: &str = "open terminal 200 150\nwait 50\n";

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("iris_aci_{name}_{}_{id}", std::process::id()));
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

fn events(t: &Temp, name: &str, text: &str) -> PathBuf {
    let path = t.join(name);
    fs::write(&path, text).unwrap();
    path
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
    assert_eq!(names, ["replay"]);
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    assert!(help.contains("replay"));
}

#[test]
fn json_replay_writes_png_and_reports_stats() {
    let t = Temp::new("replay");
    let src = events(&t, "events.txt", EVENTS);
    let out = t.join("frame.png");
    let value = data(&run(&[
        "--json",
        "replay",
        s(&src),
        "--out",
        s(&out),
        "--width",
        "400",
        "--height",
        "300",
    ]));
    let bytes = fs::read(&out).unwrap();
    assert_eq!(field(&value, "path").as_str(), Some(fwd(&out).as_str()));
    assert_eq!(field(&value, "bytes"), &Json::Int(bytes.len() as i64));
    assert_eq!(field(&value, "width"), &Json::Int(400));
    assert_eq!(field(&value, "height"), &Json::Int(300));
    assert_eq!(bytes[..8], PNG_SIGNATURE);
    assert_eq!(field(&value, "events"), &Json::Int(2));
    assert!(field(&value, "damage_rects").as_i64().unwrap() >= 1);

    let windows = field(&value, "windows").as_array().unwrap();
    assert_eq!(windows.len(), 1);
    let window = &windows[0];
    assert_eq!(field(window, "title").as_str(), Some("terminal"));
    assert_eq!(field(window, "focused"), &Json::Bool(true));
    assert!(field(window, "width").as_i64().unwrap() > 0);

    let damage = field(&value, "damage").as_array().unwrap();
    assert!(!damage.is_empty());
    for rect in damage {
        assert!(rect.get("width").and_then(Json::as_i64).is_some());
        assert!(rect.get("height").and_then(Json::as_i64).is_some());
    }
}

#[test]
fn json_replay_out_dash_is_usage() {
    let t = Temp::new("dash");
    let src = events(&t, "events.txt", EVENTS);
    let output = run(&["--json", "replay", s(&src), "--out", "-"]);
    let (code, error) = fail(&output, 2, "replay");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("use --out FILE.png")
    );
}

#[test]
fn json_missing_events_is_not_found() {
    let t = Temp::new("missing");
    let missing = t.join("nope.txt");
    let out = t.join("frame.png");
    let output = run(&["--json", "replay", s(&missing), "--out", s(&out)]);
    assert_eq!(fail(&output, 1, "replay").0, "not_found");
}

#[test]
fn json_bad_event_is_invalid() {
    let t = Temp::new("bad");
    let src = events(&t, "bad.txt", "frobnicate\n");
    let out = t.join("frame.png");
    let output = run(&["--json", "replay", s(&src), "--out", s(&out)]);
    assert_eq!(fail(&output, 1, "replay").0, "invalid");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    let (code, error) = fail(&output, 2, "");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("run: iris --describe")
    );
}

#[test]
fn json_path_with_spaces_and_backslashes() {
    let t = Temp::new("spaces");
    let folder = t.join("with space");
    fs::create_dir_all(&folder).unwrap();
    let src = events(&t, "events.txt", EVENTS);
    let out = folder.join("frame.png");
    let value = data(&run(&["--json", "replay", s(&src), "--out", s(&out)]));
    let path = field(&value, "path").as_str().unwrap();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("with space/frame.png"), "{path}");
}

#[test]
fn text_mode_unchanged() {
    let t = Temp::new("text");
    let src = events(&t, "events.txt", EVENTS);
    let out = t.join("frame.png");
    let output = run(&[
        "replay",
        s(&src),
        "--out",
        s(&out),
        "--width",
        "400",
        "--height",
        "300",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("screen 400x300\nevents 2\n"), "{stdout}");
    assert!(stdout.contains("windows 1\n"), "{stdout}");
}
