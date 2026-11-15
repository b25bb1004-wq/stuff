//! CLI integration tests for the sphinx binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sphinx")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sphinx_cli_{}_{}_{}",
            std::process::id(),
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
        let p = self.path.join(name);
        fs::write(&p, contents).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run(args: &[&str], stdin_data: Option<&[u8]>) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    if let Some(data) = stdin_data {
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(data).unwrap();
        child.wait_with_output().unwrap()
    } else {
        cmd.output().unwrap()
    }
}

#[test]
fn help_and_version() {
    let out = run(&["--help"], None);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for verb in ["hash", "hmac", "keygen", "sign", "verify", "seal", "open"] {
        assert!(stdout.contains(verb), "help missing {verb}");
    }

    let ver = run(&["--version"], None);
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(ver.stdout).unwrap().trim(),
        "sphinx 0.1.0"
    );
}

#[test]
fn unknown_subcommand_exits_2() {
    let out = run(&["bogus"], None);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("sphinx: "), "stderr was: {err}");
}

#[test]
fn hash_known_answers_and_bad_input() {
    let dir = TempDir::new("hash");
    let abc = dir.file("abc.txt", b"abc");

    let out256 = run(&["hash", "sha256", abc.to_str().unwrap()], None);
    assert_eq!(out256.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out256.stdout).unwrap().trim(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    let out512 = run(&["hash", "sha512", "-"], Some(b"abc"));
    assert_eq!(out512.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out512.stdout).unwrap().trim(),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );

    let missing = dir.path.join("does_not_exist");
    let bad = run(&["hash", "sha256", missing.to_str().unwrap()], None);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8(bad.stderr)
        .unwrap()
        .starts_with("sphinx: "));
}

#[test]
fn hmac_known_answer_and_bad_input() {
    let dir = TempDir::new("hmac");
    let msg = dir.file("msg.bin", b"what do ya want for nothing?");
    // RFC 4231 test case 2: key = "Jefe" (0x4a656665)
    let out = run(&["hmac", "4a656665", msg.to_str().unwrap()], None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );

    let bad = run(&["hmac", "zz", msg.to_str().unwrap()], None);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8(bad.stderr)
        .unwrap()
        .starts_with("sphinx: "));
}

#[test]
fn keygen_sign_verify_round_trip_and_rfc8032() {
    let dir = TempDir::new("ed25519");
    let key_path = dir.path.join("id.key");
    let msg_path = dir.file("msg.txt", b"hello monolith");

    let kg = run(&["keygen", "-o", key_path.to_str().unwrap()], None);
    assert_eq!(kg.status.code(), Some(0));
    let pub_hex = String::from_utf8(kg.stdout).unwrap().trim().to_string();
    assert_eq!(pub_hex.len(), 64);
    assert_eq!(fs::read(&key_path).unwrap().len(), 32);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let sig_out = run(
        &[
            "sign",
            key_path.to_str().unwrap(),
            msg_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(sig_out.status.code(), Some(0));
    let sig_hex = String::from_utf8(sig_out.stdout)
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(sig_hex.len(), 128);

    let ver_ok = run(
        &["verify", &pub_hex, &sig_hex, msg_path.to_str().unwrap()],
        None,
    );
    assert_eq!(ver_ok.status.code(), Some(0));

    let bad_msg = dir.file("bad.txt", b"hello monolith!");
    let ver_bad = run(
        &["verify", &pub_hex, &sig_hex, bad_msg.to_str().unwrap()],
        None,
    );
    assert_eq!(ver_bad.status.code(), Some(1));
    assert!(String::from_utf8(ver_bad.stderr)
        .unwrap()
        .starts_with("sphinx: "));

    // RFC 8032 test vector 1: empty message
    let rfc_seed = assay::hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
    let rfc_key = dir.file("rfc1.key", &rfc_seed);
    let empty = dir.file("empty.bin", b"");
    let rfc_sig = run(
        &["sign", rfc_key.to_str().unwrap(), empty.to_str().unwrap()],
        None,
    );
    assert_eq!(rfc_sig.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(rfc_sig.stdout).unwrap().trim(),
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );

    // Bad inputs for keygen and sign
    let bad_kg = run(&["keygen"], None);
    assert_eq!(bad_kg.status.code(), Some(2));
    assert!(String::from_utf8(bad_kg.stderr)
        .unwrap()
        .starts_with("sphinx: "));

    let short_key = dir.file("short.key", b"too short");
    let bad_sign = run(
        &[
            "sign",
            short_key.to_str().unwrap(),
            msg_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(bad_sign.status.code(), Some(1));
    assert!(String::from_utf8(bad_sign.stderr)
        .unwrap()
        .starts_with("sphinx: "));
}

#[test]
fn seal_and_open_round_trip_and_bad_input() {
    let dir = TempDir::new("aead");
    let key = dir.file("aead.key", &[0x42u8; 32]);
    let plain = b"secret message for sphinx aead";
    let plain_path = dir.file("plain.txt", plain);
    let sealed_path = dir.path.join("sealed.bin");

    let s1 = run(
        &[
            "seal",
            key.to_str().unwrap(),
            plain_path.to_str().unwrap(),
            "-o",
            sealed_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(s1.status.code(), Some(0));

    let s2 = run(
        &["seal", key.to_str().unwrap(), plain_path.to_str().unwrap()],
        None,
    );
    assert_eq!(s2.status.code(), Some(0));
    let sealed_bytes = fs::read(&sealed_path).unwrap();
    assert_ne!(sealed_bytes, s2.stdout);

    let opened = run(
        &["open", key.to_str().unwrap(), sealed_path.to_str().unwrap()],
        None,
    );
    assert_eq!(opened.status.code(), Some(0));
    assert_eq!(opened.stdout, plain);

    // Pipe via stdin
    let opened_stdin = run(&["open", key.to_str().unwrap(), "-"], Some(&s2.stdout));
    assert_eq!(opened_stdin.status.code(), Some(0));
    assert_eq!(opened_stdin.stdout, plain);

    // Tampered ciphertext fails with exit 1
    let mut tampered = sealed_bytes;
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    let tampered_path = dir.file("tampered.bin", &tampered);
    let bad_open = run(
        &[
            "open",
            key.to_str().unwrap(),
            tampered_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(bad_open.status.code(), Some(1));
    assert!(String::from_utf8(bad_open.stderr)
        .unwrap()
        .starts_with("sphinx: "));

    // Short key fails seal with exit 1
    let bad_key = dir.file("bad.key", &[1u8; 16]);
    let bad_seal = run(
        &[
            "seal",
            bad_key.to_str().unwrap(),
            plain_path.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(bad_seal.status.code(), Some(1));
    assert!(String::from_utf8(bad_seal.stderr)
        .unwrap()
        .starts_with("sphinx: "));
}
