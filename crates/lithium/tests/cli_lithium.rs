//! CLI integration tests for the `lithium` binary.
#![allow(clippy::unwrap_used)]
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_lithium")
}

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "lithium_cli_{}_{}_{name}",
        std::process::id(),
        nanos
    ))
}

#[test]
fn help_and_version() {
    let help = Command::new(bin()).arg("--help").output().unwrap();
    assert!(
        help.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&help.stderr)
    );
    let help_out = String::from_utf8_lossy(&help.stdout);
    assert!(help_out.contains("run"), "help missing run: {help_out}");
    assert!(help_out.contains("image"), "help missing image: {help_out}");
    assert!(
        help_out.contains("Examples:"),
        "help missing examples: {help_out}"
    );

    let ver = Command::new(bin()).arg("--version").output().unwrap();
    assert!(ver.status.success());
    assert_eq!(String::from_utf8_lossy(&ver.stdout).trim(), "lithium 0.1.0");
}

#[test]
fn run_good_and_bad() {
    let ok = Command::new(bin()).arg("run").output().unwrap();
    let stdout = String::from_utf8_lossy(&ok.stdout);
    let stderr = String::from_utf8_lossy(&ok.stderr);
    assert!(
        ok.status.success(),
        "run failed (code {:?}):\nstdout:\n{stdout}\nstderr:\n{stderr}",
        ok.status.code()
    );
    assert!(
        stdout.contains("exit: pass"),
        "expected exit: pass, got:\n{stdout}"
    );
    assert!(stdout.contains("steps:"), "missing steps line: {stdout}");

    let bounded = Command::new(bin())
        .args(["run", "--steps", "100"])
        .output()
        .unwrap();
    assert!(bounded.status.success());
    let bounded_out = String::from_utf8_lossy(&bounded.stdout);
    assert!(
        bounded_out.contains("exit: out_of_steps"),
        "expected out_of_steps: {bounded_out}"
    );
    assert!(
        bounded_out.contains("steps: 100"),
        "expected steps: 100: {bounded_out}"
    );

    let bounded_eq = Command::new(bin())
        .args(["run", "--steps=50"])
        .output()
        .unwrap();
    assert!(bounded_eq.status.success());
    let bounded_eq_out = String::from_utf8_lossy(&bounded_eq.stdout);
    assert!(bounded_eq_out.contains("steps: 50"));

    for bad_val in ["notanumber", "0"] {
        let bad = Command::new(bin())
            .args(["run", "--steps", bad_val])
            .output()
            .unwrap();
        assert_eq!(bad.status.code(), Some(1));
        let bad_err = String::from_utf8_lossy(&bad.stderr);
        assert!(
            bad_err.starts_with("lithium:"),
            "expected lithium: error, got: {bad_err}"
        );
    }
}

#[test]
fn image_good_and_bad() {
    let expected = lithium::image().unwrap();

    let out_file = temp_path("kernel.bin");
    let ok = Command::new(bin())
        .args(["image", "-o"])
        .arg(&out_file)
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "image failed: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let ok_out = String::from_utf8_lossy(&ok.stdout);
    assert!(ok_out.contains("bytes:"), "missing bytes line: {ok_out}");
    let written = fs::read(&out_file).unwrap();
    assert_eq!(written, expected);
    let _ = fs::remove_file(&out_file);

    let to_stdout = Command::new(bin())
        .args(["image", "-o", "-"])
        .output()
        .unwrap();
    assert!(to_stdout.status.success());
    assert_eq!(to_stdout.stdout, expected);

    let bad_path = temp_path("missing_dir").join("out.bin");
    let bad = Command::new(bin())
        .args(["image", "-o"])
        .arg(&bad_path)
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let bad_err = String::from_utf8_lossy(&bad.stderr);
    assert!(
        bad_err.starts_with("lithium:"),
        "expected lithium: error, got: {bad_err}"
    );
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        vec!["frobnicate"],
        vec![],
        vec!["image"],
        vec!["run", "--bogus"],
        vec!["run", "--steps"],
        vec!["image", "-o"],
    ] {
        let out = Command::new(bin()).args(&args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "expected exit 2 for args {args:?}, got {:?}",
            out.status.code()
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.starts_with("lithium:"), "got: {err}");
    }
}
