//! Build script for `titan`: passes `kernel.ld` for `riscv64*` and compiles built-in user ELFs.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chisel::elf::{Image, Segment, PF_R, PF_W, PF_X};

const USER_BASE: u64 = 0x1_0000;
const USER_PROGS: [&str; 7] = ["init", "ping", "pong", "hog", "fault", "evil", "fsdemo"];

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");
    println!("cargo:rerun-if-changed=user.ld");
    println!("cargo:rerun-if-changed=user");
    println!("cargo:rerun-if-changed=src/bin/userland.rs");
    println!("cargo:rerun-if-changed=src/bin/userland");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap_or_default());

    for name in USER_PROGS {
        build_user_elf(&manifest_dir, &out_dir, name);
    }

    let target = env::var("TARGET").unwrap_or_default();
    if target.starts_with("riscv64") {
        let kernel_ld = manifest_dir.join("kernel.ld");
        let user_ld = manifest_dir.join("user.ld");
        println!("cargo:rustc-link-arg-bin=kernel=-T{}", kernel_ld.display());
        println!("cargo:rustc-link-arg-bin=userland=-T{}", user_ld.display());

        if env::var("TITAN_NESTED").as_deref() != Ok("1") {
            let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
            let userland_target = out_dir.join("userland-target");
            let status = std::process::Command::new(cargo)
                .current_dir(&manifest_dir)
                .env("TITAN_NESTED", "1")
                .env("CARGO_TARGET_DIR", &userland_target)
                .args([
                    "build",
                    "--release",
                    "-p",
                    "titan",
                    "--bin",
                    "userland",
                    "--target",
                    "riscv64gc-unknown-none-elf",
                ])
                .status()
                .unwrap_or_else(|e| panic!("failed to run nested userland build: {e}"));
            assert!(status.success(), "nested userland cargo build failed");

            let built = userland_target
                .join("riscv64gc-unknown-none-elf")
                .join("release")
                .join("userland");
            let dest = out_dir.join("userland.elf");
            fs::copy(&built, &dest).unwrap_or_else(|e| {
                panic!(
                    "failed to copy {} to {}: {e}",
                    built.display(),
                    dest.display()
                )
            });
        }
    }
}

fn build_user_elf(manifest_dir: &Path, out_dir: &Path, name: &str) {
    let src_path = manifest_dir.join("user").join(format!("{name}.s"));
    println!("cargo:rerun-if-changed={}", src_path.display());
    let src = fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));
    let out = chisel::assemble(&src, USER_BASE)
        .unwrap_or_else(|e| panic!("failed to assemble {}: {e}", src_path.display()));

    let entry = out
        .symbols
        .iter()
        .find(|(sym, _)| sym == "_start")
        .map_or(USER_BASE, |(_, addr)| *addr);

    let data_addr = out
        .symbols
        .iter()
        .find(|(sym, _)| sym == "_data")
        .map(|(_, addr)| *addr);

    let mut segments = Vec::new();
    if let Some(d_addr) = data_addr {
        let split = usize::try_from(d_addr - USER_BASE)
            .unwrap_or_else(|_| panic!("{name}: invalid _data address"));
        segments.push(Segment {
            vaddr: USER_BASE,
            flags: PF_R | PF_X,
            data: out.bytes[..split].to_vec(),
            mem_size: split as u64,
        });
        let data_bytes = out.bytes[split..].to_vec();
        let data_len = data_bytes.len() as u64;
        segments.push(Segment {
            vaddr: d_addr,
            flags: PF_R | PF_W,
            data: data_bytes,
            mem_size: data_len,
        });
    } else {
        let len = out.bytes.len() as u64;
        segments.push(Segment {
            vaddr: USER_BASE,
            flags: PF_R | PF_X,
            data: out.bytes,
            mem_size: len,
        });
    }

    let elf = chisel::elf::write(&Image {
        entry,
        segments,
        symbols: out.symbols,
    })
    .expect("chisel elf write");

    let dest = out_dir.join(format!("{name}.elf"));
    fs::write(&dest, elf).unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));
}
