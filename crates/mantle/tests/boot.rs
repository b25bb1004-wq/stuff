//! Host integration test that builds the RV64 Mantle kernel and boots it on Quartz.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use quartz::machine::{Config, Exit, Machine};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn build_kernel_elf() -> Vec<u8> {
    let root = workspace_root();
    let target_dir = root.join("target").join("mantle-kernel");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    let status = Command::new(cargo)
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args([
            "build",
            "-p",
            "mantle",
            "--bin",
            "kernel",
            "--release",
            "--target",
            "riscv64gc-unknown-none-elf",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "cargo build for mantle kernel failed");

    let elf_path = target_dir
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel");
    std::fs::read(&elf_path).unwrap()
}

fn boot_and_verify(elf_bytes: &[u8], ram_size: usize) -> u64 {
    let image = chisel::elf::read(elf_bytes).unwrap();
    let cfg = Config {
        ram_size,
        ..Config::default()
    };
    let mut machine = Machine::new(cfg);
    chisel::elf::load(&image, &mut machine).unwrap();

    machine.cpu.x[10] = 0; // a0 = hart id 0
    machine.cpu.x[11] = ram_size as u64; // a1 = ram_size

    let exit = machine.run(50_000_000);
    let out_bytes = machine.board.take_uart_output();
    let transcript = String::from_utf8(out_bytes).unwrap();
    let steps = machine.board.mtime();

    assert_eq!(
        exit,
        Exit::Pass,
        "kernel did not exit with Pass (ram_size={ram_size}, steps={steps}):\n{transcript}"
    );

    let lines: Vec<&str> = transcript.lines().collect();
    assert_eq!(
        lines.len(),
        11,
        "unexpected transcript line count:\n{transcript}"
    );

    let ram_end = 0x8000_0000u64 + (ram_size as u64);
    let mib = ram_size / (1024 * 1024);

    assert_eq!(lines[0], "Mantle v0.1");
    assert_eq!(
        lines[1],
        format!("ram 0x80000000..0x{ram_end:x} ({mib} MiB)")
    );
    assert!(
        lines[2].starts_with("heap 0x") && lines[2].contains("..0x"),
        "bad heap line: {}",
        lines[2]
    );

    let heap_end_hex = lines[2].split("..0x").nth(1).unwrap();
    let heap_end = u64::from_str_radix(heap_end_hex, 16).unwrap();
    let expected_frames = (ram_end - heap_end) / 4096;
    assert_eq!(lines[3], format!("frames {expected_frames} free"));

    assert_eq!(lines[4], "sv39 on");
    assert_eq!(lines[5], "heap ok");
    assert!(
        lines[6].starts_with("fault load 0x") && lines[6].ends_with(" scause 13"),
        "bad fault load line: {}",
        lines[6]
    );
    assert!(
        lines[7].starts_with("fault store 0x") && lines[7].ends_with(" scause 15"),
        "bad fault store line: {}",
        lines[7]
    );
    assert!(
        lines[8].starts_with("fault fetch 0x") && lines[8].ends_with(" scause 12"),
        "bad fault fetch line: {}",
        lines[8]
    );
    assert_eq!(lines[9], "ticks ok");
    assert_eq!(lines[10], "boot ok");

    steps
}

#[test]
fn kernel_boots_at_16_mib_and_64_mib() {
    let elf = build_kernel_elf();
    let steps_16 = boot_and_verify(&elf, 16 * 1024 * 1024);
    let steps_64 = boot_and_verify(&elf, 64 * 1024 * 1024);
    let steps_256 = boot_and_verify(&elf, 256 * 1024 * 1024);
    assert!(steps_16 < 50_000_000);
    assert!(steps_64 < 50_000_000);
    assert!(steps_256 < 50_000_000);
}
