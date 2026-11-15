//! CLI integration tests for chronos.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chronos::{hash_object, to_hex, Kind};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_chronos")
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
            "chronos_cli_{label}_{}_{}_{}",
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

fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_and_version_and_unknown() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    for sub in [
        "init", "add", "commit", "log", "branch", "checkout", "cat-file", "status",
    ] {
        assert!(stdout.contains(sub), "help missing {sub}");
    }

    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "chronos 0.1.0"
    );

    let out = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("chronos:"));
}

#[test]
fn workflow_branches_checkout_roundtrip_and_cat_file() {
    let tmp = TempDir::new("workflow");
    let repo_dir = tmp.path().join("repo");

    let out = Command::new(bin())
        .arg("init")
        .arg(&repo_dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert!(repo_dir.join(".chronos/HEAD").is_file());

    // Known blob hash test
    fs::write(repo_dir.join("hello.txt"), b"hello\n").unwrap();
    let expected_blob_id = to_hex(&hash_object(Kind::Blob, b"hello\n"));

    let out = run_git(&repo_dir, &["add", "hello.txt"]);
    assert!(out.status.success(), "{:?}", out);

    let out = run_git(&repo_dir, &["status"]);
    assert!(out.status.success());
    let status_s = String::from_utf8(out.stdout).unwrap();
    assert!(status_s.contains("A hello.txt"), "status: {status_s}");

    let out = run_git(&repo_dir, &["commit", "-m", "initial"]);
    assert!(out.status.success(), "{:?}", out);
    let c1_hex = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_eq!(c1_hex.len(), 64);

    // cat-file on known blob id matches library hash_object
    let out = run_git(&repo_dir, &["cat-file", &expected_blob_id]);
    assert!(out.status.success(), "{:?}", out);
    assert_eq!(out.stdout, b"hello\n");

    // Create second branch and switch to it
    let out = run_git(&repo_dir, &["branch", "topic"]);
    assert!(out.status.success(), "{:?}", out);

    let out = run_git(&repo_dir, &["branch"]);
    assert!(out.status.success());
    let br_s = String::from_utf8(out.stdout).unwrap();
    assert!(br_s.contains("* main"));
    assert!(br_s.contains("topic"));

    let out = run_git(&repo_dir, &["checkout", "topic"]);
    assert!(out.status.success(), "{:?}", out);

    fs::write(repo_dir.join("topic.txt"), b"on topic\n").unwrap();
    assert!(run_git(&repo_dir, &["add", "topic.txt"]).status.success());
    let out = run_git(&repo_dir, &["commit", "-m", "topic commit"]);
    assert!(out.status.success());

    let out = run_git(&repo_dir, &["log"]);
    assert!(out.status.success());
    let log_s = String::from_utf8(out.stdout).unwrap();
    assert!(log_s.contains("topic commit"));
    assert!(log_s.contains("initial"));
    assert!(log_s.contains(&c1_hex));

    // Checkout back to main removes topic.txt; checkout back to topic restores it
    assert!(run_git(&repo_dir, &["checkout", "main"]).status.success());
    assert!(!repo_dir.join("topic.txt").exists());
    assert!(repo_dir.join("hello.txt").exists());

    assert!(run_git(&repo_dir, &["checkout", "topic"]).status.success());
    assert_eq!(fs::read(repo_dir.join("topic.txt")).unwrap(), b"on topic\n");
}

#[test]
fn bad_inputs_exit_one() {
    let tmp = TempDir::new("bad");
    let file_not_dir = tmp.path().join("afile");
    fs::write(&file_not_dir, b"x").unwrap();

    // init on a file
    let out = Command::new(bin())
        .arg("init")
        .arg(&file_not_dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("chronos:"));

    // status outside repo
    let out = run_git(tmp.path(), &["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("chronos:"));

    // log outside / empty repo
    let repo_dir = tmp.path().join("r");
    assert!(Command::new(bin())
        .arg("init")
        .arg(&repo_dir)
        .output()
        .unwrap()
        .status
        .success());

    for args in [
        &["add", "missing.txt"][..],
        &["commit", "-m", "empty"][..],
        &["log"][..],
        &["branch", "bad..name"][..],
        &["checkout", "nope"][..],
        &["cat-file", "deadbeef"][..],
    ] {
        let out = run_git(&repo_dir, args);
        assert_eq!(out.status.code(), Some(1), "expected exit 1 for {:?}", args);
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .starts_with("chronos:"),
            "expected chronos: prefix for {:?}",
            args
        );
    }
}
