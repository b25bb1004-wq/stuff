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

#[test]
fn edit_inserts_newlines_in_the_file_style_on_stdout_and_save() {
    let dir = TempDir::new("eol");
    let script = dir.path().join("s.txt");
    // The script's `insert X\n` takes an escaped backslash-n, which the edit turns into a newline.
    fs::write(&script, "goto 1 1\ninsert X\\n\nsave\n").unwrap();
    for (label, original, expected) in [
        ("crlf", "one\r\ntwo\r\n", "X\r\none\r\ntwo\r\n"),
        ("lf", "one\ntwo\n", "X\none\ntwo\n"),
    ] {
        let file = dir.path().join(format!("{label}.txt"));
        fs::write(&file, original).unwrap();
        let out = Command::new(bin())
            .arg("edit")
            .arg(&script)
            .arg(&file)
            .output()
            .unwrap();
        assert!(out.status.success(), "{:?}", out);
        assert_eq!(String::from_utf8(out.stdout).unwrap(), expected, "{label}");
        assert_eq!(fs::read_to_string(&file).unwrap(), expected, "{label} save");
    }
}

#[test]
fn edit_errors_name_the_script_line() {
    let dir = TempDir::new("errline");
    let script = dir.path().join("s.txt");
    let file = dir.path().join("f.txt");
    fs::write(&file, "one\n").unwrap();
    fs::write(&script, "goto 1 1\ngoto 99 1\n").unwrap();
    let out = Command::new(bin())
        .arg("edit")
        .arg(&script)
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "calliope: script line 2: line 99 out of range\n"
    );
}

#[test]
fn a_file_name_without_a_terminal_says_the_editor_needs_one() {
    let out = Command::new(bin()).arg("notes.txt").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "calliope: the editor needs a terminal (stdin is not a terminal)\n"
    );
    let out = Command::new(bin()).arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("unknown subcommand 'frobnicate'"));
}

#[test]
fn grep_into_a_closed_pipe_exits_quietly() {
    use std::io::Read;
    use std::process::Stdio;
    let dir = TempDir::new("pipe");
    let file = dir.path().join("many.txt");
    fs::write(&file, "match line\n".repeat(50_000)).unwrap();
    let mut child = Command::new(bin())
        .arg("grep")
        .arg("match")
        .arg(&file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read a little, then close the pipe the way `head -1` does.
    let mut first = [0u8; 16];
    child.stdout.take().unwrap().read_exact(&mut first).unwrap();
    let status = child.wait().unwrap();
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert_eq!(status.code(), Some(0));
    assert_eq!(err, "");
}
