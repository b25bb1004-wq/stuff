//! Integration tests for the `odyssey` command-line tool.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "odyssey_cli_test_{}_{}_{name}",
            std::process::id(),
            id
        ));
        fs::write(&path, contents).unwrap();
        Self { path }
    }

    fn path_only(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "odyssey_cli_test_{}_{}_{name}",
            std::process::id(),
            id
        ));
        Self { path }
    }

    fn str(&self) -> &str {
        self.path.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_odyssey")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn run_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn help_and_version() {
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&help.stdout);
    for sub in ["tokens", "tree", "select", "render"] {
        assert!(stdout.contains(sub), "help missing subcommand {sub}");
    }

    let ver = run(&["--version"]);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ver.stdout).trim(), "odyssey 0.1.0");
}

#[test]
fn tokens_good_and_stdin() {
    let html = TempFile::new(
        "tok.html",
        "<!doctype html><p class=\"a\">hi\nthere</p><!-- c -->",
    );
    let out = run(&["tokens", html.str()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "DOCTYPE html",
            "START p class=\"a\"",
            "TEXT \"hi\\nthere\"",
            "END p",
            "COMMENT \" c \"",
        ]
    );

    let stdin_out = run_stdin(&["tokens", "-"], "<br/>ok");
    assert_eq!(stdin_out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&stdin_out.stdout)
            .lines()
            .collect::<Vec<_>>(),
        vec!["START br /", "TEXT \"ok\""]
    );
}

#[test]
fn tree_good() {
    let html = TempFile::new(
        "tree.html",
        "<!doctype html><html><body><p id=\"x\">hello</p></body></html>",
    );
    let out = run(&["tree", html.str()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("| <!DOCTYPE html>"));
    assert!(stdout.contains("|   <body>"));
    assert!(stdout.contains("|     <p>"));
    assert!(stdout.contains("|       id=\"x\""));
    assert!(stdout.contains("|       \"hello\""));
}

#[test]
fn select_good() {
    let html = TempFile::new(
        "sel.html",
        "<html><body><div><p class=\"lead\">one</p><p id=\"two\">two</p></div></body></html>",
    );
    let out = run(&["select", "div > p", html.str()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "/html/body/div/p[1] <p class=\"lead\">",
            "/html/body/div/p[2] <p id=\"two\">",
        ]
    );
}

#[test]
fn render_good() {
    let html = TempFile::new(
        "render.html",
        "<html><body style=\"background-color: #fafafa; color: #111; padding: 8px\"><h1>Title</h1><p>Body</p></body></html>",
    );
    let png_out = TempFile::path_only("page.png");
    let out = run(&[
        "render",
        html.str(),
        "--width",
        "320",
        "--out",
        png_out.str(),
    ]);
    assert_eq!(out.status.code(), Some(0));
    let bytes = fs::read(&png_out.path).unwrap();
    let decoded = pigment::decode_png(&bytes, 1_000_000).unwrap();
    assert_eq!(decoded.width, 320);
    assert!(decoded.height > 0);

    // Default width (800) and stdin/stdout (`-`).
    let stdout_png = run_stdin(&["render", "-", "--out", "-"], "<p>hello</p>");
    assert_eq!(stdout_png.status.code(), Some(0));
    let decoded_default = pigment::decode_png(&stdout_png.stdout, 1_000_000).unwrap();
    assert_eq!(decoded_default.width, 800);
    assert!(decoded_default.height > 0);
}

#[test]
fn bad_inputs_exit_1() {
    let missing = TempFile::path_only("does_not_exist.html");
    let html = TempFile::new("good.html", "<p>ok</p>");

    for args in [
        vec!["tokens", missing.str()],
        vec!["tree", missing.str()],
        vec!["select", "p", missing.str()],
        vec!["select", "div[", html.str()],
        vec!["render", missing.str(), "--out", "out.png"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "expected exit 1 for {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.starts_with("odyssey:"),
            "expected odyssey: prefix in stderr for {args:?}, got {stderr:?}"
        );
    }

    let bad_dir = TempFile::path_only("no_such_dir");
    let bad_png = bad_dir.path.join("out.png");
    let out = run(&["render", html.str(), "--out", bad_png.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("odyssey:"));
}

#[test]
fn usage_errors_exit_2() {
    let html = TempFile::new("usage.html", "<p>ok</p>");
    for args in [
        vec![],
        vec!["bogus"],
        vec!["tokens"],
        vec!["tree"],
        vec!["select", "p"],
        vec!["render", html.str()],
        vec!["render", html.str(), "--width", "0", "--out", "a.png"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "expected exit 2 for {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.starts_with("odyssey:"),
            "expected odyssey: prefix in stderr for {args:?}, got {stderr:?}"
        );
    }
}
