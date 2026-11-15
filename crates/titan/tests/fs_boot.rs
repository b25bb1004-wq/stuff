//! Integration tests for Titan block driver, Hades root filesystem mount, and file syscalls.
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

fn boot_with_disk(disk: Vec<u8>) -> (Exit, String, Vec<u8>) {
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

    let exit = machine.run(50_000_000);
    let transcript = String::from_utf8(machine.board.take_uart_output()).unwrap();
    let disk_after = machine.board.disk().to_vec();
    (exit, transcript, disk_after)
}

fn build_test_disk(blocks: u64, extra_files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut disk = MemDisk::new(blocks);
    mkfs(&mut disk, MkfsOptions::default()).unwrap();
    let mut fs = Fs::mount(disk).unwrap();
    fs.create("/hello.txt").unwrap();
    assert_eq!(fs.write("/hello.txt", 0, b"hello").unwrap(), 5);
    for (path, data) in extra_files {
        fs.create(path).unwrap();
        assert_eq!(fs.write(path, 0, data).unwrap(), data.len());
    }
    let disk = fs.unmount().unwrap();
    disk.bytes().to_vec()
}

fn assert_t25_prefix(lines: &[&str]) {
    assert_eq!(lines[0], "Titan v0.1");
    assert!(lines[1].starts_with("ram 0x80000000..0x"));
    assert!(lines[2].starts_with("heap 0x"));
    assert!(lines[3].starts_with("frames ") && lines[3].ends_with(" free"));
    assert_eq!(lines[4], "sv39 on");
    assert_eq!(lines[5], "heap ok");
    assert!(lines[6].starts_with("fault load 0x") && lines[6].ends_with(" scause 13"));
    assert!(lines[7].starts_with("fault store 0x") && lines[7].ends_with(" scause 15"));
    assert!(lines[8].starts_with("fault fetch 0x") && lines[8].ends_with(" scause 12"));
    assert_eq!(lines[9], "ticks ok");
    assert_eq!(lines[10], "boot ok");
}

fn assert_t26_body(lines: &[&str], expect_fsdemo: bool, expected_ls: Option<&str>) {
    assert_eq!(lines[0], "init: hello pid 1");

    let mut pings = Vec::new();
    let mut pongs = Vec::new();
    for &line in &lines[1..11] {
        if let Some(v) = line.strip_prefix("ping ") {
            pings.push(v);
        } else if let Some(v) = line.strip_prefix("pong ") {
            pongs.push(v);
        } else {
            panic!("unexpected ping/pong line: {line:?}");
        }
    }
    assert_eq!(pings, ["0", "1", "2", "3", "4"]);
    assert_eq!(pongs, ["0", "1", "2", "3", "4"]);

    assert_eq!(lines[11], "init: ping exited 7");
    assert_eq!(lines[12], "init: pong exited 9");
    assert_eq!(lines[13], "hog: sbrk ok");
    assert_eq!(lines[14], "init: hog exited 0");
    assert!(lines[15].starts_with("kill pid ") && lines[15].ends_with(": load fault at 0x0"));
    assert_eq!(lines[16], "init: fault exited -1");
    assert_eq!(lines[17], "evil: rejected");
    assert_eq!(lines[18], "init: evil exited 3");

    let rest = if expect_fsdemo {
        assert_eq!(lines[19], "fsdemo: read hello");
        assert_eq!(lines[20], "fsdemo: wrote 5");
        if let Some(ls_line) = expected_ls {
            assert_eq!(lines[21], ls_line);
        } else {
            assert!(lines[21].starts_with("fsdemo: ls "));
        }
        assert_eq!(lines[22], "fsdemo: errors ok");
        assert_eq!(lines[23], "fsdemo: unlink ok");
        assert_eq!(lines[24], "init: fsdemo exited 0");
        &lines[25..]
    } else {
        &lines[19..]
    };

    assert_eq!(rest.len(), 4);
    assert_eq!(rest[0], "1 running init");
    assert_eq!(rest[1], "init: uptime ok");
    assert_eq!(rest[2], "init exited 0");
    assert!(rest[3].starts_with("frames ") && rest[3].ends_with(" free"));
}

#[test]
fn fs_boot_transcript_host_verify_and_persistence() {
    let initial_disk = build_test_disk(DISK_BLOCKS, &[]);
    let (exit, transcript, disk_after) = boot_with_disk(initial_disk);
    assert_eq!(exit, Exit::Pass, "boot failed:\n{transcript}");

    let lines: Vec<&str> = transcript.lines().collect();
    assert_eq!(lines.len(), 41, "expected 41 lines:\n{transcript}");
    assert_t25_prefix(&lines[..11]);
    assert_eq!(lines[11], format!("fs: mounted {DISK_BLOCKS} blocks"));
    assert_t26_body(&lines[12..], true, Some("fsdemo: ls a.txt dir hello.txt"));

    // Verify disk state and fsck on host after run.
    let mut mem = MemDisk::new(DISK_BLOCKS);
    mem.bytes_mut().copy_from_slice(&disk_after);
    let rep = fsck(&mut mem, false).unwrap();
    assert!(rep.errors.is_empty(), "fsck errors: {:?}", rep.errors);

    let mut fs = Fs::mount(mem).unwrap();
    assert!(fs.stat("/a.txt").is_err(), "/a.txt should be unlinked");
    assert_eq!(fs.stat("/dir").unwrap().kind, Kind::Dir);
    let mut buf = [0u8; 16];
    let n = fs.read("/hello.txt", 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello");
    let mem = fs.unmount().unwrap();

    // Persistence: second boot on the resulting disk still passes.
    let (exit2, transcript2, disk_after2) = boot_with_disk(mem.bytes().to_vec());
    assert_eq!(exit2, Exit::Pass, "second boot failed:\n{transcript2}");
    let lines2: Vec<&str> = transcript2.lines().collect();
    assert_eq!(
        lines2.len(),
        41,
        "expected 41 lines on reboot:\n{transcript2}"
    );
    assert_t25_prefix(&lines2[..11]);
    assert_eq!(lines2[11], format!("fs: mounted {DISK_BLOCKS} blocks"));
    assert_t26_body(&lines2[12..], true, Some("fsdemo: ls a.txt dir hello.txt"));

    let mut mem2 = MemDisk::new(DISK_BLOCKS);
    mem2.bytes_mut().copy_from_slice(&disk_after2);
    let rep2 = fsck(&mut mem2, false).unwrap();
    assert!(rep2.errors.is_empty(), "fsck2 errors: {:?}", rep2.errors);
}

#[test]
fn empty_disk_matches_t26_transcript() {
    let (exit, transcript, _) = boot_with_disk(Vec::new());
    assert_eq!(exit, Exit::Pass, "empty-disk boot failed:\n{transcript}");
    let lines: Vec<&str> = transcript.lines().collect();
    assert_eq!(lines.len(), 34, "expected 34 lines:\n{transcript}");
    assert_t25_prefix(&lines[..11]);
    assert_t26_body(&lines[11..], false, None);
}

#[test]
fn corrupted_disk_fails_mount_cleanly() {
    let mut bad_disk = build_test_disk(DISK_BLOCKS, &[]);
    // Corrupt the superblock magic/CRC in block 0.
    for b in &mut bad_disk[..64] {
        *b ^= 0xFF;
    }
    let (exit, transcript, _) = boot_with_disk(bad_disk);
    assert_eq!(
        exit,
        Exit::Pass,
        "corrupted-disk boot failed:\n{transcript}"
    );
    let lines: Vec<&str> = transcript.lines().collect();
    assert_eq!(lines.len(), 35, "expected 35 lines:\n{transcript}");
    assert_t25_prefix(&lines[..11]);
    assert_eq!(lines[11], "fs: mount failed");
    assert_t26_body(&lines[12..], false, None);
}

#[test]
fn preseeded_extra_files_and_fsck_clean() {
    let mut rng = Rng::new(0x27_5EED);
    let mut extra_names = Vec::new();
    let mut extra_data = Vec::new();
    for i in 0..4 {
        let len = 4 + rng.below(20);
        let mut payload = vec![0u8; len];
        for b in &mut payload {
            *b = b'a' + (rng.below(26) as u8);
        }
        extra_names.push(format!("/z{i}_{}.dat", rng.below(1000)));
        extra_data.push(payload);
    }
    let extra_refs: Vec<(&str, &[u8])> = extra_names
        .iter()
        .zip(extra_data.iter())
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();

    let disk = build_test_disk(DISK_BLOCKS, &extra_refs);
    let (exit, transcript, disk_after) = boot_with_disk(disk);
    assert_eq!(exit, Exit::Pass, "extra-files boot failed:\n{transcript}");
    let lines: Vec<&str> = transcript.lines().collect();
    assert_eq!(lines.len(), 41, "expected 41 lines:\n{transcript}");
    assert_t25_prefix(&lines[..11]);
    assert_eq!(lines[11], format!("fs: mounted {DISK_BLOCKS} blocks"));
    assert_t26_body(&lines[12..], true, None);

    let ls_line = lines[33];
    for name in &extra_names {
        let short = name.trim_start_matches('/');
        assert!(ls_line.contains(short), "ls missing {short}: {ls_line}");
    }

    let mut mem = MemDisk::new(DISK_BLOCKS);
    mem.bytes_mut().copy_from_slice(&disk_after);
    let rep = fsck(&mut mem, false).unwrap();
    assert!(rep.errors.is_empty(), "fsck errors: {:?}", rep.errors);
    let mut fs = Fs::mount(mem).unwrap();
    for (name, data) in extra_refs {
        let mut got = vec![0u8; data.len() + 4];
        let n = fs.read(name, 0, &mut got).unwrap();
        assert_eq!(&got[..n], data);
    }
}

// ---- T27 leader check (2026-11-16): random metadata corruption never panics, hangs or leaks ----
#[test]
fn leader_corrupted_metadata_fuzz_fresh_seed() {
    let clean = build_test_disk(256, &[("/notes.txt", b"leader")]);
    let mut r = Rng::new(0x0127_1EAD_2711);
    for case in 0..16 {
        let mut disk = clean.clone();
        let span = 32 * 4096.min(disk.len() / 32); // the metadata end of the disk
        for _ in 0..1 + r.below(16) {
            let i = r.below(span);
            disk[i] ^= 1 << r.below(8);
        }
        let (exit, out, _) = boot_with_disk(disk);
        assert!(!out.contains("FAIL"), "case {case}: kernel failed:\n{out}");
        assert_eq!(exit, Exit::Pass, "case {case}:\n{out}");
        assert!(out.starts_with("Titan v0.1\n"), "case {case}:\n{out}");
    }
}
