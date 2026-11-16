//! Regression tests for brief fix-chronos-1: git-compatible staging, status, unstaging, rm/mv,
//! switch with local changes, and the commit-message guard. One test per brief item.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chronos::ignore::Rules;

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
            "chronos_fix1_{label}_{}_{nanos}_{seq}",
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

fn init_with_commit(dir: &Path, files: &[(&str, &str)]) {
    ok(dir, &["init"]);
    for (name, body) in files {
        write(dir, name, body);
    }
    ok(dir, &["add", "."]);
    ok(dir, &["commit", "-m", "one"]);
}

fn write(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// Lines of `status` output, so assertions can match whole entries.
fn status_lines(dir: &Path) -> Vec<String> {
    text(&ok(dir, &["status"]))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The status section that starts with `heading` (up to the next heading or the end).
fn section(lines: &[String], heading: &str) -> Vec<String> {
    let Some(start) = lines.iter().position(|l| l == heading) else {
        return Vec::new();
    };
    lines[start + 1..]
        .iter()
        .take_while(|l| !l.ends_with(':'))
        .cloned()
        .collect()
}

#[test]
fn chronos_1_item1_deleted_tracked_paths_stage_removal() {
    let tmp = TempDir::new("item1");
    let r = tmp.path();
    init_with_commit(
        r,
        &[
            ("keep.txt", "k\n"),
            ("gone.txt", "g\n"),
            ("dir/inner.txt", "i\n"),
        ],
    );

    fs::remove_file(r.join("gone.txt")).unwrap();
    fs::remove_file(r.join("dir/inner.txt")).unwrap();
    assert!(section(&status_lines(r), "Changes not staged for commit:")
        .contains(&"D gone.txt".to_string()));

    // `add .` stages both removals (a file and a folder's file), and commit records them.
    ok(r, &["add", "."]);
    let staged = section(&status_lines(r), "Changes to be committed:");
    assert!(staged.contains(&"D gone.txt".to_string()), "{staged:?}");
    assert!(
        staged.contains(&"D dir/inner.txt".to_string()),
        "{staged:?}"
    );
    ok(r, &["commit", "-m", "two"]);
    assert_eq!(status_lines(r).len(), 1, "only the header line when clean");

    let commit = text(&ok(r, &["cat-file", "HEAD"]));
    let tree = commit
        .lines()
        .find_map(|l| l.strip_prefix("tree "))
        .unwrap()
        .to_string();
    let listing = text(&ok(r, &["cat-file", &tree]));
    assert!(listing.contains("keep.txt"), "{listing}");
    assert!(!listing.contains("gone.txt"), "{listing}");
    assert!(!listing.contains("dir"), "{listing}");

    // Naming one deleted tracked file also works.
    let other = TempDir::new("item1b");
    let o = other.path();
    init_with_commit(o, &[("solo.txt", "s\n")]);
    fs::remove_file(o.join("solo.txt")).unwrap();
    ok(o, &["add", "solo.txt"]);
    ok(o, &["commit", "-m", "removed solo"]);
    assert_eq!(status_lines(o).len(), 1);
}

#[test]
fn chronos_1_item2_add_honours_gitignore() {
    let tmp = TempDir::new("item2");
    let r = tmp.path();
    init_with_commit(r, &[("base.txt", "base\n")]);
    write(
        r,
        ".gitignore",
        "# build output\n\ntarget/\n*.log\n/root.txt\n!keep.log\na?c.txt\n**/tmp\n",
    );
    write(r, "sub/.gitignore", "secret\n");
    for f in [
        "target/leak.txt",
        "debug.log",
        "keep.log",
        "root.txt",
        "sub/root.txt",
        "abc.txt",
        "x/tmp/y.txt",
        "x/tmpx.txt",
        "sub/secret",
        "secret.txt",
        "visible.txt",
    ] {
        write(r, f, "data\n");
    }

    ok(r, &["add", "."]);
    let staged = section(&status_lines(r), "Changes to be committed:");
    for want in [
        "A keep.log",
        "A sub/root.txt",
        "A secret.txt",
        "A x/tmpx.txt",
        "A visible.txt",
        "A .gitignore",
        "A sub/.gitignore",
    ] {
        assert!(
            staged.contains(&want.to_string()),
            "missing {want}: {staged:?}"
        );
    }
    for bad in [
        "A target/leak.txt",
        "A debug.log",
        "A root.txt",
        "A abc.txt",
        "A x/tmp/y.txt",
        "A sub/secret",
    ] {
        assert!(
            !staged.contains(&bad.to_string()),
            "staged {bad}: {staged:?}"
        );
    }
    assert!(!status_lines(r).iter().any(|l| l.contains(".chronos")));

    // A named ignored file is refused like git, and -f overrides.
    let refused = chronos(r, &["add", "target/leak.txt"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        err(&refused).contains("paths are ignored"),
        "{}",
        err(&refused)
    );
    assert!(err(&refused).contains("use -f"), "{}", err(&refused));
    ok(r, &["add", "-f", "target/leak.txt"]);
    assert!(section(&status_lines(r), "Changes to be committed:")
        .contains(&"A target/leak.txt".to_string()));

    // The matcher directly: anchoring, dir-only, negation and `**` in the middle.
    let mut rules = Rules::new();
    rules.add("", "build/\n/anchored\na/**/b\nst*r.txt\n");
    assert!(!rules.is_ignored("build", false));
    assert!(rules.is_ignored("build", true));
    assert!(rules.is_ignored("q/build", true));
    assert!(rules.is_ignored("build/x.o", false));
    assert!(rules.is_ignored("anchored", false));
    assert!(!rules.is_ignored("d/anchored", false));
    assert!(rules.is_ignored("a/b", false));
    assert!(rules.is_ignored("a/x/y/b", false));
    assert!(!rules.is_ignored("ab", false));
    assert!(rules.is_ignored("star.txt", false));
    assert!(!rules.is_ignored("st/r.txt", false));
}

#[test]
fn chronos_1_item3_status_lists_untracked_files() {
    let tmp = TempDir::new("item3");
    let r = tmp.path();
    init_with_commit(r, &[("t.txt", "t\n")]);
    write(r, ".gitignore", "ign.txt\n");
    write(r, "ign.txt", "x\n");
    write(r, "new.txt", "n\n");
    write(r, "dir/a.txt", "a\n");
    write(r, "dir/deep/b.txt", "b\n");

    let lines = status_lines(r);
    let untracked = section(&lines, "Untracked files:");
    assert!(
        untracked.contains(&"? new.txt".to_string()),
        "{untracked:?}"
    );
    assert!(untracked.contains(&"? dir/".to_string()), "{untracked:?}");
    assert!(
        untracked.contains(&"? .gitignore".to_string()),
        "{untracked:?}"
    );
    assert!(!lines.iter().any(|l| l.contains("ign.txt")), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("dir/a.txt")), "{lines:?}");

    // Once something in dir/ is tracked, the folder is no longer collapsed.
    ok(r, &["add", "dir/a.txt"]);
    let untracked = section(&status_lines(r), "Untracked files:");
    assert!(
        untracked.contains(&"? dir/deep/".to_string()),
        "{untracked:?}"
    );
    assert!(!untracked.contains(&"? dir/".to_string()), "{untracked:?}");
}

#[test]
fn chronos_1_item4_add_all_stages_new_modified_and_deleted() {
    let tmp = TempDir::new("item4");
    let r = tmp.path();
    init_with_commit(r, &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    write(r, "a.txt", "a2\n");
    fs::remove_file(r.join("b.txt")).unwrap();
    write(r, "c.txt", "c\n");
    write(r, "ignored.log", "x\n");
    write(r, ".gitignore", "*.log\n");

    ok(r, &["add", "-A"]);
    let staged = section(&status_lines(r), "Changes to be committed:");
    for want in ["M a.txt", "D b.txt", "A c.txt", "A .gitignore"] {
        assert!(
            staged.contains(&want.to_string()),
            "missing {want}: {staged:?}"
        );
    }
    assert!(
        !staged.iter().any(|l| l.contains("ignored.log")),
        "{staged:?}"
    );
    let long = chronos(r, &["add", "--all"]);
    assert!(long.status.success(), "{}", err(&long));
}

#[test]
fn chronos_1_item5_restore_staged_and_reset_unstage() {
    let tmp = TempDir::new("item5");
    let r = tmp.path();
    init_with_commit(r, &[("a.txt", "one\n")]);

    write(r, "a.txt", "two\n");
    ok(r, &["add", "a.txt"]);
    assert!(section(&status_lines(r), "Changes to be committed:").contains(&"M a.txt".to_string()));
    ok(r, &["restore", "--staged", "a.txt"]);
    let lines = status_lines(r);
    assert!(
        section(&lines, "Changes to be committed:").is_empty(),
        "{lines:?}"
    );
    assert!(section(&lines, "Changes not staged for commit:").contains(&"M a.txt".to_string()));
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "two\n");

    write(r, "n.txt", "new\n");
    ok(r, &["add", "n.txt"]);
    ok(r, &["reset"]);
    let lines = status_lines(r);
    assert!(
        section(&lines, "Changes to be committed:").is_empty(),
        "{lines:?}"
    );
    assert!(section(&lines, "Untracked files:").contains(&"? n.txt".to_string()));
    assert_eq!(fs::read_to_string(r.join("n.txt")).unwrap(), "new\n");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "two\n");

    let unknown = chronos(r, &["restore", "--staged", "nope.txt"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(err(&unknown).starts_with("chronos:"));
}

#[test]
fn chronos_1_item6_rm_and_mv() {
    let tmp = TempDir::new("item6");
    let r = tmp.path();
    init_with_commit(
        r,
        &[
            ("a.txt", "a\n"),
            ("b.txt", "b\n"),
            ("c.txt", "c\n"),
            ("m.txt", "m\n"),
        ],
    );

    ok(r, &["rm", "a.txt"]);
    assert!(!r.join("a.txt").exists());
    assert!(section(&status_lines(r), "Changes to be committed:").contains(&"D a.txt".to_string()));

    ok(r, &["rm", "--cached", "b.txt"]);
    assert!(r.join("b.txt").exists());
    let lines = status_lines(r);
    assert!(section(&lines, "Changes to be committed:").contains(&"D b.txt".to_string()));
    assert!(section(&lines, "Untracked files:").contains(&"? b.txt".to_string()));

    write(r, "c.txt", "changed\n");
    let refused = chronos(r, &["rm", "c.txt"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        err(&refused).contains("uncommitted changes"),
        "{}",
        err(&refused)
    );
    assert!(r.join("c.txt").exists());
    ok(r, &["rm", "-f", "c.txt"]);
    assert!(!r.join("c.txt").exists());

    ok(r, &["mv", "m.txt", "n.txt"]);
    assert!(!r.join("m.txt").exists());
    assert_eq!(fs::read_to_string(r.join("n.txt")).unwrap(), "m\n");
    let lines = status_lines(r);
    let staged = section(&lines, "Changes to be committed:");
    assert!(staged.contains(&"A n.txt".to_string()), "{staged:?}");
    assert!(staged.contains(&"D m.txt".to_string()), "{staged:?}");

    let help = text(&ok(r, &["--help"]));
    for want in [
        "restore --staged PATH",
        "reset",
        "rm [-f] [--cached]",
        "mv SRC DST",
    ] {
        assert!(help.contains(want), "help missing {want}");
    }
}

#[test]
fn chronos_1_item7_switch_keeps_non_conflicting_local_changes() {
    let tmp = TempDir::new("item7");
    let r = tmp.path();
    init_with_commit(r, &[("f1.txt", "a\n"), ("f2.txt", "b\n")]);
    ok(r, &["branch", "topic"]);
    ok(r, &["switch", "topic"]);
    write(r, "f2.txt", "topic2\n");
    ok(r, &["add", "f2.txt"]);
    ok(r, &["commit", "-m", "topic edit"]);
    ok(r, &["switch", "main"]);
    assert_eq!(fs::read_to_string(r.join("f2.txt")).unwrap(), "b\n");

    // Local edit to f1, which topic shares with main: the switch is allowed and keeps it.
    write(r, "f1.txt", "dirty\n");
    ok(r, &["switch", "topic"]);
    assert_eq!(fs::read_to_string(r.join("f1.txt")).unwrap(), "dirty\n");
    assert_eq!(fs::read_to_string(r.join("f2.txt")).unwrap(), "topic2\n");
    assert!(text(&ok(r, &["status"])).starts_with("on branch topic"));

    // Local edit to f2 on topic, which main changes: refused, and the path is named.
    write(r, "f2.txt", "x\n");
    let refused = chronos(r, &["switch", "main"]);
    assert_eq!(refused.status.code(), Some(1));
    let msg = err(&refused);
    assert!(
        msg.contains("f2.txt") && msg.contains("would be overwritten"),
        "{msg}"
    );
    assert!(!msg.contains("f1.txt"), "{msg}");
    assert!(text(&ok(r, &["status"])).starts_with("on branch topic"));
    assert_eq!(fs::read_to_string(r.join("f2.txt")).unwrap(), "x\n");
}

#[test]
fn chronos_1_item8_commit_without_message_fails_without_editor() {
    let tmp = TempDir::new("item8");
    let r = tmp.path();
    init_with_commit(r, &[("a.txt", "a\n")]);
    write(r, "a.txt", "b\n");
    ok(r, &["add", "a.txt"]);

    // No EDITOR/VISUAL and no terminal: fail at once, do not spawn calliope.
    let out = chronos(r, &["commit"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        err(&out).contains("no commit message: use -m MSG or set EDITOR"),
        "{}",
        err(&out)
    );
    assert!(!r.join(".chronos").join("COMMIT_EDITMSG").exists());
    assert!(section(&status_lines(r), "Changes to be committed:").contains(&"M a.txt".to_string()));
}
