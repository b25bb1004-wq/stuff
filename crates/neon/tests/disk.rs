//! Integration tests for Titan persistent Hades disk images.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use hades::{mkfs, Fs, Kind, MemDisk, MkfsOptions, BLOCK};
use neon::session::{default_disk, Pump, Titan};

const MAX_WAIT_STEPS: u64 = 100_000_000;
const CHUNK_STEPS: u64 = 500_000;

fn default_kernel_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("titan-kernel")
        .join("riscv64gc-unknown-none-elf")
        .join("release")
        .join("kernel")
}

fn load_kernel_or_skip() -> Option<Vec<u8>> {
    let path = default_kernel_path();
    match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            eprintln!("skipping kernel test (missing {}: {err})", path.display());
            None
        }
    }
}

fn pump_to_prompt(titan: &mut Titan) {
    let mut steps = 0u64;
    loop {
        match titan.pump(CHUNK_STEPS) {
            Pump::WaitingForInput => return,
            Pump::Exited(code) => panic!("unexpected guest exit ({code})"),
            Pump::Busy => {
                steps = steps.saturating_add(CHUNK_STEPS);
                assert!(steps < MAX_WAIT_STEPS, "guest timed out reaching prompt");
            }
        }
    }
}

#[test]
fn default_disk_mounts_and_contains_shell_marker() {
    let bytes = default_disk().unwrap();
    assert_eq!(bytes.len() % BLOCK, 0);
    let blocks = u64::try_from(bytes.len() / BLOCK).unwrap();
    let mut dev = MemDisk::new(blocks);
    dev.bytes_mut().copy_from_slice(&bytes);
    let mut fs = Fs::mount(dev).unwrap();
    let stat = fs.stat("/etc/shell").unwrap();
    assert_eq!(stat.kind, Kind::File);
}

#[test]
fn boot_with_disk_rejects_wrong_size_and_garbage() {
    let kernel_opt = fs::read(default_kernel_path()).ok();
    let elf: &[u8] = kernel_opt.as_deref().unwrap_or(&[]);

    assert!(Titan::boot_with_disk(elf, Vec::new()).is_err());
    assert!(Titan::boot_with_disk(elf, vec![0u8; 512]).is_err());
    assert!(Titan::boot_with_disk(elf, vec![0u8; BLOCK + 1]).is_err());
    assert!(Titan::boot_with_disk(elf, vec![0xAAu8; 256 * BLOCK]).is_err());

    let mut no_shell = MemDisk::new(256);
    mkfs(&mut no_shell, MkfsOptions::default()).unwrap();
    assert!(Titan::boot_with_disk(elf, no_shell.bytes().to_vec()).is_err());

    let mut dir_shell = MemDisk::new(256);
    mkfs(&mut dir_shell, MkfsOptions::default()).unwrap();
    let mut fs = Fs::mount(dir_shell).unwrap();
    fs.mkdir("/etc").unwrap();
    fs.mkdir("/etc/shell").unwrap();
    let dir_shell = fs.unmount().unwrap();
    assert!(Titan::boot_with_disk(elf, dir_shell.bytes().to_vec()).is_err());
}

#[test]
fn shell_write_persists_via_disk_bytes_and_boot_with_disk() {
    let Some(elf) = load_kernel_or_skip() else {
        return;
    };

    let mut session1 = Titan::boot(&elf).unwrap();
    pump_to_prompt(&mut session1);
    let _ = session1.take_output();

    session1.send("echo disk-persist-ok > /persist.txt");
    pump_to_prompt(&mut session1);
    let saved = session1.disk_bytes();

    let mut session2 = Titan::boot_with_disk(&elf, saved).unwrap();
    pump_to_prompt(&mut session2);
    let _ = session2.take_output();

    session2.send("cat /persist.txt");
    pump_to_prompt(&mut session2);
    let raw = session2.take_output();
    let output = String::from_utf8_lossy(&raw);
    assert!(
        output.contains("disk-persist-ok"),
        "expected persisted file output, got: {output}"
    );
}
