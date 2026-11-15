//! CLI tests for the `zeusctl` host binary.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run_zeusctl(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zeusctl"))
        .current_dir(workspace_root())
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_lists_run_and_example() {
    let out = run_zeusctl(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("run"), "help missing run:\n{stdout}");
    assert!(
        stdout.contains("zeusctl run"),
        "help missing example:\n{stdout}"
    );
}

#[test]
fn version_prints_expected_string() {
    let out = run_zeusctl(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.trim(), "zeusctl 0.1.0");
}

#[test]
fn missing_zeus_file_exits_1_with_build_command() {
    let missing = std::env::temp_dir().join("zeusctl_nonexistent_zeus_elf");
    let _ = std::fs::remove_file(&missing);
    let missing_str = missing.to_str().unwrap();

    let out = run_zeusctl(&["run", "--zeus", missing_str]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("zeusctl: "), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "cargo build -p zeus --bin zeus --release --target riscv64gc-unknown-none-elf"
        ),
        "stderr missing build command:\n{stderr}"
    );
}

#[test]
fn missing_titan_file_exits_1_with_build_command() {
    let dummy_zeus = std::env::temp_dir().join("zeusctl_dummy_zeus_elf_for_test");
    std::fs::write(&dummy_zeus, b"not-an-elf").unwrap();
    let missing_titan = std::env::temp_dir().join("zeusctl_nonexistent_titan_kernel");
    let _ = std::fs::remove_file(&missing_titan);

    let out = run_zeusctl(&[
        "run",
        "--zeus",
        dummy_zeus.to_str().unwrap(),
        "--guest",
        "titan",
        "--titan",
        missing_titan.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(&dummy_zeus);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("zeusctl: "), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "cargo build -p titan --bin kernel --release --target riscv64gc-unknown-none-elf"
        ),
        "stderr missing titan build command:\n{stderr}"
    );
}

#[test]
fn bad_guest_specs_exit_2() {
    for bad in [
        "unknown",
        "lithium:",
        "lithium:0",
        "lithium:1",
        "lithium:3",
        "lithium:18",
        "titan:abc",
    ] {
        let out = run_zeusctl(&["run", "--guest", bad]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "expected exit 2 for guest {bad:?}, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.starts_with("zeusctl: "), "stderr:\n{stderr}");
    }

    let too_many = run_zeusctl(&[
        "run", "--guest", "lithium", "--guest", "lithium", "--guest", "lithium", "--guest",
        "lithium",
    ]);
    assert_eq!(too_many.status.code(), Some(2));
}

#[test]
fn unknown_subcommand_and_bad_options_exit_2() {
    for args in [
        &[][..],
        &["bogus"][..],
        &["--bogus"][..],
        &["run", "--bogus"][..],
        &["run", "--steps", "0"][..],
        &["run", "--steps", "abc"][..],
        &["run", "--zeus"][..],
        &["run", "extra"][..],
    ] {
        let out = run_zeusctl(args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "expected exit 2 for {args:?}, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.starts_with("zeusctl: "), "stderr:\n{stderr}");
    }
}

#[test]
fn run_default_lithium_guest_when_zeus_elf_exists() {
    let zeus_path = workspace_root()
        .join("target")
        .join("zeus-bin")
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("zeus");
    if !zeus_path.exists() {
        eprintln!(
            "skipping zeusctl lithium run test: missing {}",
            zeus_path.display()
        );
        return;
    }

    let out = run_zeusctl(&["run"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "zeusctl failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("zeus: lithium exited 0"),
        "missing lithium exit line:\n{stdout}"
    );
    assert!(
        stdout.contains("[lithium] lithium: boot"),
        "missing lithium boot line:\n{stdout}"
    );
    assert!(
        stdout.contains("[lithium] lithium: halt"),
        "missing lithium halt line:\n{stdout}"
    );
    assert!(
        stdout.contains("exit: Pass"),
        "missing exit line:\n{stdout}"
    );
    assert!(stdout.contains("steps: "), "missing steps line:\n{stdout}");
}
