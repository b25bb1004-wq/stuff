//! CLI integration tests for the `titan` binary.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn titan_bin() -> &'static str {
    env!("CARGO_BIN_EXE_titan")
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("neon_titan_{tag}_{}_{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn default_kernel_path() -> PathBuf {
    workspace_root()
        .join("target")
        .join("titan-kernel")
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel")
}

fn run_titan(args: &[&str], stdin_bytes: Option<&[u8]>, cwd: Option<&Path>) -> Output {
    let mut cmd = Command::new(titan_bin());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(bytes) = stdin_bytes {
        child.stdin.as_mut().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn help_prints_usage_and_example() {
    let out = run_titan(&["--help"], None, None);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("boot"), "stdout: {stdout}");
    assert!(stdout.contains("titan boot"), "stdout: {stdout}");

    let sub_help = run_titan(&["boot", "--help"], None, None);
    assert_eq!(sub_help.status.code(), Some(0));
    assert_eq!(sub_help.stdout, out.stdout);
}

#[test]
fn version_prints_expected_string() {
    let out = run_titan(&["--version"], None, None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "titan 0.1.0");
}

#[test]
fn unknown_subcommand_and_bad_flags_exit_two() {
    for args in [
        &["bogus"][..],
        &[][..],
        &["boot", "--disk"][..],
        &["boot", "--kernel"][..],
        &["boot", "--script"][..],
        &["boot", "--unknown"][..],
    ] {
        let out = run_titan(args, None, None);
        assert_eq!(out.status.code(), Some(2), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.starts_with("titan:"),
            "args: {args:?}, stderr: {stderr}"
        );
    }
}

#[test]
fn boot_bad_inputs_exit_one_with_prefix() {
    let dir = temp_dir("bad");
    let missing_kernel = dir.join("missing.elf");
    let out = run_titan(
        &["boot", "--kernel", missing_kernel.to_str().unwrap()],
        None,
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("titan:"), "stderr: {stderr}");
    assert!(stderr.contains("cargo build"), "stderr: {stderr}");

    let bad_elf = dir.join("bad.elf");
    fs::write(&bad_elf, b"not an elf").unwrap();
    let out = run_titan(&["boot", "--kernel", bad_elf.to_str().unwrap()], None, None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("titan:"), "stderr: {stderr}");

    let missing_script = dir.join("missing.txt");
    let out = run_titan(
        &[
            "boot",
            "--kernel",
            bad_elf.to_str().unwrap(),
            "--script",
            missing_script.to_str().unwrap(),
        ],
        None,
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("titan:"), "stderr: {stderr}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn boot_script_default_kernel_and_interactive_work() {
    let kernel = default_kernel_path();
    assert!(
        kernel.exists(),
        "expected built titan kernel at {}",
        kernel.display()
    );

    let dir = temp_dir("good");
    let script = dir.join("script.txt");
    fs::write(&script, "echo titan-ok\nls\n").unwrap();

    // Default --kernel path from workspace root with --script FILE.
    let root = workspace_root();
    let out = run_titan(
        &["boot", "--script", script.to_str().unwrap()],
        None,
        Some(&root),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("titan-ok"), "stdout: {stdout}");
    assert!(stdout.contains("hello.txt"), "stdout: {stdout}");

    // Script from stdin (`--script -`).
    let script_stdin = run_titan(
        &[
            "boot",
            "--kernel",
            kernel.to_str().unwrap(),
            "--script",
            "-",
        ],
        Some(b"echo dash-ok\n"),
        None,
    );
    assert_eq!(
        script_stdin.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&script_stdin.stderr)
    );
    let stdout = String::from_utf8_lossy(&script_stdin.stdout);
    assert!(stdout.contains("dash-ok"), "stdout: {stdout}");

    // Interactive stdin -> stdout.
    let interactive = run_titan(
        &["boot", "--kernel", kernel.to_str().unwrap()],
        Some(b"echo interactive-ok\n"),
        None,
    );
    assert_eq!(
        interactive.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&interactive.stderr)
    );
    let stdout = String::from_utf8_lossy(&interactive.stdout);
    assert!(stdout.contains("interactive-ok"), "stdout: {stdout}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn boot_disk_persists_across_runs_and_rejects_bad_image() {
    let kernel = default_kernel_path();
    if !kernel.exists() {
        eprintln!(
            "skipping kernel CLI disk test (missing {})",
            kernel.display()
        );
        return;
    }

    let dir = temp_dir("disk_cli");
    let bad_disk = dir.join("bad.img");
    fs::write(&bad_disk, b"not a valid disk").unwrap();
    let bad_out = run_titan(
        &[
            "boot",
            "--kernel",
            kernel.to_str().unwrap(),
            "--disk",
            bad_disk.to_str().unwrap(),
        ],
        None,
        None,
    );
    assert_eq!(bad_out.status.code(), Some(1));
    let bad_stderr = String::from_utf8_lossy(&bad_out.stderr);
    assert!(bad_stderr.starts_with("titan:"), "stderr: {bad_stderr}");
    assert_eq!(fs::read(&bad_disk).unwrap(), b"not a valid disk");

    let disk_img = dir.join("persist.img");
    let script1 = dir.join("write.txt");
    let script2 = dir.join("read.txt");
    fs::write(&script1, "echo cli-persist-ok > /cli.txt\n").unwrap();
    fs::write(&script2, "cat /cli.txt\n").unwrap();

    let run1 = run_titan(
        &[
            "boot",
            "--kernel",
            kernel.to_str().unwrap(),
            "--disk",
            disk_img.to_str().unwrap(),
            "--script",
            script1.to_str().unwrap(),
        ],
        None,
        None,
    );
    assert_eq!(
        run1.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&run1.stderr)
    );
    assert!(
        disk_img.exists(),
        "expected --disk to create {}",
        disk_img.display()
    );

    let run2 = run_titan(
        &[
            "boot",
            "--kernel",
            kernel.to_str().unwrap(),
            "--disk",
            disk_img.to_str().unwrap(),
            "--script",
            script2.to_str().unwrap(),
        ],
        None,
        None,
    );
    assert_eq!(
        run2.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&run2.stderr)
    );
    let stdout2 = String::from_utf8_lossy(&run2.stdout);
    assert!(stdout2.contains("cli-persist-ok"), "stdout: {stdout2}");

    let _ = fs::remove_dir_all(&dir);
}
