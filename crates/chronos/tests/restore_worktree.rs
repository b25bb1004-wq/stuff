//! `chronos restore PATH...` (work tree from the index) and `--source REV`.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "chronos_restore_{label}_{}_{nanos}_{seq}",
            std::process::id()
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

fn chronos(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_chronos"))
        .current_dir(dir)
        .env_remove("EDITOR")
        .env_remove("VISUAL")
        .stdin(Stdio::null())
        .args(args)
        .output()
        .unwrap()
}

fn ok(dir: &Path, args: &[&str]) -> Output {
    let out = chronos(dir, args);
    assert!(out.status.success(), "{args:?}: {}", err(&out));
    out
}

fn err(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, body).unwrap();
}

fn commit(dir: &Path, msg: &str) {
    ok(dir, &["add", "."]);
    ok(dir, &["commit", "-m", msg]);
}

fn init(dir: &Path) {
    ok(dir, &["init"]);
}

#[test]
fn deleted_file_comes_back_identical_and_clean() {
    let tmp = TempDir::new("deleted");
    let r = tmp.path();
    init(r);
    let bytes = b"line one\r\n\x00binary\xff tail\n";
    fs::write(r.join("n.txt"), bytes).unwrap();
    commit(r, "base");

    fs::remove_file(r.join("n.txt")).unwrap();
    let out = ok(r, &["restore", "n.txt"]);
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert_eq!(fs::read(r.join("n.txt")).unwrap(), bytes);

    let status = stdout(&ok(r, &["status"]));
    assert!(!status.contains("n.txt"), "{status}");
}

#[test]
fn deleted_file_in_folder_recreates_folders() {
    let tmp = TempDir::new("folder");
    let r = tmp.path();
    init(r);
    write(r, "dir/sub/x.txt", "x\n");
    commit(r, "base");

    fs::remove_dir_all(r.join("dir")).unwrap();
    ok(r, &["restore", "dir/sub/x.txt"]);
    assert_eq!(fs::read_to_string(r.join("dir/sub/x.txt")).unwrap(), "x\n");
}

#[test]
fn edited_file_is_reset_to_staged_content() {
    let tmp = TempDir::new("edited");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "one\n");
    commit(r, "base");
    write(r, "a.txt", "staged\n");
    ok(r, &["add", "a.txt"]);
    write(r, "a.txt", "edited again\n");

    ok(r, &["restore", "a.txt"]);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "staged\n");
    // The index still holds the staged version, so the file now matches it: no unstaged change.
    let status = stdout(&ok(r, &["status"]));
    assert!(!status.contains("Changes not staged"), "{status}");
    assert!(status.contains("M a.txt"), "{status}");
}

#[test]
fn source_head_1_restores_the_older_version() {
    let tmp = TempDir::new("source_head");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "v1\n");
    commit(r, "v1");
    write(r, "a.txt", "v2\n");
    commit(r, "v2");

    let out = ok(r, &["--json", "restore", "--source", "HEAD~1", "a.txt"]);
    assert!(stdout(&out).contains("\"paths\""), "{}", stdout(&out));
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "v1\n");
}

#[test]
fn source_tag_restores_that_commit() {
    let tmp = TempDir::new("source_tag");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "old\n");
    commit(r, "old");
    ok(r, &["tag", "v1"]);
    write(r, "a.txt", "new\n");
    commit(r, "new");

    ok(r, &["restore", "--source", "v1", "a.txt"]);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "old\n");
}

#[test]
fn unknown_path_is_exit_1_and_names_the_path() {
    let tmp = TempDir::new("unknown");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "one\n");
    commit(r, "base");

    let out = chronos(r, &["restore", "nope.txt"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(err(&out).contains("nope.txt"), "{}", err(&out));
    assert!(!r.join("nope.txt").exists());

    let json = chronos(r, &["--json", "restore", "nope.txt"]);
    assert_eq!(json.status.code(), Some(1));
    assert!(stdout(&json).contains("not_found"), "{}", stdout(&json));
}

#[test]
fn unknown_path_writes_nothing_even_with_a_valid_path_first() {
    let tmp = TempDir::new("partial");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "one\n");
    write(r, "b.txt", "two\n");
    commit(r, "base");
    fs::remove_file(r.join("a.txt")).unwrap();

    let out = chronos(r, &["restore", "a.txt", "nope.txt"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!r.join("a.txt").exists());
}

#[test]
fn unknown_source_revision_is_an_error() {
    let tmp = TempDir::new("badrev");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "one\n");
    commit(r, "base");

    let out = chronos(r, &["restore", "--source", "no-such-rev", "a.txt"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(err(&out).contains("no-such-rev"), "{}", err(&out));
}

#[test]
fn staged_mode_still_unstages_and_leaves_the_work_tree() {
    let tmp = TempDir::new("staged");
    let r = tmp.path();
    init(r);
    write(r, "a.txt", "one\n");
    commit(r, "base");
    write(r, "a.txt", "two\n");
    ok(r, &["add", "a.txt"]);

    ok(r, &["restore", "--staged", "a.txt"]);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "two\n");
    let status = stdout(&ok(r, &["status"]));
    assert!(status.contains("a.txt"), "{status}");
}
