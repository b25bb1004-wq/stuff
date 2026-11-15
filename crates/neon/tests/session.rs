//! Host integration tests for `neon::session::Titan`.
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

const SESSION_LINES: [&str; 14] = [
    "echo hi there",
    "ls /",
    "cat /hello.txt",
    "mkdir /tmp",
    "echo saved > /tmp/out.txt",
    "cat < /tmp/out.txt",
    "echo more >> /tmp/out.txt",
    "cat /tmp/out.txt",
    "rm /hello.txt && echo removed",
    "cat /nope || echo failed $?",
    "X=7; echo x=$X",
    "nosuch",
    "ls / | cat",
    "exit 3",
];

const EXPECTED_SHELL: &str = concat!(
    "$ hi there\n",
    "$ dir\netc\nhello.txt\n",
    "$ hello$ $ $ saved\n",
    "$ $ saved\nmore\n",
    "$ removed\n",
    "$ cat: /nope: error -2\nfailed 1\n",
    "$ x=7\n",
    "$ sh: nosuch: not found\n",
    "$ sh: unsupported\n",
    "$ ",
);

fn pump_until_prompt(titan: &mut Titan, chunk: u64, out: &mut Vec<u8>) -> usize {
    let mut busy_count = 0usize;
    for _ in 0..5000 {
        let p = titan.pump(chunk);
        out.extend_from_slice(&titan.take_output());
        match p {
            Pump::WaitingForInput => return busy_count,
            Pump::Busy => busy_count += 1,
            Pump::Exited(code) => {
                let text = String::from_utf8_lossy(out);
                panic!("unexpected exit {code} before prompt:\n{text}");
            }
        }
    }
    let text = String::from_utf8_lossy(out);
    panic!("timed out waiting for prompt:\n{text}");
}

fn pump_until_exit(titan: &mut Titan, chunk: u64, out: &mut Vec<u8>) -> (i32, usize) {
    let mut busy_count = 0usize;
    for _ in 0..5000 {
        let p = titan.pump(chunk);
        out.extend_from_slice(&titan.take_output());
        match p {
            Pump::Exited(code) => return (code, busy_count),
            Pump::Busy => busy_count += 1,
            Pump::WaitingForInput => {
                let text = String::from_utf8_lossy(out);
                panic!("unexpected prompt when waiting for exit:\n{text}");
            }
        }
    }
    let text = String::from_utf8_lossy(out);
    panic!("timed out waiting for exit:\n{text}");
}

fn run_session(chunk: u64) -> (String, usize) {
    let mut titan = Titan::boot(kernel_elf()).unwrap();
    let mut out = Vec::new();
    let mut total_busy = 0usize;

    for line in SESSION_LINES {
        total_busy += pump_until_prompt(&mut titan, chunk, &mut out);
        titan.send(line);
    }

    let (code, exit_busy) = pump_until_exit(&mut titan, chunk, &mut out);
    total_busy += exit_busy;
    assert_eq!(code, 0, "guest failed with exit code {code}");

    let transcript = String::from_utf8(out).unwrap();
    (transcript, total_busy)
}

#[test]
fn session_matches_titan_shell_transcript() {
    let (transcript, _) = run_session(2_000_000);
    let start_marker = "init: fsdemo exited 0\n";
    let end_marker = "init: sh exited 3\n";
    let start_idx = transcript
        .find(start_marker)
        .expect("missing fsdemo exit line")
        + start_marker.len();
    let end_idx = transcript.find(end_marker).expect("missing sh exit line");
    let shell_out = &transcript[start_idx..end_idx];

    assert_eq!(shell_out, EXPECTED_SHELL, "full:\n{transcript}");
    assert!(transcript.contains("1 running init\n"));
    assert!(transcript.contains("init exited 0\n"));
}

#[test]
fn small_pump_slices_lose_no_output() {
    let (transcript, busy_count) = run_session(25_000);
    assert!(
        busy_count >= 10,
        "expected many Busy stops with small max_steps, got {busy_count}"
    );
    let start_marker = "init: fsdemo exited 0\n";
    let end_marker = "init: sh exited 3\n";
    let start_idx = transcript.find(start_marker).unwrap() + start_marker.len();
    let end_idx = transcript.find(end_marker).unwrap();
    assert_eq!(&transcript[start_idx..end_idx], EXPECTED_SHELL);
}

#[test]
fn idle_prompt_does_not_hit_eof_after_ten_pumps() {
    let mut titan = Titan::boot(kernel_elf()).unwrap();
    let mut out = Vec::new();
    pump_until_prompt(&mut titan, 2_000_000, &mut out);
    assert!(out.ends_with(b"$ "));

    for i in 0..10 {
        let p = titan.pump(100_000);
        assert_eq!(
            p,
            Pump::WaitingForInput,
            "pump {i} should stay WaitingForInput instead of hitting EOF/exiting"
        );
        assert!(titan.take_output().is_empty());
    }
}
