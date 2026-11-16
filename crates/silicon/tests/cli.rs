//! CLI tests for the `silicon` tool: `--help`, `run` on a hand-assembled ELF, and the step limit.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

#[allow(dead_code)]
#[path = "../../chisel/src/elf.rs"]
mod elf;

/// Prints "OK\n" to the UART at 0x1000_0000, then writes 0x5555 to the finisher at 0x0010_0000.
const PROGRAM: [u32; 12] = [
    0x1000_02b7, // lui   t0, 0x10000
    0x04f0_0313, // addi  t1, zero, 'O'
    0x0062_8023, // sb    t1, 0(t0)
    0x04b0_0313, // addi  t1, zero, 'K'
    0x0062_8023, // sb    t1, 0(t0)
    0x00a0_0313, // addi  t1, zero, '\n'
    0x0062_8023, // sb    t1, 0(t0)
    0x0010_03b7, // lui   t2, 0x100
    0x0000_5e37, // lui   t3, 0x5
    0x555e_0e13, // addi  t3, t3, 0x555
    0x01c3_a023, // sw    t3, 0(t2)
    0x0000_006f, // j     .
];

fn silicon(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_silicon"))
        .args(args)
        .output()
        .unwrap()
}

fn write_program(name: &str) -> PathBuf {
    let code: Vec<u8> = PROGRAM.iter().flat_map(|w| w.to_le_bytes()).collect();
    let img = elf::Image {
        entry: 0x8000_0000,
        segments: vec![elf::Segment {
            vaddr: 0x8000_0000,
            flags: elf::PF_R | elf::PF_X,
            mem_size: code.len() as u64,
            data: code,
        }],
        symbols: Vec::new(),
    };
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, elf::write(&img).unwrap()).unwrap();
    path
}

#[test]
fn help_and_version_exit_zero() {
    let help = silicon(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("silicon run FILE.elf"), "{text}");
    assert!(text.contains("--steps N"), "{text}");

    let version = silicon(&["--version"]);
    assert_eq!(version.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&version.stdout), "silicon 0.1.0\n");
}

#[test]
fn run_prints_uart_and_exits_zero_on_pass() {
    let path = write_program("silicon-cli-ok.elf");
    let out = silicon(&["run", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "OK\n");
}

#[test]
fn run_stops_at_step_limit_with_exit_one() {
    let path = write_program("silicon-cli-steps.elf");
    let out = silicon(&["run", path.to_str().unwrap(), "--steps", "3"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no exit after 3 steps"));
}

#[test]
fn usage_errors_exit_two() {
    let out = silicon(&["bogus"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("silicon: unknown command"));
}
