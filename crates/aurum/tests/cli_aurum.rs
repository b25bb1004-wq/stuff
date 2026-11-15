//! CLI integration tests for the `aurum` binary.
#![allow(clippy::unwrap_used)]

use assay::{hex, to_hex};
use aurum::{merkle_root, merkle_verify, Hash, Tx};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_aurum")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let id = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aurum_cli_test_{label}_{}_{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn run_with_stdin(args: &[&str], stdin_data: &[u8]) -> Output {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin_data).unwrap();
    child.wait_with_output().unwrap()
}

fn assert_err_exit(out: &Output, expected_code: i32) -> String {
    assert_eq!(out.status.code(), Some(expected_code));
    let stderr = String::from_utf8(out.stderr.clone()).unwrap();
    assert!(
        stderr.starts_with("aurum: "),
        "expected 'aurum: ' prefix, got: {stderr:?}"
    );
    stderr
}

fn path_str(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn help_and_version_and_unknown() {
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8(help.stdout).unwrap();
    for sub in ["keygen", "tx", "merkle", "verify", "demo"] {
        assert!(stdout.contains(sub), "help missing {sub}");
    }

    let ver = run(&["--version"]);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(String::from_utf8(ver.stdout).unwrap().trim(), "aurum 0.1.0");

    let unknown = run(&["bogus"]);
    assert_err_exit(&unknown, 2);

    let missing = run(&[]);
    assert_err_exit(&missing, 2);
}

#[test]
fn keygen_good_and_bad() {
    let dir = TempDir::new("keygen");
    let k1 = dir.path("alice.key");
    let k2 = dir.path("bob.key");

    let out1 = run(&["keygen", "-o", path_str(&k1)]);
    assert_eq!(out1.status.code(), Some(0));
    let addr1 = String::from_utf8(out1.stdout).unwrap();
    assert_eq!(hex(addr1.trim()).len(), 32);
    assert_eq!(hex(fs::read_to_string(&k1).unwrap().trim()).len(), 32);

    let out2 = run(&["keygen", "-o", path_str(&k2)]);
    assert_eq!(out2.status.code(), Some(0));
    let addr2 = String::from_utf8(out2.stdout).unwrap();
    assert_ne!(addr1.trim(), addr2.trim());

    let bad_path = dir.path("no_such_dir").join("k.key");
    let bad = run(&["keygen", "-o", path_str(&bad_path)]);
    assert_err_exit(&bad, 1);

    let usage = run(&["keygen"]);
    assert_err_exit(&usage, 2);
}

#[test]
fn tx_good_and_bad() {
    let dir = TempDir::new("tx");
    let alice_key = dir.path("alice.key");
    let bob_key = dir.path("bob.key");
    let tx_file = dir.path("transfer.tx");

    let alice_out = run(&["keygen", "-o", path_str(&alice_key)]);
    assert_eq!(alice_out.status.code(), Some(0));
    let alice_addr: [u8; 32] = hex(String::from_utf8(alice_out.stdout).unwrap().trim())
        .try_into()
        .unwrap();

    let bob_out = run(&["keygen", "-o", path_str(&bob_key)]);
    assert_eq!(bob_out.status.code(), Some(0));
    let bob_hex = String::from_utf8(bob_out.stdout).unwrap();
    let bob_addr: [u8; 32] = hex(bob_hex.trim()).try_into().unwrap();

    let out = run(&[
        "tx",
        path_str(&alice_key),
        bob_hex.trim(),
        "42",
        "7",
        "-o",
        path_str(&tx_file),
    ]);
    assert_eq!(out.status.code(), Some(0));
    let printed_id = String::from_utf8(out.stdout).unwrap();

    let raw = hex(&fs::read_to_string(&tx_file).unwrap());
    let decoded = Tx::decode(&raw).unwrap();
    assert!(decoded.verify());
    assert_eq!(decoded.from, alice_addr);
    assert_eq!(decoded.to, bob_addr);
    assert_eq!(decoded.amount, 42);
    assert_eq!(decoded.nonce, 7);
    assert_eq!(to_hex(&decoded.id()), printed_id.trim());

    let bad_addr = run(&[
        "tx",
        path_str(&alice_key),
        "deadbeef",
        "42",
        "7",
        "-o",
        path_str(&tx_file),
    ]);
    assert_err_exit(&bad_addr, 1);

    let bad_amount = run(&[
        "tx",
        path_str(&alice_key),
        bob_hex.trim(),
        "not_a_number",
        "7",
        "-o",
        path_str(&tx_file),
    ]);
    assert_err_exit(&bad_amount, 1);

    let bad_key = dir.path("bad.key");
    fs::write(&bad_key, "not_hex\n").unwrap();
    let bad_key_out = run(&[
        "tx",
        path_str(&bad_key),
        bob_hex.trim(),
        "1",
        "0",
        "-o",
        path_str(&tx_file),
    ]);
    assert_err_exit(&bad_key_out, 1);
}

#[test]
fn merkle_good_and_bad() {
    let dir = TempDir::new("merkle");
    let f0 = dir.path("a.bin");
    let f1 = dir.path("b.bin");
    let f2 = dir.path("c.bin");
    fs::write(&f0, b"alpha").unwrap();
    fs::write(&f1, b"beta").unwrap();
    fs::write(&f2, b"gamma").unwrap();

    let expected_root = merkle_root(&[b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()]);
    let out = run(&["merkle", path_str(&f0), path_str(&f1), path_str(&f2)]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        to_hex(&expected_root)
    );

    let proof_out = run(&[
        "merkle",
        "--proof",
        "1",
        path_str(&f0),
        path_str(&f1),
        path_str(&f2),
    ]);
    assert_eq!(proof_out.status.code(), Some(0));
    let text = String::from_utf8(proof_out.stdout).unwrap();
    let mut proof: Vec<Hash> = Vec::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("proof ") {
            proof.push(hex(h).try_into().unwrap());
        }
    }
    assert!(merkle_verify(&expected_root, b"beta", 1, 3, &proof));

    let stdin_out = run_with_stdin(&["merkle", "-"], b"delta");
    assert_eq!(stdin_out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(stdin_out.stdout).unwrap().trim(),
        to_hex(&merkle_root(&[b"delta".to_vec()]))
    );

    let bad_idx = run(&["merkle", "--proof", "9", path_str(&f0), path_str(&f1)]);
    assert_err_exit(&bad_idx, 1);

    let missing_file = dir.path("missing.bin");
    let bad_file = run(&["merkle", path_str(&missing_file)]);
    assert_err_exit(&bad_file, 1);
}

#[test]
fn demo_and_verify_good_and_bad() {
    let dir = TempDir::new("chain");
    let chain_file = dir.path("demo.chain");

    let demo_out = run(&["demo", "-o", path_str(&chain_file)]);
    assert_eq!(demo_out.status.code(), Some(0));
    let demo_txt = String::from_utf8(demo_out.stdout).unwrap();
    assert!(demo_txt.contains("height 3"));
    assert!(demo_txt.contains("blocks 4"));

    let verify_out = run(&["verify", path_str(&chain_file)]);
    assert_eq!(verify_out.status.code(), Some(0));
    let verify_txt = String::from_utf8(verify_out.stdout).unwrap();
    assert!(verify_txt.starts_with("ok\n"));
    assert!(verify_txt.contains("height 3"));
    assert!(verify_txt.contains("blocks 4"));

    // Pipe demo output via `-o -` into `verify -`.
    let piped_demo = run(&["demo", "-o", "-"]);
    assert_eq!(piped_demo.status.code(), Some(0));
    let piped_verify = run_with_stdin(&["verify", "-"], &piped_demo.stdout);
    assert_eq!(piped_verify.status.code(), Some(0));

    // Corrupt a byte inside the last block (near the tail) and expect verify to fail at block 3.
    let mut corrupted = fs::read(&chain_file).unwrap();
    let idx = corrupted.len() - 10;
    corrupted[idx] ^= 0xff;
    let bad_chain = dir.path("corrupted.chain");
    fs::write(&bad_chain, corrupted).unwrap();
    let verify_bad = run(&["verify", path_str(&bad_chain)]);
    let err = assert_err_exit(&verify_bad, 1);
    assert!(err.contains("block 3:"), "expected block 3 failure: {err}");

    // Bad header file.
    let garbage = dir.path("garbage.chain");
    fs::write(&garbage, b"not a chain").unwrap();
    assert_err_exit(&run(&["verify", path_str(&garbage)]), 1);

    // Bad demo output path.
    let bad_demo = dir.path("no_such_dir").join("demo.chain");
    assert_err_exit(&run(&["demo", "-o", path_str(&bad_demo)]), 1);
}
