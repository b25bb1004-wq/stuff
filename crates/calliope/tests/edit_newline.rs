//! `calliope edit` script: the `newline` command, the documented grammar, and error lines.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
            "calliope_newline_{label}_{}_{nanos}_{seq}",
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

/// Run `calliope edit SCRIPT FILE` where FILE starts as `original` and SCRIPT is `script`.
fn edit(label: &str, original: &str, script: &str) -> (Output, PathBuf) {
    let tmp = TempDir::new(label);
    let file = tmp.path().join("doc.txt");
    let script_path = tmp.path().join("ops.txt");
    fs::write(&file, original).unwrap();
    fs::write(&script_path, script).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_calliope"))
        .arg("edit")
        .arg(&script_path)
        .arg(&file)
        .output()
        .unwrap();
    // Keep the file on disk for the caller by moving it out of the temp dir's reach.
    let kept = std::env::temp_dir().join(format!(
        "calliope_newline_{label}_{}.out",
        std::process::id()
    ));
    let _ = fs::copy(&file, &kept);
    (out, kept)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn replace_line_2_with_new_text() {
    let (out, kept) = edit(
        "replace",
        "one\ntwo\nthree\n",
        "goto 2 1\ndelete 3\ninsert two changed\nsave\n",
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(&out), "one\ntwo changed\nthree\n");
    assert_eq!(
        fs::read_to_string(&kept).unwrap(),
        "one\ntwo changed\nthree\n"
    );
    let _ = fs::remove_file(kept);
}

#[test]
fn newline_inserts_two_lines_and_leaves_the_cursor_at_the_new_line() {
    let (out, kept) = edit(
        "two_lines",
        "x\n",
        "goto 1 1\ninsert A\nnewline\ninsert B\nnewline\nsave\n",
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(&out), "A\nB\nx\n");
    assert_eq!(fs::read_to_string(&kept).unwrap(), "A\nB\nx\n");
    let _ = fs::remove_file(kept);
}

#[test]
fn newline_in_a_crlf_file_inserts_crlf() {
    let (out, kept) = edit(
        "crlf",
        "one\r\ntwo\r\n",
        "goto 2 1\nnewline\ninsert Y\nsave\n",
    );
    assert!(out.status.success(), "{out:?}");
    let want = "one\r\n\r\nYtwo\r\n";
    assert_eq!(stdout(&out), want);
    assert_eq!(fs::read_to_string(&kept).unwrap(), want);
    let _ = fs::remove_file(kept);
}

#[test]
fn newline_in_an_lf_file_inserts_lf() {
    let (out, kept) = edit("lf", "one\ntwo\n", "goto 2 1\nnewline\nsave\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(&out), "one\n\ntwo\n");
    let _ = fs::remove_file(kept);
}

#[test]
fn delete_counts_a_line_break_as_one_character() {
    let (out, kept) = edit("delete_nl", "one\ntwo\n", "goto 1 4\ndelete 1\nsave\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(&out), "onetwo\n");
    let _ = fs::remove_file(kept);
}

#[test]
fn without_save_nothing_is_written() {
    let tmp = TempDir::new("nosave");
    let file = tmp.path().join("doc.txt");
    let script = tmp.path().join("ops.txt");
    fs::write(&file, "keep\n").unwrap();
    fs::write(&script, "goto 1 1\ninsert new\nnewline\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_calliope"))
        .arg("edit")
        .arg(&script)
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(&out), "new\nkeep\n");
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep\n");
}

#[test]
fn unknown_command_still_errors_with_its_script_line() {
    let (out, kept) = edit("unknown", "one\n", "goto 1 1\nfrobnicate\n");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "calliope: script line 2: unknown command 'frobnicate'\n"
    );
    let _ = fs::remove_file(kept);
}

#[test]
fn help_teaches_the_newline_command() {
    let out = Command::new(env!("CARGO_BIN_EXE_calliope"))
        .arg("--help")
        .output()
        .unwrap();
    let help = stdout(&out);
    assert!(help.contains("newline"), "{help}");
    assert!(help.contains("cursor add LINE COL"), "{help}");
    assert!(help.contains("save"), "{help}");
}
