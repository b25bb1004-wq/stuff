#![allow(missing_docs)]
// Real programs as children. The krypton binary is itself a program on both Windows and Linux:
// `-c SCRIPT` runs a script, and `-` reads a script from stdin.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("krypton-d1-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Run `script` with the krypton binary bound to `$K`, in `dir`. Returns (stdout, status).
fn run(script: &str, dir: &Path) -> (String, i32) {
    let k = env!("CARGO_BIN_EXE_krypton");
    let full = format!("K='{k}'; {script}");
    let out = Command::new(k)
        .args(["-c", &full])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    (stdout, out.status.code().unwrap_or(-1))
}

#[test]
fn pipeline_moves_data_between_real_programs() {
    let dir = temp_dir("pipe");
    let script = r#""$K" -c "echo 'echo deep'" | "$K" - | "$K" -c 'read x; echo got:$x'"#;
    let (out, code) = run(script, &dir);
    assert_eq!((out.as_str(), code), ("got:deep\n", 0));
}

#[test]
fn pipeline_status_is_the_last_stage() {
    let dir = temp_dir("status");
    let (out, code) = run(r#""$K" -c 'exit 3' | "$K" -c 'exit 5'; echo $?"#, &dir);
    assert_eq!((out.as_str(), code), ("5\n", 0));
}

#[test]
fn redirections_write_append_read_and_stderr() {
    let dir = temp_dir("redir");
    fs::write(dir.join("in.txt"), "from-file\n").expect("write input");
    let script = concat!(
        r#""$K" -c 'echo a' > out.txt; "$K" -c 'echo b' >> out.txt; "#,
        r#""$K" -c 'echo oops >&2' 2> err.txt; "#,
        r#""$K" -c 'read line; echo $line' < in.txt"#,
    );
    let (out, code) = run(script, &dir);
    let written = fs::read_to_string(dir.join("out.txt")).expect("out.txt");
    let err = fs::read_to_string(dir.join("err.txt")).expect("err.txt");
    assert_eq!(out, "from-file\n");
    assert_eq!(code, 0);
    assert_eq!(written.replace("\r\n", "\n"), "a\nb\n");
    assert_eq!(err.replace("\r\n", "\n"), "oops\n");
}

#[test]
fn exported_variables_reach_child_programs_and_braces_work() {
    let dir = temp_dir("env");
    let (out, _) = run(r#"export X=7; "$K" -c 'echo inner:$X'; echo ${X}"#, &dir);
    assert_eq!(out, "inner:7\n7\n");
}

#[test]
fn unknown_command_gives_127() {
    let dir = temp_dir("missing");
    let (out, _) = run("nosuchprogram_d1_xyz; echo $?", &dir);
    assert_eq!(out, "127\n");
}

#[test]
fn cd_tilde_uses_home() {
    let dir = temp_dir("home");
    let k = env!("CARGO_BIN_EXE_krypton");
    let out = Command::new(k)
        .args(["-c", "cd ~; pwd"])
        .current_dir(&dir)
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    let shown = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(Path::new(&shown), dir.as_path());
}

#[cfg(windows)]
#[test]
fn batch_files_run_through_cmd_with_safe_quoting() {
    let dir = temp_dir("batch");
    fs::write(
        dir.join("show.cmd"),
        "@echo off\r\necho BATOK %1 %2\r\nexit /b 4\r\n",
    )
    .expect("write batch");
    let (out, code) = run(r#"./show.cmd one 'two three'; echo rc=$?"#, &dir);
    assert_eq!(out, "BATOK one \"two three\"\nrc=4\n");
    assert_eq!(code, 0);
    let (out, _) = run(r#"./show.cmd 'bad%arg'; echo rc=$?"#, &dir);
    assert!(out.starts_with("rc=126") || out.contains("rc=126"), "{out}");
}
