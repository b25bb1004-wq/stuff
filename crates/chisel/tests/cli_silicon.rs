//! Integration tests for the `silicon` CLI binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use chisel::elf::{self, Image, Segment, PF_R, PF_X};
use silicon::machine::RAM_BASE;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_silicon")
}

fn temp_file(name: &str, data: &[u8]) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "chisel_silicon_{}_{}_{}",
        std::process::id(),
        n,
        name
    ));
    fs::write(&path, data).unwrap();
    path
}

fn build_elf(asm: &str, vaddr: u64) -> Vec<u8> {
    let out = chisel::assemble(asm, vaddr).unwrap();
    let img = Image {
        entry: vaddr,
        segments: vec![Segment {
            vaddr,
            flags: PF_R | PF_X,
            data: out.bytes.clone(),
            mem_size: out.bytes.len() as u64,
        }],
        symbols: out.symbols,
    };
    elf::write(&img)
}

fn hi_elf() -> Vec<u8> {
    build_elf(
        "
        li t0, 0x10000000
        li t1, 0x68
        sb t1, 0(t0)
        li t1, 0x69
        sb t1, 0(t0)
        li t0, 0x00100000
        li t1, 0x5555
        sw t1, 0(t0)
        ",
        RAM_BASE,
    )
}

#[test]
fn help_and_version() {
    let help = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("run"), "help missing run: {stdout}");
    assert!(
        stdout.contains("Examples:"),
        "help missing examples: {stdout}"
    );

    let ver = Command::new(bin()).arg("--version").output().unwrap();
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ver.stdout).trim(), "silicon 0.1.0");
}

#[test]
fn run_hi_elf() {
    let path = temp_file("hi.elf", &hi_elf());
    let out = Command::new(bin())
        .args(["run", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hi\n"), "missing UART output: {stdout}");
    assert!(stdout.contains("exit: Pass\n"), "missing exit: {stdout}");
    assert!(stdout.contains("instret: "), "missing instret: {stdout}");
}

#[test]
fn run_from_stdin() {
    let elf_bytes = hi_elf();
    let mut child = Command::new(bin())
        .args(["run", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&elf_bytes).unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hi\n"), "missing UART output: {stdout}");
    assert!(stdout.contains("exit: Pass\n"), "missing exit: {stdout}");
}

#[test]
fn run_trace_and_steps_cap() {
    let loop_elf = build_elf(
        "
        loop:
            addi t0, t0, 1
            j loop
        ",
        RAM_BASE,
    );
    let path = temp_file("loop.elf", &loop_elf);
    let out = Command::new(bin())
        .args([
            "run",
            path.to_str().unwrap(),
            "--steps",
            "1200",
            "--ram",
            "1",
            "--trace",
        ])
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // 1000 trace lines + exit line + instret line = 1002 lines.
    assert_eq!(lines.len(), 1002, "unexpected line count: {}", lines.len());
    assert!(
        lines[0].starts_with("0x0000000080000000 0x"),
        "bad trace line: {}",
        lines[0]
    );
    assert_eq!(lines[1000], "exit: OutOfSteps");
    assert_eq!(lines[1001], "instret: 1200");
}

#[test]
fn run_guest_fail_exits_one() {
    let fail_elf = build_elf(
        "
        li t0, 0x00100000
        li t1, 0x00023333
        sw t1, 0(t0)
        ",
        RAM_BASE,
    );
    let path = temp_file("fail.elf", &fail_elf);
    let out = Command::new(bin())
        .args(["run", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("exit: Fail(2)\n"), "stdout: {stdout}");
    assert!(stderr.contains("silicon:"), "stderr: {stderr}");
}

#[test]
fn run_bad_inputs_exit_one() {
    let bad_elf = temp_file("bad.elf", b"not an elf");
    let out = Command::new(bin())
        .args(["run", bad_elf.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&bad_elf);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("silicon:"));

    let missing = Command::new(bin())
        .args(["run", "definitely_missing_chisel_silicon_file.elf"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("silicon:"));

    // Valid ELF pointing outside guest RAM.
    let unmapped_elf = build_elf("nop", 0x1000_0000_0000);
    let unmapped_path = temp_file("unmapped.elf", &unmapped_elf);
    let unmapped = Command::new(bin())
        .args(["run", unmapped_path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&unmapped_path);
    assert_eq!(unmapped.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unmapped.stderr).contains("silicon:"));
}

#[test]
fn usage_errors_exit_two() {
    let unknown = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("silicon:"));

    let no_args = Command::new(bin()).output().unwrap();
    assert_eq!(no_args.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&no_args.stderr).contains("silicon:"));

    let bad_flag = Command::new(bin())
        .args(["run", "foo.elf", "--nope"])
        .output()
        .unwrap();
    assert_eq!(bad_flag.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad_flag.stderr).contains("silicon:"));

    let bad_steps = Command::new(bin())
        .args(["run", "foo.elf", "--steps", "abc"])
        .output()
        .unwrap();
    assert_eq!(bad_steps.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad_steps.stderr).contains("silicon:"));
}
