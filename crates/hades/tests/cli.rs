//! CLI integration tests for the `hades` command-line tool.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_hades");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("hades_cli_{name}_{pid}_{id}"));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn assert_failed_with_prefix(out: &Output, code: i32) -> String {
    assert_eq!(
        out.status.code(),
        Some(code),
        "expected exit {code}, got {:?}, stdout={}, stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("hades:"),
        "expected stderr to contain 'hades:', got: {stderr:?}"
    );
    stderr
}

fn mkfs_img(dir: &TempDir, name: &str, blocks: &str) -> PathBuf {
    let img = dir.path(name);
    let img_str = img.to_str().unwrap();
    let out = run(&["mkfs", img_str, "--blocks", blocks]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "mkfs failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    img
}

fn read_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[test]
fn help_and_version() {
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let help_out = String::from_utf8_lossy(&help.stdout);
    for cmd in ["mkfs", "ls", "cat", "put", "rm", "mkdir", "fsck"] {
        assert!(help_out.contains(cmd), "--help missing {cmd}");
    }
    assert!(help_out.contains("Examples:"));

    let ver = run(&["--version"]);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ver.stdout).trim(), "hades 0.1.0");
}

#[test]
fn unknown_subcommand_and_usage_errors() {
    let unknown = run(&["bogus"]);
    assert_failed_with_prefix(&unknown, 2);

    let empty = run(&[]);
    assert_failed_with_prefix(&empty, 2);

    let bad_blocks = run(&["mkfs", "disk.img", "--blocks", "abc"]);
    assert_failed_with_prefix(&bad_blocks, 2);

    let missing_arg = run(&["ls", "disk.img"]);
    assert_failed_with_prefix(&missing_arg, 2);
}

#[test]
fn mkfs_good_and_bad() {
    let dir = TempDir::new("mkfs");
    let img = dir.path("custom.img");
    let out = run(&["mkfs", img.to_str().unwrap(), "--blocks", "128"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(read_file(&img).len(), 128 * hades::BLOCK);

    let default_img = dir.path("default.img");
    let out_def = run(&["mkfs", default_img.to_str().unwrap()]);
    assert_eq!(out_def.status.code(), Some(0));
    assert_eq!(read_file(&default_img).len(), 1024 * hades::BLOCK);

    let bad_img = dir.path("tiny.img");
    let bad = run(&["mkfs", bad_img.to_str().unwrap(), "--blocks", "4"]);
    let stderr = assert_failed_with_prefix(&bad, 2);
    assert!(
        stderr.contains("hades: mkfs: need at least 68 blocks"),
        "{stderr}"
    );
    assert!(!bad_img.exists());
}

#[test]
fn mkfs_minimum_is_exact() {
    let dir = TempDir::new("mkfs_min");
    let below = dir.path("below.img");
    let out = run(&["mkfs", below.to_str().unwrap(), "--blocks", "67"]);
    assert_failed_with_prefix(&out, 2);
    assert!(!below.exists());

    let at = dir.path("at.img");
    let out = run(&["mkfs", at.to_str().unwrap(), "--blocks", "68"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(read_file(&at).len(), 68 * hades::BLOCK);
    let check = run(&["fsck", at.to_str().unwrap()]);
    assert_eq!(check.status.code(), Some(0));
}

#[test]
fn mkdir_and_ls_good_and_bad() {
    let dir = TempDir::new("mkdir_ls");
    let img = mkfs_img(&dir, "disk.img", "128");
    let img_s = img.to_str().unwrap();

    let mk1 = run(&["mkdir", img_s, "/docs"]);
    assert_eq!(mk1.status.code(), Some(0));
    let mk2 = run(&["mkdir", img_s, "/docs/sub"]);
    assert_eq!(mk2.status.code(), Some(0));

    let ls_root = run(&["ls", img_s, "/"]);
    assert_eq!(ls_root.status.code(), Some(0));
    let root_text = String::from_utf8_lossy(&ls_root.stdout);
    assert!(root_text.contains("dir\t"));
    assert!(root_text.contains("docs"));

    let ls_sub = run(&["ls", img_s, "/docs"]);
    assert_eq!(ls_sub.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&ls_sub.stdout).contains("sub"));

    // Bad mkdir: already exists
    let dup = run(&["mkdir", img_s, "/docs"]);
    assert_failed_with_prefix(&dup, 1);

    // Bad ls: missing path
    let missing = run(&["ls", img_s, "/nope"]);
    assert_failed_with_prefix(&missing, 1);
}

#[test]
fn put_cat_and_rm_good_and_bad() {
    let dir = TempDir::new("put_cat_rm");
    let img = mkfs_img(&dir, "disk.img", "256");
    let img_s = img.to_str().unwrap();

    assert_eq!(run(&["mkdir", img_s, "/data"]).status.code(), Some(0));

    // Multi-chunk binary payload (> 32 KiB)
    let payload: Vec<u8> = (0..45_000usize).map(|i| (i % 251) as u8).collect();
    let src = dir.path("blob.bin");
    fs::write(&src, &payload).unwrap();

    let put_ok = run(&["put", img_s, src.to_str().unwrap(), "/data/blob.bin"]);
    assert_eq!(
        put_ok.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&put_ok.stderr)
    );

    let cat_ok = run(&["cat", img_s, "/data/blob.bin"]);
    assert_eq!(cat_ok.status.code(), Some(0));
    assert_eq!(cat_ok.stdout, payload);

    // Stdin put + overwrite existing file with shorter content
    let shorter = b"replaced via stdin\n";
    let put_stdin = run_with_stdin(&["put", img_s, "-", "/data/blob.bin"], shorter);
    assert_eq!(put_stdin.status.code(), Some(0));

    let cat_short = run(&["cat", img_s, "/data/blob.bin"]);
    assert_eq!(cat_short.status.code(), Some(0));
    assert_eq!(cat_short.stdout, shorter);

    // Bad put: missing host source file
    let bad_put = run(&[
        "put",
        img_s,
        dir.path("missing.txt").to_str().unwrap(),
        "/a",
    ]);
    assert_failed_with_prefix(&bad_put, 1);

    // Bad cat: directory path
    let bad_cat = run(&["cat", img_s, "/data"]);
    assert_failed_with_prefix(&bad_cat, 1);

    // Good rm
    let rm_ok = run(&["rm", img_s, "/data/blob.bin"]);
    assert_eq!(rm_ok.status.code(), Some(0));
    let cat_after_rm = run(&["cat", img_s, "/data/blob.bin"]);
    assert_failed_with_prefix(&cat_after_rm, 1);

    // Bad rm: missing path and directory path
    let bad_rm_missing = run(&["rm", img_s, "/data/blob.bin"]);
    assert_failed_with_prefix(&bad_rm_missing, 1);
    let bad_rm_dir = run(&["rm", img_s, "/data"]);
    assert_failed_with_prefix(&bad_rm_dir, 1);
}

#[test]
fn fsck_good_and_bad() {
    let dir = TempDir::new("fsck");
    let img = mkfs_img(&dir, "disk.img", "128");
    let img_s = img.to_str().unwrap();

    let src = dir.path("note.txt");
    fs::write(&src, b"hello hades\n").unwrap();
    assert_eq!(
        run(&["put", img_s, src.to_str().unwrap(), "/note.txt"])
            .status
            .code(),
        Some(0)
    );

    let ok = run(&["fsck", img_s]);
    assert_eq!(ok.status.code(), Some(0));

    // Corrupt a bit in the block bitmap (block 65 = after sb + 64 journal blocks)
    let mut bytes = read_file(&img);
    let bitmap_byte = 65 * hades::BLOCK + 12;
    bytes[bitmap_byte] ^= 0x80;
    fs::write(&img, &bytes).unwrap();

    let bad_bitmap = run(&["fsck", img_s]);
    let stderr = assert_failed_with_prefix(&bad_bitmap, 1);
    assert!(stderr.contains("bitmap"));

    // Corrupt superblock magic
    bytes[0] ^= 0xff;
    fs::write(&img, &bytes).unwrap();
    let bad_sb = run(&["fsck", img_s]);
    assert_failed_with_prefix(&bad_sb, 1);
}
