#![allow(missing_docs)]
// CLI tests for the krypton binary's own argument handling.

use std::process::{Command, Output};

fn krypton(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_krypton"))
        .args(args)
        .output()
        .expect("failed to execute krypton")
}

#[test]
fn help_exits_0_and_prints_usage() {
    for flag in ["--help", "-h"] {
        let output = krypton(&[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"), "{flag}: {stdout}");
    }
}

#[test]
fn version_prints_name_and_version() {
    let output = krypton(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "krypton 0.1.0");
}

#[test]
fn unknown_option_exits_2() {
    // krypton has no subcommands; an unknown option is the closest usage error.
    let output = krypton(&["--bogus"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("krypton: unknown option: --bogus"),
        "{stderr}"
    );
}

#[test]
fn missing_option_argument_exits_2() {
    let output = krypton(&["-c"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("krypton: -c:"), "{stderr}");
}

#[test]
fn missing_script_file_exits_1_with_prefix() {
    let output = krypton(&["definitely-missing-krypton-script.sh"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("krypton: definitely-missing-krypton-script.sh:"),
        "{stderr}"
    );
}
