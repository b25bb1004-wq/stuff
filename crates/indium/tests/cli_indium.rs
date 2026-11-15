//! CLI integration tests for the `indium` binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use pigment::decode_png;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("indium_cli_{prefix}_{pid}_{id}"));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_indium")
}

#[test]
fn help_flag_lists_render_and_layout_format() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("render"));
    assert!(stdout.contains("column"));
    assert!(stdout.contains("row"));
    assert!(stdout.contains("text"));
    assert!(stdout.contains("button"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("--focus"));
}

#[test]
fn version_flag_prints_name_and_version() {
    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.trim(), "indium 0.1.0");
}

#[test]
fn unknown_subcommand_exits_2() {
    let out = Command::new(bin()).arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("indium:"), "stderr was: {stderr}");
}

#[test]
fn render_good_layout_writes_png_and_prints_focus_order() {
    let dir = TempDir::new("good");
    let layout = dir.file("ui.layout");
    let png_a = dir.file("out_a.png");
    let png_b = dir.file("out_b.png");

    fs::write(
        &layout,
        "\
# Sample UI
column
  text \"Settings\"
  row
    button \"Save\"
    button \"Cancel\"
  list 2
",
    )
    .unwrap();

    let out_a = Command::new(bin())
        .args([
            "render",
            layout.to_str().unwrap(),
            "--out",
            png_a.to_str().unwrap(),
            "--width",
            "320",
            "--height",
            "240",
            "--focus",
            "0",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out_a.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out_a.stderr)
    );

    let stdout = String::from_utf8(out_a.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 5, "stdout was:\n{stdout}");
    assert!(lines[0].starts_with("0: button \"Save\""));
    assert!(lines[1].starts_with("1: button \"Cancel\""));
    assert!(lines[2].starts_with("2: list-item \"item 1\""));
    assert!(lines[3].starts_with("3: list-item \"item 2\""));
    assert!(lines[4].starts_with("focused: 0"));

    let bytes_a = fs::read(&png_a).unwrap();
    assert!(bytes_a.starts_with(b"\x89PNG\r\n\x1a\n"));
    let decoded_a = decode_png(&bytes_a, 1_000_000).unwrap();
    assert_eq!(decoded_a.width, 320);
    assert_eq!(decoded_a.height, 240);

    // Focusing a different widget changes the rendered focus ring pixels.
    let out_b = Command::new(bin())
        .args([
            "render",
            layout.to_str().unwrap(),
            "--out",
            png_b.to_str().unwrap(),
            "--width",
            "320",
            "--height",
            "240",
            "--focus",
            "2",
        ])
        .output()
        .unwrap();
    assert_eq!(out_b.status.code(), Some(0));
    let bytes_b = fs::read(&png_b).unwrap();
    let decoded_b = decode_png(&bytes_b, 1_000_000).unwrap();
    assert_ne!(decoded_a.pixels, decoded_b.pixels);
}

#[test]
fn render_stdin_layout_succeeds() {
    let dir = TempDir::new("stdin");
    let png = dir.file("stdin.png");

    let mut child = Command::new(bin())
        .args(["render", "-", "--out", png.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"column\n  button \"Go\"\n").unwrap();
    }

    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("0: button \"Go\""));
    let bytes = fs::read(&png).unwrap();
    assert!(decode_png(&bytes, 1_000_000).is_ok());
}

#[test]
fn render_bad_layout_exits_1_with_indium_prefix() {
    let dir = TempDir::new("bad_syntax");
    let layout = dir.file("bad.layout");
    let png = dir.file("bad.png");
    fs::write(&layout, "column\n  slider 50\n").unwrap();

    let out = Command::new(bin())
        .args([
            "render",
            layout.to_str().unwrap(),
            "--out",
            png.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("indium:"), "stderr was: {stderr}");
}

#[test]
fn render_missing_file_exits_1() {
    let dir = TempDir::new("missing");
    let layout = dir.file("does_not_exist.layout");
    let png = dir.file("out.png");

    let out = Command::new(bin())
        .args([
            "render",
            layout.to_str().unwrap(),
            "--out",
            png.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("indium:"), "stderr was: {stderr}");
}

#[test]
fn render_focus_out_of_range_exits_1() {
    let dir = TempDir::new("focus_oob");
    let layout = dir.file("ui.layout");
    let png = dir.file("out.png");
    fs::write(&layout, "column\n  button \"Only\"\n").unwrap();

    let out = Command::new(bin())
        .args([
            "render",
            layout.to_str().unwrap(),
            "--out",
            png.to_str().unwrap(),
            "--focus",
            "5",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("indium:"), "stderr was: {stderr}");
}
