//! CLI integration tests for the `helios` binary.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("helios_cli_test_{label}_{pid}_{id}"));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.path.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    fn path(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_helios")
}

fn run_cmd(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn run_with_stdin(args: &[&str], stdin_bytes: &[u8]) -> Output {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_bytes)
        .unwrap();
    child.wait_with_output().unwrap()
}

fn write_game(dir: &TempDir) -> PathBuf {
    dir.write("game.logos", helios::script::GAME_SRC.as_bytes())
}

fn pstr(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn help_prints_subcommands_and_examples() {
    let out = run_cmd(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("run "));
    assert!(stdout.contains("record "));
    assert!(stdout.contains("replay "));
    assert!(stdout.contains("Example:"));
}

#[test]
fn version_prints_expected_string() {
    let out = run_cmd(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "helios 0.1.0");
}

#[test]
fn unknown_subcommand_exits_2() {
    let out = run_cmd(&["does-not-exist"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("helios:"), "stderr was: {stderr}");
}

#[test]
fn usage_errors_exit_2() {
    let dir = TempDir::new("usage");
    let game = write_game(&dir);
    let inputs = dir.write("inputs.txt", b"0 1\n");

    let missing_o = run_cmd(&["record", pstr(&game), pstr(&inputs)]);
    assert_eq!(missing_o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_o.stderr).contains("helios:"));

    let bad_ticks = run_cmd(&["run", pstr(&game), "--ticks", "nope"]);
    assert_eq!(bad_ticks.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad_ticks.stderr).contains("helios:"));
}

#[test]
fn run_good_game_prints_periodic_and_final_hashes_deterministically() {
    let dir = TempDir::new("run_ok");
    let game = write_game(&dir);

    let out1 = run_cmd(&["run", pstr(&game), "--ticks", "130", "--seed", "42"]);
    assert_eq!(out1.status.code(), Some(0));
    let s1 = String::from_utf8_lossy(&out1.stdout);
    assert!(s1.contains("tick 60 hash 0x"), "stdout: {s1}");
    assert!(s1.contains("tick 120 hash 0x"), "stdout: {s1}");
    assert!(s1.contains("final tick 130 hash 0x"), "stdout: {s1}");

    let out2 = run_cmd(&["run", pstr(&game), "--ticks", "130", "--seed", "42"]);
    assert_eq!(out2.status.code(), Some(0));
    assert_eq!(out1.stdout, out2.stdout);

    let out_diff = run_cmd(&["run", pstr(&game), "--ticks", "130", "--seed", "99"]);
    assert_eq!(out_diff.status.code(), Some(0));
    assert_ne!(out1.stdout, out_diff.stdout);
}

#[test]
fn run_bad_input_exits_1() {
    let dir = TempDir::new("run_bad");
    let bad = dir.write("bad.logos", b"fn broken( {");

    let out = run_cmd(&["run", pstr(&bad)]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("helios:"), "stderr: {stderr}");

    let missing = dir.path("missing.logos");
    let out_missing = run_cmd(&["run", pstr(&missing)]);
    assert_eq!(out_missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_missing.stderr).contains("helios:"));
}

#[test]
fn record_and_replay_produce_identical_final_hash() {
    let dir = TempDir::new("rec_rep");
    let game = write_game(&dir);
    let inputs = dir.write(
        "inputs.txt",
        b"# move left, fire, release\n0 1\n15 0x11\n45 2\n90 0\n",
    );
    let rec_file = dir.path("session.trec");

    let rec_out = run_cmd(&["record", pstr(&game), pstr(&inputs), "-o", pstr(&rec_file)]);
    assert_eq!(
        rec_out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&rec_out.stderr)
    );
    let rec_stdout = String::from_utf8_lossy(&rec_out.stdout);
    assert!(
        rec_stdout.contains("final tick 91 hash 0x"),
        "stdout: {rec_stdout}"
    );

    let trec = fs::read(&rec_file).unwrap();
    assert!(trec.starts_with(b"TREC"));

    let rep_out = run_cmd(&["replay", pstr(&game), pstr(&rec_file)]);
    assert_eq!(
        rep_out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&rep_out.stderr)
    );
    let rep_stdout = String::from_utf8_lossy(&rep_out.stdout);
    assert_eq!(rec_stdout.trim(), rep_stdout.trim());

    // Verify inputs actually affected the hash compared to running with no inputs.
    let no_inputs = dir.write("empty.txt", b"");
    let empty_rec = dir.path("empty.trec");
    let empty_out = run_cmd(&[
        "record",
        pstr(&game),
        pstr(&no_inputs),
        "-o",
        pstr(&empty_rec),
        "--ticks",
        "91",
    ]);
    assert_eq!(empty_out.status.code(), Some(0));
    assert_ne!(
        rec_stdout.trim(),
        String::from_utf8_lossy(&empty_out.stdout).trim()
    );
}

#[test]
fn record_bad_input_exits_1() {
    let dir = TempDir::new("rec_bad");
    let game = write_game(&dir);
    let bad_inputs = dir.write("bad.txt", b"10 1\n5 2\n");
    let out_file = dir.path("out.trec");

    let out = run_cmd(&[
        "record",
        pstr(&game),
        pstr(&bad_inputs),
        "-o",
        pstr(&out_file),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("helios:"), "stderr: {stderr}");
}

#[test]
fn replay_bad_file_exits_1() {
    let dir = TempDir::new("rep_bad");
    let game = write_game(&dir);
    let bad_trec = dir.write("bad.trec", b"BAD_MAGIC_DATA");

    let out = run_cmd(&["replay", pstr(&game), pstr(&bad_trec)]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("helios:"), "stderr: {stderr}");
}

#[test]
fn stdin_and_stdout_dash_work() {
    let dir = TempDir::new("stdio");
    let game = write_game(&dir);

    let run_stdin = run_with_stdin(
        &["run", "-", "--ticks", "60"],
        helios::script::GAME_SRC.as_bytes(),
    );
    assert_eq!(run_stdin.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&run_stdin.stdout);
    assert!(stdout.contains("tick 60 hash 0x"));
    assert!(stdout.contains("final tick 60 hash 0x"));

    let inputs = dir.write("in.txt", b"0 1\n10 0\n");
    let rec_stdout = run_cmd(&["record", pstr(&game), pstr(&inputs), "-o", "-"]);
    assert_eq!(rec_stdout.status.code(), Some(0));
    assert!(rec_stdout.stdout.starts_with(b"TREC"));
}
