//! Host integration test that builds the RV64 Titan kernel and boots it on Silicon.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use silicon::machine::{Config, Exit, Machine};

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
    let target_dir = root.join("target").join("titan-kernel");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    let status = Command::new(cargo)
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args([
            "build",
            "-p",
            "titan",
            "--bin",
            "kernel",
            "--release",
            "--target",
            "riscv64gc-unknown-none-elf",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "cargo build for titan kernel failed");

    let elf_path = target_dir
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel");
    std::fs::read(&elf_path).unwrap()
}

fn boot_and_verify(elf_bytes: &[u8], ram_size: usize, decode_cache: bool) -> u64 {
    let image = chisel::elf::read(elf_bytes).unwrap();
    let cfg = Config {
        ram_size,
        decode_cache,
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
    assert!(
        steps <= 50_000_000,
        "exceeded 50_000_000 step budget: {steps}"
    );

    let lines: Vec<&str> = transcript.lines().collect();
    // 11 T25 lines + 1 (hello) + 10 (ping/pong) + 12 (rest of T26) = 34 lines
    assert_eq!(
        lines.len(),
        34,
        "unexpected transcript line count ({}):\n{transcript}",
        lines.len()
    );

    let ram_end = 0x8000_0000u64 + (ram_size as u64);
    let mib = ram_size / (1024 * 1024);

    assert_eq!(lines[0], "Titan v0.1");
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

    // T26 transcript lines
    assert_eq!(lines[11], "init: hello pid 1");

    // lines[12..22]: ping 0..4 and pong 0..4, interleaved:
    // at least one pong line before the last ping line.
    let pp = &lines[12..22];
    let mut pings = Vec::new();
    let mut pongs = Vec::new();
    let mut last_ping_idx = None;
    let mut first_pong_idx = None;
    for (idx, &line) in pp.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("ping ") {
            pings.push(rest);
            last_ping_idx = Some(idx);
        } else if let Some(rest) = line.strip_prefix("pong ") {
            pongs.push(rest);
            if first_pong_idx.is_none() {
                first_pong_idx = Some(idx);
            }
        } else {
            panic!("unexpected ping/pong line: {line:?}\n{transcript}");
        }
    }
    assert_eq!(pings, ["0", "1", "2", "3", "4"]);
    assert_eq!(pongs, ["0", "1", "2", "3", "4"]);
    assert!(
        first_pong_idx.unwrap() < last_ping_idx.unwrap(),
        "ping and pong did not interleave:\n{transcript}"
    );

    assert_eq!(lines[22], "init: ping exited 7");
    assert_eq!(lines[23], "init: pong exited 9");
    assert_eq!(lines[24], "hog: sbrk ok");
    assert_eq!(lines[25], "init: hog exited 0");
    assert!(
        lines[26].starts_with("kill pid ") && lines[26].ends_with(": load fault at 0x0"),
        "bad kill fault line: {}",
        lines[26]
    );
    let killed_pid: u32 = lines[26]
        .strip_prefix("kill pid ")
        .unwrap()
        .strip_suffix(": load fault at 0x0")
        .unwrap()
        .parse()
        .unwrap();
    assert!(killed_pid > 1);
    assert_eq!(lines[27], "init: fault exited -1");
    assert_eq!(lines[28], "evil: rejected");
    assert_eq!(lines[29], "init: evil exited 3");
    assert_eq!(lines[30], "1 running init");
    assert_eq!(lines[31], "init: uptime ok");
    assert_eq!(lines[32], "init exited 0");
    assert!(
        lines[33].starts_with("frames ") && lines[33].ends_with(" free"),
        "bad final frames line: {}",
        lines[33]
    );
    let final_frames: u64 = lines[33]
        .strip_prefix("frames ")
        .unwrap()
        .strip_suffix(" free")
        .unwrap()
        .parse()
        .unwrap();
    assert!(final_frames > 0 && final_frames < expected_frames);

    steps
}

#[test]
fn kernel_boots_at_16_mib_and_64_mib() {
    let elf = build_kernel_elf();
    let steps_16 = boot_and_verify(&elf, 16 * 1024 * 1024, true);
    let steps_64 = boot_and_verify(&elf, 64 * 1024 * 1024, true);
    let steps_uncached = boot_and_verify(&elf, 16 * 1024 * 1024, false);
    assert!(steps_16 <= 50_000_000);
    assert!(steps_64 <= 50_000_000);
    assert!(steps_uncached <= 50_000_000);
}
