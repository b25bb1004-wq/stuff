//! CLI integration tests for kairos.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_kairos")
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
            "kairos_cli_{label}_{}_{}_{}",
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
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "kairos 0.1.0"
    );

    let out = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("kairos:"));
}

#[test]
fn sim_deterministic_and_bad_input() {
    let tmp = TempDir::new("sim");
    let sim_file = tmp.path().join("work.txt");
    fs::write(
        &sim_file,
        "cpus 2\ntask heavy 200 10\ntask light 100 10\ntask mid 100 10\n",
    )
    .unwrap();

    let out1 = Command::new(bin())
        .arg("sim")
        .arg(&sim_file)
        .output()
        .unwrap();
    assert!(out1.status.success(), "{:?}", out1);
    let out2 = Command::new(bin())
        .arg("sim")
        .arg(&sim_file)
        .output()
        .unwrap();
    assert!(out2.status.success());
    assert_eq!(out1.stdout, out2.stdout);

    let s = String::from_utf8(out1.stdout).unwrap();
    assert!(s.contains("cpu 0:"));
    assert!(s.contains("cpu 1:"));
    assert!(s.contains("task heavy: ticks=10"));
    assert!(s.contains("task light: ticks=10"));

    // Bad input -> exit 1 with kairos:
    let bad_file = tmp.path().join("bad.txt");
    fs::write(&bad_file, "cpus 0\ntask a 1 1\n").unwrap();
    let out = Command::new(bin())
        .arg("sim")
        .arg(&bad_file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("kairos:"));
}
