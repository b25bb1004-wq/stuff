//! CLI integration tests for the `iris` binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use pigment::decode_png;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_iris")
}

fn temp_path(name: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("iris_cli_test_{pid}_{n}_{name}"))
}

#[test]
fn help_prints_usage_and_exits_zero() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert!(out.status.success(), "status={:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("USAGE:"), "stdout={stdout}");
    assert!(stdout.contains("replay"), "stdout={stdout}");
    assert!(stdout.contains("EXAMPLE:"), "stdout={stdout}");
}

#[test]
fn version_prints_iris_version_and_exits_zero() {
    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert!(out.status.success(), "status={:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "iris 0.1.0");
}

#[test]
fn replay_good_events_writes_png_and_prints_windows_and_damage() {
    let events_path = temp_path("good.events");
    let png_path = temp_path("frame.png");
    let script = "\
# Open two windows, move, type, focus, tile, and close one
open terminal 260 160
text echo hello
key enter
open editor /readme.txt 240 150
text hi
move 2 100 80
focus 1
tile
open files / 200 140
close 3
";
    fs::write(&events_path, script).unwrap();

    let out = Command::new(bin())
        .arg("replay")
        .arg(&events_path)
        .arg("--out")
        .arg(&png_path)
        .arg("--width")
        .arg("640")
        .arg("--height")
        .arg("400")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "status={:?}, stderr={stderr}",
        out.status
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("screen 640x400\n"), "stdout={stdout}");
    assert!(stdout.contains("layout tiling\n"), "stdout={stdout}");
    assert!(stdout.contains("windows 2\n"), "stdout={stdout}");
    assert!(stdout.contains("window 1 "), "stdout={stdout}");
    assert!(stdout.contains("window 2 "), "stdout={stdout}");
    assert!(stdout.contains("damage_rects "), "stdout={stdout}");
    assert!(stdout.contains("damage_pixels "), "stdout={stdout}");

    let png_bytes = fs::read(&png_path).unwrap();
    let canvas = decode_png(&png_bytes, 640 * 400).unwrap();
    assert_eq!(canvas.width, 640);
    assert_eq!(canvas.height, 400);

    let _ = fs::remove_file(&events_path);
    let _ = fs::remove_file(&png_path);
}

#[test]
fn replay_stdin_and_move_in_floating_mode() {
    let png_path = temp_path("stdin.png");
    let mut child = Command::new(bin())
        .arg("replay")
        .arg("-")
        .arg("--out")
        .arg(&png_path)
        .arg("--width=480")
        .arg("--height=320")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin
            .write_all(b"open terminal 200 120\nmove 1 40 50\nkey super+enter\n")
            .unwrap();
    }

    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr={stderr}");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("windows 2\n"), "stdout={stdout}");
    assert!(stdout.contains("window 1 x=40 y=50 "), "stdout={stdout}");

    let png_bytes = fs::read(&png_path).unwrap();
    let canvas = decode_png(&png_bytes, 480 * 320).unwrap();
    assert_eq!(canvas.width, 480);
    assert_eq!(canvas.height, 320);

    let _ = fs::remove_file(&png_path);
}

#[test]
fn replay_bad_input_exits_one_with_iris_prefix() {
    let events_path = temp_path("bad.events");
    let png_path = temp_path("bad.png");
    fs::write(&events_path, "open terminal\nbogus_command 123\n").unwrap();

    let out = Command::new(bin())
        .arg("replay")
        .arg(&events_path)
        .arg("--out")
        .arg(&png_path)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("iris: "), "stderr={stderr}");
    assert!(!png_path.exists());

    let _ = fs::remove_file(&events_path);
}

#[test]
fn replay_missing_file_exits_one_with_iris_prefix() {
    let missing = temp_path("does_not_exist.events");
    let png_path = temp_path("missing.png");

    let out = Command::new(bin())
        .arg("replay")
        .arg(&missing)
        .arg("--out")
        .arg(&png_path)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("iris: "), "stderr={stderr}");
}

#[test]
fn unknown_subcommand_exits_two() {
    let out = Command::new(bin()).arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("iris: "), "stderr={stderr}");
}

#[test]
fn missing_out_flag_exits_two() {
    let events_path = temp_path("noout.events");
    fs::write(&events_path, "open terminal\n").unwrap();

    let out = Command::new(bin())
        .arg("replay")
        .arg(&events_path)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("iris: "), "stderr={stderr}");

    let _ = fs::remove_file(&events_path);
}
