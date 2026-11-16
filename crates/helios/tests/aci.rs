// Agent-interface tests for the helios binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_helios");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const INPUTS: &str = "0 1\n30 2\n90 0\n";

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("helios_aci_{name}_{}_{id}", std::process::id()));
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

fn write(t: &Temp, name: &str, text: &str) -> PathBuf {
    let path = t.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn game(t: &Temp) -> PathBuf {
    write(t, "game.logos", helios::script::GAME_SRC)
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
    assert_eq!(names, ["run", "record", "replay"]);
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    for name in names {
        assert!(help.contains(name), "{name} not in --help");
    }
}

#[test]
fn json_run_reports_hashes_every_60_ticks() {
    let t = Temp::new("run");
    let g = game(&t);
    let value = data(&run(&[
        "--json",
        "run",
        s(&g),
        "--ticks",
        "120",
        "--seed",
        "1",
    ]));
    assert_eq!(field(&value, "ticks"), &Json::Int(120));
    assert_eq!(field(&value, "seed"), &Json::Int(1));
    let hashes = field(&value, "hashes").as_array().unwrap();
    let ticks: Vec<i64> = hashes
        .iter()
        .map(|h| field(h, "tick").as_i64().unwrap())
        .collect();
    assert_eq!(ticks, [60, 120]);
    let final_hash = field(&value, "final_hash").as_str().unwrap();
    assert_eq!(final_hash.len(), 16);
    assert!(final_hash
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(field(&hashes[1], "hash").as_str(), Some(final_hash));
}

#[test]
fn run_twice_gives_same_final_hash() {
    let t = Temp::new("twice");
    let g = game(&t);
    let args = ["--json", "run", s(&g), "--ticks", "120", "--seed", "1"];
    let a = data(&run(&args));
    let b = data(&run(&args));
    assert_eq!(field(&a, "final_hash"), field(&b, "final_hash"));
}

#[test]
fn json_record_writes_trec_file() {
    let t = Temp::new("record");
    let g = game(&t);
    let inputs = write(&t, "inputs.txt", INPUTS);
    let out = t.join("run.trec");
    let value = data(&run(&[
        "--json",
        "record",
        s(&g),
        s(&inputs),
        "-o",
        s(&out),
    ]));
    let bytes = fs::read(&out).unwrap();
    assert_eq!(field(&value, "path").as_str(), Some(fwd(&out).as_str()));
    assert_eq!(field(&value, "bytes"), &Json::Int(bytes.len() as i64));
    assert_eq!(&bytes[..4], b"TREC");
    assert_eq!(field(&value, "ticks"), &Json::Int(91));
}

#[test]
fn json_replay_matches_text_record_hash() {
    let t = Temp::new("replay");
    let g = game(&t);
    let inputs = write(&t, "inputs.txt", INPUTS);
    let out = t.join("run.trec");
    let recorded = run(&["record", s(&g), s(&inputs), "-o", s(&out)]);
    assert_eq!(recorded.status.code(), Some(0));
    let text = String::from_utf8(recorded.stdout).unwrap();
    let text_hash = text.trim().rsplit("0x").next().unwrap();

    let value = data(&run(&["--json", "replay", s(&g), s(&out)]));
    assert_eq!(field(&value, "ticks"), &Json::Int(91));
    assert_eq!(field(&value, "final_hash").as_str(), Some(text_hash));
}

#[test]
fn json_record_dash_is_usage() {
    let t = Temp::new("dash");
    let g = game(&t);
    let inputs = write(&t, "inputs.txt", INPUTS);
    let output = run(&["--json", "record", s(&g), s(&inputs), "-o", "-"]);
    let (code, error) = fail(&output, 2, "record");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("use -o FILE.trec")
    );
}

#[test]
fn json_missing_files_are_not_found() {
    let t = Temp::new("missing");
    let missing_game = t.join("nope.logos");
    let missing_trec = t.join("nope.trec");
    let g = game(&t);
    let inputs = write(&t, "inputs.txt", INPUTS);
    let out = t.join("run.trec");
    let cases: [Vec<&str>; 3] = [
        vec!["--json", "run", s(&missing_game), "--ticks", "5"],
        vec![
            "--json",
            "record",
            s(&missing_game),
            s(&inputs),
            "-o",
            s(&out),
        ],
        vec!["--json", "replay", s(&g), s(&missing_trec)],
    ];
    for args in cases {
        let cmd = args[1];
        assert_eq!(fail(&run(&args), 1, cmd).0, "not_found", "{args:?}");
    }
}

#[test]
fn json_bad_inputs_are_invalid() {
    let t = Temp::new("bad");
    let g = game(&t);
    let bad_inputs = write(&t, "bad.txt", "frobnicate\n");
    let out = t.join("run.trec");
    let (code, _) = fail(
        &run(&["--json", "record", s(&g), s(&bad_inputs), "-o", s(&out)]),
        1,
        "record",
    );
    assert_eq!(code, "invalid");

    let garbage = write(&t, "garbage.trec", "not a replay");
    let (code, _) = fail(&run(&["--json", "replay", s(&g), s(&garbage)]), 1, "replay");
    assert_eq!(code, "invalid");
}

#[test]
fn json_bad_option_value_is_usage() {
    let t = Temp::new("ticks");
    let g = game(&t);
    let output = run(&["--json", "run", s(&g), "--ticks", "nope"]);
    let (code, error) = fail(&output, 2, "run");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("run: helios --describe")
    );
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    let (code, error) = fail(&output, 2, "");
    assert_eq!(code, "usage");
    assert_eq!(
        error.get("hint").and_then(Json::as_str),
        Some("run: helios --describe")
    );
}

#[test]
fn json_help_returns_usage_text() {
    let value = data(&run(&["--json", "--help"]));
    let usage = field(&value, "usage").as_str().unwrap();
    assert!(usage.contains("SUBCOMMANDS"), "{usage}");
}

#[test]
fn json_path_with_spaces_and_backslashes() {
    let t = Temp::new("spaces");
    let folder = t.join("with space");
    fs::create_dir_all(&folder).unwrap();
    let g = game(&t);
    let inputs = write(&t, "inputs.txt", INPUTS);
    let out = folder.join("run.trec");
    let value = data(&run(&[
        "--json",
        "record",
        s(&g),
        s(&inputs),
        "-o",
        s(&out),
    ]));
    let path = field(&value, "path").as_str().unwrap();
    assert!(!path.contains('\\'), "{path}");
    assert!(path.ends_with("with space/run.trec"), "{path}");
}

#[test]
fn text_mode_unchanged() {
    let t = Temp::new("text");
    let g = game(&t);
    let output = run(&["run", s(&g), "--ticks", "130", "--seed", "42"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("tick 60 hash 0x"), "{stdout}");
    assert!(stdout.contains("tick 120 hash 0x"), "{stdout}");
    assert!(stdout.contains("final tick 130 hash 0x"), "{stdout}");

    let bad = run(&["run", s(&g), "--ticks", "nope"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("helios:"));
}
