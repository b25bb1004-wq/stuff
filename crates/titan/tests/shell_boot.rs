//! Integration tests for Titan userland multicall binary, krypton shell, argv, and redirection.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use assay::Rng;
use hades::{fsck, mkfs, Fs, Kind, MemDisk, MkfsOptions};
use silicon::machine::{Config, Exit, Machine};

const DISK_BLOCKS: u64 = 256;
const RAM_SIZE: usize = 16 * 1024 * 1024;

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

fn build_shell_disk() -> Vec<u8> {
    let mut disk = MemDisk::new(DISK_BLOCKS);
    mkfs(&mut disk, MkfsOptions::default()).unwrap();
    let mut fs = Fs::mount(disk).unwrap();
    fs.create("/hello.txt").unwrap();
    assert_eq!(fs.write("/hello.txt", 0, b"hello").unwrap(), 5);
    fs.mkdir("/etc").unwrap();
    fs.create("/etc/shell").unwrap();
    let disk = fs.unmount().unwrap();
    disk.bytes().to_vec()
}

fn boot_shell(disk: Vec<u8>, input: &[u8]) -> (Exit, String, Vec<u8>) {
    let image = chisel::elf::read(kernel_elf()).unwrap();
    let cfg = Config {
        ram_size: RAM_SIZE,
        disk,
        decode_cache: true,
        ..Config::default()
    };
    let mut machine = Machine::new(cfg);
    chisel::elf::load(&image, &mut machine).unwrap();
    machine.cpu.x[10] = 0;
    machine.cpu.x[11] = RAM_SIZE as u64;
    machine.board.uart_input(input);

    let exit = machine.run(50_000_000);
    let transcript = String::from_utf8(machine.board.take_uart_output()).unwrap();
    let disk_after = machine.board.disk().to_vec();
    (exit, transcript, disk_after)
}

#[test]
fn shell_session_transcript_and_disk_persistence() {
    let script = concat!(
        "echo hi there\n",
        "ls /\n",
        "cat /hello.txt\n",
        "mkdir /tmp\n",
        "echo saved > /tmp/out.txt\n",
        "cat < /tmp/out.txt\n",
        "echo more >> /tmp/out.txt\n",
        "cat /tmp/out.txt\n",
        "rm /hello.txt && echo removed\n",
        "cat /nope || echo failed $?\n",
        "X=7; echo x=$X\n",
        "nosuch\n",
        "ls / | cat\n",
        "exit 3\n",
    );

    let (exit, transcript, disk_after) = boot_shell(build_shell_disk(), script.as_bytes());
    assert_eq!(exit, Exit::Pass, "boot failed:\n{transcript}");

    let start_marker = "init: fsdemo exited 0\n";
    let end_marker = "init: sh exited 3\n";
    let start_idx = transcript
        .find(start_marker)
        .expect("missing fsdemo exit line")
        + start_marker.len();
    let end_idx = transcript.find(end_marker).expect("missing sh exit line");
    let shell_out = &transcript[start_idx..end_idx];

    let expected_shell = concat!(
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
    assert_eq!(shell_out, expected_shell, "full:\n{transcript}");
    assert!(transcript.contains("1 running init\n"));
    assert!(transcript.contains("init exited 0\n"));

    let mut mem = MemDisk::new(DISK_BLOCKS);
    mem.bytes_mut().copy_from_slice(&disk_after);
    let rep = fsck(&mut mem, false).unwrap();
    assert!(rep.errors.is_empty(), "fsck errors: {:?}", rep.errors);

    let mut fs = Fs::mount(mem).unwrap();
    assert!(fs.stat("/hello.txt").is_err(), "/hello.txt should be gone");
    assert_eq!(fs.stat("/tmp").unwrap().kind, Kind::Dir);
    let mut buf = [0u8; 32];
    let n = fs.read("/tmp/out.txt", 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"saved\nmore\n");
}

#[test]
fn shell_argv_edges_missing_redir_and_eof() {
    let long_255 = "a".repeat(255);
    let too_long_256 = "b".repeat(256);
    let fifteen_args = (1..=15)
        .map(|i| format!("a{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let sixteen_args = (1..=16)
        .map(|i| format!("b{i}"))
        .collect::<Vec<_>>()
        .join(" ");

    let script = format!(
        "echo {fifteen_args}\n\
         echo {long_255}\n\
         echo {sixteen_args}\n\
         echo {too_long_256}\n\
         echo hi > /missing/out.txt\n\
         ls /hello.txt\n\
         cat /etc\n\
         mkdir /etc\n\
         rm /nope\n\
         ps\n"
    );

    let (exit, transcript, _) = boot_shell(build_shell_disk(), script.as_bytes());
    assert_eq!(exit, Exit::Pass, "boot failed:\n{transcript}");
    assert!(!transcript.contains("FAIL"), "kernel FAIL:\n{transcript}");
    assert!(!transcript.contains("panic"), "panic:\n{transcript}");
    assert!(transcript.contains(&format!("$ {fifteen_args}\n")));
    assert!(transcript.contains(&format!("$ {long_255}\n")));
    assert!(transcript.contains("sh: echo: error -1\n"));
    assert!(transcript.contains("sh: /missing/out.txt: error -2\n"));
    assert!(transcript.contains("ls: /hello.txt: error -6\n"));
    assert!(transcript.contains("cat: /etc: error -1\n"));
    assert!(transcript.contains("mkdir: /etc: error -5\n"));
    assert!(transcript.contains("rm: /nope: error -2\n"));
    assert!(transcript.contains("init: sh exited 0\n"));
}

#[test]
fn shell_empty_input_hits_eof_cleanly() {
    let (exit, transcript, _) = boot_shell(build_shell_disk(), b"");
    assert_eq!(exit, Exit::Pass, "empty input boot failed:\n{transcript}");
    assert!(transcript.contains("init: fsdemo exited 0\n$ init: sh exited 0\n"));
}

#[test]
fn shell_random_command_lines_never_panic_or_fail() {
    let mut rng = Rng::new(0x28_54E11);
    let tokens = [
        "echo",
        "ls",
        "cat",
        "mkdir",
        "rm",
        "ps",
        "cd",
        "exit",
        "nosuch",
        "/",
        "/hello.txt",
        "/nope",
        "/a/b",
        "x",
        "1",
        ";",
        "&&",
        "||",
        "|",
        ">",
        ">>",
        "<",
        "X=1",
        "$X",
        "$?",
        "\"q\"",
        "'s'",
        "&",
        "(",
        ")",
        "if",
        "fi",
    ];
    let mut script = String::new();
    for _ in 0..40 {
        let len = 1 + rng.below(7);
        for i in 0..len {
            if i > 0 {
                script.push(' ');
            }
            let tok = *rng.pick(&tokens);
            if tok == "exit" {
                script.push_str("echo");
            } else {
                script.push_str(tok);
            }
        }
        script.push('\n');
    }
    script.push_str("exit 0\n");

    let (exit, transcript, disk_after) = boot_shell(build_shell_disk(), script.as_bytes());
    assert_eq!(exit, Exit::Pass, "random shell boot failed:\n{transcript}");
    assert!(!transcript.contains("FAIL"), "kernel FAIL:\n{transcript}");
    assert!(!transcript.contains("panic"), "panic:\n{transcript}");
    assert!(transcript.contains("init: sh exited 0\n"));

    let mut mem = MemDisk::new(DISK_BLOCKS);
    mem.bytes_mut().copy_from_slice(&disk_after);
    let rep = fsck(&mut mem, false).unwrap();
    assert!(rep.errors.is_empty(), "fsck errors: {:?}", rep.errors);
}

/// Leader check: raw random bytes (non-UTF-8, NULs, 1000-byte lines, stray quotes) never panic the shell or the
/// kernel, and the shell still reaches EOF and exits through init.
#[test]
fn leader_raw_byte_input_fresh_seed() {
    let mut rng = Rng::new(0x28_1EAD_2026);
    let mut input = Vec::new();
    for _ in 0..30 {
        let len = if rng.below(10) == 0 {
            1000
        } else {
            rng.below(40)
        };
        for _ in 0..len {
            let b = match rng.below(4) {
                0 => rng.below(256) as u8,
                1 => *rng.pick(b"'\"$;&|<>(){}\\ \t"),
                _ => b'a' + rng.below(26) as u8,
            };
            input.push(if b == b'\n' { b' ' } else { b });
        }
        input.push(b'\n');
    }
    let (exit, transcript, _) = boot_shell(build_shell_disk(), &input);
    assert_eq!(exit, Exit::Pass, "{transcript}");
    assert!(
        !transcript.contains("FAIL") && !transcript.contains("panic"),
        "{transcript}"
    );
    assert!(transcript.contains("init: sh exited"), "{transcript}");
    assert!(
        transcript.matches("$ ").count() >= 30,
        "every line gets a prompt:\n{transcript}"
    );
}
