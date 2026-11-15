//! CLI integration tests for calliope.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_calliope")
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
            "calliope_cli_{label}_{}_{}_{}",
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
    for sub in ["grep", "edit"] {
        assert!(stdout.contains(sub), "help missing {sub}");
    }

    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "calliope 0.1.0"
    );

    let out = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("calliope:"));
}

#[test]
fn grep_match_nomatch_missing_and_bad_regex() {
    let tmp = TempDir::new("grep");
    let file = tmp.path().join("sample.txt");
    fs::write(&file, "alpha one\nbeta two\ngamma 42\n").unwrap();

    let out = Command::new(bin())
        .arg("grep")
        .arg("[0-9]+")
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, format!("{}:3:7: gamma 42\n", file.display()));

    // No match -> exit 1
    let out = Command::new(bin())
        .arg("grep")
        .arg("zzz")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    // Missing file -> exit 1 with calliope:
    let out = Command::new(bin())
        .arg("grep")
        .arg("alpha")
        .arg(tmp.path().join("missing.txt"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("calliope:"));

    // Bad regex -> exit 2 with calliope:
    let out = Command::new(bin())
        .arg("grep")
        .arg("[unclosed")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("calliope:"));
}

#[test]
fn edit_script_multicursor_undo_redo_save_and_bad() {
    let tmp = TempDir::new("edit");
    let file = tmp.path().join("doc.txt");
    let script = tmp.path().join("ops.txt");

    fs::write(&file, "foo\nbar\nbaz\n").unwrap();
    fs::write(
        &script,
        "goto 1 1\ncursor add 2 1\ninsert > \nundo\nredo\ngoto 3 1\ndelete 3\ninsert qux\nsave\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .arg("edit")
        .arg(&script)
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let expected = "> foo\n> bar\nqux\n";
    assert_eq!(String::from_utf8(out.stdout).unwrap(), expected);
    assert_eq!(fs::read_to_string(&file).unwrap(), expected);

    // Bad script command -> exit 1 with calliope:
    let bad_script = tmp.path().join("bad.txt");
    fs::write(&bad_script, "goto 999 1\n").unwrap();
    let out = Command::new(bin())
        .arg("edit")
        .arg(&bad_script)
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("calliope:"));
}
