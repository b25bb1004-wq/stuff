//! Probe counters for `neon::session::Titan` after a real shell command.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use neon::session::{Pump, Titan};

fn kernel_elf() -> &'static [u8] {
    static ELF: OnceLock<Vec<u8>> = OnceLock::new();
    ELF.get_or_init(|| {
        let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
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

        std::fs::read(
            target_dir
                .join("riscv64gc-unknown-none-elf")
                .join("release")
                .join("kernel"),
        )
        .unwrap()
    })
}

/// Pump until the shell prompt waits for input, appending every output byte to `out`.
fn pump_until_prompt(titan: &mut Titan, out: &mut Vec<u8>) {
    for _ in 0..5000 {
        let p = titan.pump(2_000_000);
        out.extend_from_slice(&titan.take_output());
        match p {
            Pump::WaitingForInput => return,
            Pump::Busy => {}
            Pump::Exited(code) => panic!("unexpected exit {code}"),
        }
    }
    panic!("timed out waiting for prompt");
}

#[test]
fn ls_moves_probe_counters_and_uart_out_matches_output() {
    let mut titan = Titan::boot(kernel_elf()).unwrap();
    let mut out = Vec::new();
    pump_until_prompt(&mut titan, &mut out);
    titan.send("ls");
    pump_until_prompt(&mut titan, &mut out);

    let p = titan.probe();
    assert!(p.ecall_total() > 0, "no ecalls counted");
    assert!(p.block_reads > 0, "no block reads counted");
    assert_eq!(p.uart_out, out.len() as u64, "uart_out vs session output");

    let text = titan.probe_text();
    let rows: Vec<(&str, &str)> = text.lines().filter_map(|l| l.split_once(' ')).collect();
    assert_eq!(
        rows.len(),
        text.lines().count(),
        "every line is `name value`"
    );
    let names: Vec<&str> = rows.iter().map(|r| r.0).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "counter names are sorted");
    let value = |name: &str| rows.iter().find(|r| r.0 == name).map(|r| r.1.to_owned());
    assert_eq!(value("uart.out"), Some(out.len().to_string()));
    assert_eq!(value("ecall.count"), Some(p.ecall_total().to_string()));
    assert_eq!(value("block.reads"), Some(p.block_reads.to_string()));
}
