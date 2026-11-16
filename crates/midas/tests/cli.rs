//! CLI integration tests for midas.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_midas")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "midas_cli_{label}_{}_{}_{}",
            std::process::id(),
            nanos,
            seq
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_and_version_and_unknown() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("sim"));

    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "midas 0.1.0");

    let out = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("midas:"));
}

#[test]
fn sim_good_and_errors() {
    let tmp = TempDir::new("sim");
    let trace = tmp.path().join("trace.txt");
    fs::write(&trace, "heap 65536\nalloc 24\nalloc 8192\nfree 0\nfree 1\n").unwrap();

    let out = Command::new(bin()).arg("sim").arg(&trace).output().unwrap();
    assert!(out.status.success(), "{:?}", out);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("used=32 "));
    assert!(stdout.contains("used=8224 "));
    assert!(stdout.contains("used=8192 "));
    assert!(stdout.contains("used=0 "));
    assert!(stdout.contains("check=true"));

    // Double free -> exit 1 with midas:
    let df_trace = tmp.path().join("double_free.txt");
    fs::write(&df_trace, "heap 65536\nalloc 64\nfree 0\nfree 0\n").unwrap();
    let out = Command::new(bin())
        .arg("sim")
        .arg(&df_trace)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("midas:"));

    // Bad index -> exit 1 with midas:
    let bad_idx = tmp.path().join("bad_idx.txt");
    fs::write(&bad_idx, "heap 65536\nfree 9\n").unwrap();
    let out = Command::new(bin())
        .arg("sim")
        .arg(&bad_idx)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("midas:"));
}

#[test]
fn help_starts_with_tool_line_and_example_runs() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines[0].starts_with("midas - "), "{}", lines[0]);
    assert_eq!(lines[1], "Usage:");

    // The three lines under "runs as given" are a complete input file.
    let at = lines
        .iter()
        .position(|l| l.contains("runs as given"))
        .unwrap();
    let example: String = lines[at + 1..at + 4]
        .iter()
        .map(|l| format!("{}\n", l.trim()))
        .collect();
    let dir = TempDir::new("help_example");
    let file = dir.path().join("example.txt");
    fs::write(&file, example).unwrap();
    let run = Command::new(bin()).arg("sim").arg(&file).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
