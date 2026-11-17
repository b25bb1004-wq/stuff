//! Host tests for the door device and calls 16, 22, 23, 25, booting the kernel on Silicon.
//!
//! Each test boots with a disk whose `/etc/boot` names the program to run as PID 1.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use silicon::machine::{Config, Machine};

const RAM: usize = 16 * 1024 * 1024;
const STEP_BUDGET: u64 = 50_000_000;

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

fn boot_disk(prog: &str) -> Vec<u8> {
    let mut disk = hades::MemDisk::new(256);
    hades::mkfs(&mut disk, hades::MkfsOptions::default()).unwrap();
    let mut fs = hades::Fs::mount(disk).unwrap();
    fs.mkdir("/etc").unwrap();
    fs.create("/etc/boot").unwrap();
    fs.write("/etc/boot", 0, prog.as_bytes()).unwrap();
    fs.unmount().unwrap().bytes().to_vec()
}

fn boot(elf_bytes: &[u8], prog: &str) -> Machine {
    let image = chisel::elf::read(elf_bytes).unwrap();
    let cfg = Config {
        ram_size: RAM,
        disk: boot_disk(prog),
        ..Config::default()
    };
    let mut machine = Machine::new(cfg);
    chisel::elf::load(&image, &mut machine).unwrap();
    machine.cpu.x[10] = 0;
    machine.cpu.x[11] = RAM as u64;
    machine
}

#[test]
fn doorecho_echoes_two_frames_on_their_channels() {
    let elf = build_kernel_elf();
    let mut machine = boot(&elf, "doorecho");
    machine.door_push_to_guest(0, b"first");
    machine.door_push_to_guest(1, b"second frame");
    let _ = machine.run(STEP_BUDGET);
    assert_eq!(
        machine.door_take_from_guest(),
        vec![(0, b"first".to_vec()), (1, b"second frame".to_vec())]
    );
    assert_eq!(machine.door_dropped(), 0);
}

#[test]
fn resultp_prints_child_result() {
    let elf = build_kernel_elf();
    let mut machine = boot(&elf, "resultp");
    let _ = machine.run(STEP_BUDGET);
    let out = String::from_utf8(machine.board.take_uart_output()).unwrap();
    assert!(out.contains(r#"{"ok":true}"#), "uart output:\n{out}");
}

#[test]
fn doorbad_is_refused_without_the_door_right() {
    let elf = build_kernel_elf();
    let mut machine = boot(&elf, "doorbad");
    let _ = machine.run(STEP_BUDGET);
    let out = String::from_utf8(machine.board.take_uart_output()).unwrap();
    assert!(out.contains("refused"), "uart output:\n{out}");
}
