//! Regression tests for brief fix-chronos-2: merge, stash, show and tag, branch -d/-D, and export
//! into an empty or second git repository. One test per brief item; they run the real binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
            "chronos_fix2_{label}_{}_{nanos}_{seq}",
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

/// Run chronos in `dir` with no editor variables and no terminal on stdin.
fn chronos(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(dir)
        .env("CHRONOS_NAME", "tester")
        .env("CHRONOS_EMAIL", "tester@example.invalid")
        .env_remove("EDITOR")
        .env_remove("VISUAL")
        .stdin(Stdio::null())
        .args(args)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn err(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn ok(dir: &Path, args: &[&str]) -> Output {
    let out = chronos(dir, args);
    assert!(out.status.success(), "{args:?}: {}", err(&out));
    out
}

fn write(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap()
}

/// As `read`, with CRLF folded to LF: git on Windows may check files out with autocrlf.
fn read_lf(dir: &Path, name: &str) -> String {
    read(dir, name).replace("\r\n", "\n")
}

/// A repo on `main` with one commit containing `f.txt` = "a\nb\nc\n" and `g.txt`.
fn base_repo(dir: &Path) {
    ok(dir, &["init"]);
    write(dir, "f.txt", "a\nb\nc\n");
    write(dir, "g.txt", "keep\n");
    ok(dir, &["add", "."]);
    ok(dir, &["commit", "-m", "base"]);
}

fn commit_all(dir: &Path, file: &str, body: &str, msg: &str) {
    write(dir, file, body);
    ok(dir, &["add", file]);
    ok(dir, &["commit", "-m", msg]);
}

/// Number of `parent` lines in the commit HEAD names (2 for a merge commit).
fn parents_of_head(dir: &Path) -> usize {
    text(&ok(dir, &["cat-file", "HEAD"]))
        .lines()
        .filter(|l| l.starts_with("parent "))
        .count()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ---- item 1: merge ------------------------------------------------------------------------

#[test]
fn chronos_2_item1_merge_fast_forward() {
    let t = TempDir::new("ff");
    let dir = t.path();
    base_repo(dir);
    ok(dir, &["branch", "feat"]);
    ok(dir, &["checkout", "feat"]);
    commit_all(dir, "f.txt", "a\nb\nc\nd\n", "feat");
    ok(dir, &["checkout", "main"]);
    let out = ok(dir, &["merge", "feat"]);
    assert!(text(&out).contains("Fast-forward"), "{}", text(&out));
    assert_eq!(read(dir, "f.txt"), "a\nb\nc\nd\n");
    // No merge commit: HEAD is now feat's commit, whose only parent is base.
    assert_eq!(parents_of_head(dir), 1);
}

#[test]
fn chronos_2_item1_merge_conflict_markers_then_commit() {
    let t = TempDir::new("conflict");
    let dir = t.path();
    base_repo(dir);
    ok(dir, &["branch", "feat"]);
    ok(dir, &["checkout", "feat"]);
    commit_all(dir, "f.txt", "a\nX\nc\n", "feat");
    ok(dir, &["checkout", "main"]);
    commit_all(dir, "f.txt", "a\nY\nc\n", "main");

    let out = chronos(dir, &["merge", "feat"]);
    assert_eq!(out.status.code(), Some(1), "{}", err(&out));
    assert!(text(&out).contains("CONFLICT (content): Merge conflict in f.txt"));
    let body = read(dir, "f.txt");
    assert!(
        body.contains("<<<<<<< HEAD\nY\n=======\nX\n>>>>>>> feat\n"),
        "{body}"
    );
    assert!(dir.join(".chronos").join("MERGE_HEAD").is_file());
    let status = text(&ok(dir, &["status"]));
    assert!(status.contains("Unmerged paths:"), "{status}");
    assert!(status.contains("f.txt"), "{status}");

    // A commit while paths are unmerged is refused.
    assert!(!chronos(dir, &["commit", "-m", "too early"])
        .status
        .success());

    ok(dir, &["add", "f.txt"]);
    ok(dir, &["commit", "-m", "resolved"]);
    assert_eq!(parents_of_head(dir), 2);
    assert!(!dir.join(".chronos").join("MERGE_HEAD").exists());
}

#[test]
fn chronos_2_item1_merge_abort_restores_head() {
    let t = TempDir::new("abort");
    let dir = t.path();
    base_repo(dir);
    ok(dir, &["branch", "feat"]);
    ok(dir, &["checkout", "feat"]);
    commit_all(dir, "f.txt", "a\nX\nc\n", "feat");
    ok(dir, &["checkout", "main"]);
    commit_all(dir, "f.txt", "a\nY\nc\n", "main");
    let before = read(dir, "f.txt");

    assert_eq!(chronos(dir, &["merge", "feat"]).status.code(), Some(1));
    ok(dir, &["merge", "--abort"]);
    assert_eq!(read(dir, "f.txt"), before);
    assert!(!dir.join(".chronos").join("MERGE_HEAD").exists());
    let status = text(&ok(dir, &["status"]));
    assert!(!status.contains("Unmerged paths:"), "{status}");
}

#[test]
fn chronos_2_item1_merge_refuses_dirty_file_before_writing() {
    let t = TempDir::new("dirty");
    let dir = t.path();
    base_repo(dir);
    ok(dir, &["branch", "feat"]);
    ok(dir, &["checkout", "feat"]);
    commit_all(dir, "f.txt", "a\nb\nc\nd\n", "feat");
    ok(dir, &["checkout", "main"]);
    write(dir, "f.txt", "local edit\n");

    let out = chronos(dir, &["merge", "feat"]);
    assert!(!out.status.success());
    assert!(err(&out).contains("would be overwritten"), "{}", err(&out));
    assert_eq!(read(dir, "f.txt"), "local edit\n");
    assert!(!dir.join(".chronos").join("MERGE_HEAD").exists());
    assert_eq!(parents_of_head(dir), 0);
}

// ---- item 2: stash ------------------------------------------------------------------------

#[test]
fn chronos_2_item2_stash_push_pop_keeps_untracked() {
    let t = TempDir::new("stash");
    let dir = t.path();
    base_repo(dir);
    write(dir, "f.txt", "changed\n");
    write(dir, "new.txt", "untracked\n");
    ok(dir, &["add", "f.txt"]);
    write(dir, "f.txt", "changed again\n");

    ok(dir, &["stash"]);
    assert_eq!(read(dir, "f.txt"), "a\nb\nc\n");
    assert_eq!(
        read(dir, "new.txt"),
        "untracked\n",
        "untracked files are left alone"
    );
    let list = text(&ok(dir, &["stash", "list"]));
    assert!(list.starts_with("stash@{0}: WIP on main"), "{list}");

    ok(dir, &["stash", "pop"]);
    assert_eq!(read(dir, "f.txt"), "changed again\n");
    assert!(text(&ok(dir, &["stash", "list"])).is_empty());
}

#[test]
fn chronos_2_item2_stash_pop_refused_keeps_stash() {
    let t = TempDir::new("stash_refuse");
    let dir = t.path();
    base_repo(dir);
    write(dir, "f.txt", "stashed\n");
    ok(dir, &["stash"]);
    write(dir, "f.txt", "new local work\n");

    let out = chronos(dir, &["stash", "pop"]);
    assert!(!out.status.success());
    assert_eq!(read(dir, "f.txt"), "new local work\n");
    assert_eq!(text(&ok(dir, &["stash", "list"])).lines().count(), 1);

    ok(dir, &["stash", "drop"]);
    assert!(text(&ok(dir, &["stash", "list"])).is_empty());
}

#[test]
fn chronos_2_item2_stash_without_changes_fails() {
    let t = TempDir::new("stash_none");
    let dir = t.path();
    base_repo(dir);
    let out = chronos(dir, &["stash"]);
    assert!(!out.status.success());
    assert!(err(&out).contains("No local changes"), "{}", err(&out));
}

// ---- item 3: show and tag -----------------------------------------------------------------

#[test]
fn chronos_2_item3_show_header_and_diff() {
    let t = TempDir::new("show");
    let dir = t.path();
    base_repo(dir);
    commit_all(dir, "f.txt", "a\nb\nc\nd\n", "add d");
    let out = text(&ok(dir, &["show"]));
    assert!(out.starts_with("commit "), "{out}");
    assert!(
        out.contains("Author: tester <tester@example.invalid>"),
        "{out}"
    );
    assert!(out.contains("    add d"), "{out}");
    assert!(out.contains("+d"), "{out}");
    assert!(
        !out.contains("+keep"),
        "only the diff against the parent: {out}"
    );

    let by_branch = text(&ok(dir, &["show", "main"]));
    assert!(by_branch.contains("    add d"));
    assert!(!chronos(dir, &["show", "nope"]).status.success());
}

#[test]
fn chronos_2_item3_tag_create_list_delete() {
    let t = TempDir::new("tag");
    let dir = t.path();
    base_repo(dir);
    commit_all(dir, "f.txt", "a\nb\nc\nd\n", "two");
    ok(dir, &["tag", "v1", "HEAD~1"]);
    ok(dir, &["tag", "v2"]);
    assert_eq!(text(&ok(dir, &["tag"])), "v1\nv2\n");
    assert!(
        !chronos(dir, &["tag", "v1"]).status.success(),
        "duplicate tag refused"
    );
    let shown = text(&ok(dir, &["show", "v1"]));
    assert!(shown.contains("    base"), "{shown}");
    ok(dir, &["tag", "-d", "v1"]);
    assert_eq!(text(&ok(dir, &["tag"])), "v2\n");
    assert!(!chronos(dir, &["tag", "-d", "v1"]).status.success());
}

// ---- item 4: branch -d / -D ---------------------------------------------------------------

#[test]
fn chronos_2_item4_branch_delete_rules() {
    let t = TempDir::new("branchd");
    let dir = t.path();
    base_repo(dir);
    ok(dir, &["branch", "merged"]);
    ok(dir, &["branch", "unmerged"]);
    ok(dir, &["checkout", "unmerged"]);
    commit_all(dir, "g.txt", "other\n", "side");
    ok(dir, &["checkout", "main"]);

    let current = chronos(dir, &["branch", "-d", "main"]);
    assert!(!current.status.success(), "{}", text(&current));
    let refused = chronos(dir, &["branch", "-d", "unmerged"]);
    assert!(!refused.status.success());
    assert!(
        err(&refused).contains("not fully merged"),
        "{}",
        err(&refused)
    );
    ok(dir, &["branch", "-d", "merged"]);
    ok(dir, &["branch", "-D", "unmerged"]);
    assert_eq!(text(&ok(dir, &["branch"])), "* main\n");
    assert!(!chronos(dir, &["branch", "-d", "gone"]).status.success());
}

// ---- item 5: export -----------------------------------------------------------------------

/// A chronos repo with two commits on `main`, in `dir/src`.
fn two_commit_src(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    base_repo(&src);
    commit_all(&src, "f.txt", "a\nb\nc\nd\n", "two");
    src
}

#[test]
fn chronos_2_item5_export_into_empty_git_repo() {
    let t = TempDir::new("export_empty");
    let dir = t.path();
    let src = two_commit_src(dir);
    let target = dir.join("exp");
    fs::create_dir_all(&target).unwrap();
    git(&target, &["init", "-q"]);

    let target_str = target.to_str().unwrap();
    ok(&src, &["export", target_str]);
    assert_eq!(git(&target, &["rev-list", "--count", "main"]).trim(), "2");
    assert_eq!(
        git(&target, &["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/main"
    );
    assert_eq!(read_lf(&target, "f.txt"), "a\nb\nc\nd\n");
    assert!(text(&ok(&src, &["export", target_str])).contains("nothing to export"));
}

#[test]
fn chronos_2_item5_export_second_target_gets_full_history() {
    let t = TempDir::new("export_two");
    let dir = t.path();
    let src = two_commit_src(dir);
    let first = dir.join("first");
    let second = dir.join("second");
    for target in [&first, &second] {
        fs::create_dir_all(target).unwrap();
        git(target, &["init", "-q"]);
    }
    ok(&src, &["export", first.to_str().unwrap()]);
    ok(&src, &["export", second.to_str().unwrap()]);
    assert_eq!(git(&second, &["rev-list", "--count", "main"]).trim(), "2");
    assert_eq!(git(&first, &["rev-list", "--count", "main"]).trim(), "2");

    // New work after the second target exists goes to it too.
    commit_all(&src, "g.txt", "third\n", "three");
    let out = text(&ok(&src, &["export", second.to_str().unwrap()]));
    assert!(out.contains("exported 1 commits"), "{out}");
    assert_eq!(git(&second, &["rev-list", "--count", "main"]).trim(), "3");
}

#[test]
fn chronos_2_item5_export_accepts_dot_git_folder() {
    let t = TempDir::new("export_dotgit");
    let dir = t.path();
    let src = two_commit_src(dir);
    let target = dir.join("exp");
    fs::create_dir_all(&target).unwrap();
    git(&target, &["init", "-q"]);
    let dot_git = target.join(".git");
    ok(&src, &["export", dot_git.to_str().unwrap()]);
    assert_eq!(git(&target, &["rev-list", "--count", "main"]).trim(), "2");
    assert_eq!(read_lf(&target, "f.txt"), "a\nb\nc\nd\n");
}
