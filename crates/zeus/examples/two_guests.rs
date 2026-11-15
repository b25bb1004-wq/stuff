//! Example booting lithium and titan side-by-side under zeus and printing the console.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use silicon::machine::RAM_BASE;
use zeus::{build_machine, Guest, Image};

const MIB: u64 = 1024 * 1024;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn build_elf(pkg: &str, bin: &str, dir: &str) -> Vec<u8> {
    let root = workspace_root();
    let target_dir = root.join("target").join(dir);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args([
            "build",
            "-p",
            pkg,
            "--bin",
            bin,
            "--release",
            "--target",
            "riscv64gc-unknown-none-elf",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "failed to build {pkg}/{bin}");
    std::fs::read(
        target_dir
            .join("riscv64gc-unknown-none-elf")
            .join("release")
            .join(bin),
    )
    .unwrap()
}

fn main() {
    let zeus_elf = build_elf("zeus", "zeus", "zeus-bin");
    let titan_elf = build_elf("titan", "kernel", "titan-kernel");
    let lithium_bytes = lithium::image().unwrap();

    let guests = [
        Guest {
            name: "lithium".into(),
            image: Image::Raw {
                bytes: lithium_bytes,
                gpa: RAM_BASE,
            },
            size: 4 * MIB,
            a1: 0,
        },
        Guest {
            name: "titan".into(),
            image: Image::Elf(titan_elf),
            size: 16 * MIB,
            a1: 16 * MIB,
        },
    ];

    let mut machine = build_machine(&zeus_elf, &guests).unwrap();
    let exit = machine.run(80_000_000);
    let out = machine.board.take_uart_output();
    print!("{}", String::from_utf8_lossy(&out));
    eprintln!("machine exit: {exit:?} (steps={})", machine.board.mtime());
}
