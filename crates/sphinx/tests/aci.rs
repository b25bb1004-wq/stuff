// Agent-interface tests for the sphinx binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_sphinx");
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sphinx_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_string()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn envelope(output: &Output) -> Json {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(stdout.matches('\n').count(), 1, "not one line: {stdout:?}");
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    value
}

fn data(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

fn fail(output: &Output, exit: i32, cmd: &str) -> String {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    value
        .get("error")
        .unwrap()
        .get("code")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("no {key} in {value:?}"))
}

fn text_field<'a>(value: &'a Json, key: &str) -> &'a str {
    field(value, key).as_str().unwrap()
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[test]
fn describe_is_a_valid_manifest() {
    let output = run(&["--describe"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let names: Vec<&str> = manifest
        .get("commands")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.get("name").unwrap().as_str().unwrap())
        .collect();
    let subcommands = ["hash", "hmac", "keygen", "sign", "verify", "seal", "open"];
    assert_eq!(names, subcommands);
    let help = String::from_utf8_lossy(&run(&["--help"]).stdout).into_owned();
    for name in subcommands {
        assert!(help.contains(&format!("  {name} ")), "{name} not in --help");
    }
}

#[test]
fn sha256_of_empty_input() {
    let t = Temp::new("hash");
    let empty = t.file("empty.txt", b"");
    let value = data(&run(&["--json", "hash", "sha256", &empty]));
    assert_eq!(text_field(&value, "hash"), EMPTY_SHA256);
    assert_eq!(text_field(&value, "algorithm"), "sha256");
    let out = run(&["hash", "sha256", &empty]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{EMPTY_SHA256}\n")
    );
}

#[test]
fn json_hash_rejects_unknown_algorithm() {
    let t = Temp::new("alg");
    let file = t.file("f.txt", b"x");
    assert_eq!(
        fail(&run(&["--json", "hash", "md5", &file]), 1, "hash"),
        "invalid"
    );
}

#[test]
fn json_hmac_is_hex() {
    let t = Temp::new("hmac");
    let file = t.file("msg.bin", b"hello");
    let value = data(&run(&["--json", "hmac", "4a656665", &file]));
    assert!(is_hex(text_field(&value, "mac"), 64));
}

#[test]
fn json_keygen_sign_and_verify_round_trip() {
    let t = Temp::new("sig");
    let key = t.path("id.key");
    let made = data(&run(&["--json", "keygen", "-o", &key]));
    let public = text_field(&made, "public_key").to_string();
    assert!(is_hex(&public, 64));
    assert_eq!(fs::read(&key).unwrap().len(), 32);

    let msg = t.file("msg.txt", b"attack at dawn");
    let signed = data(&run(&["--json", "sign", &key, &msg]));
    let sig = text_field(&signed, "signature").to_string();
    assert!(is_hex(&sig, 128));

    let ok = data(&run(&["--json", "verify", &public, &sig, &msg]));
    assert_eq!(field(&ok, "valid"), &Json::Bool(true));

    let changed = t.file("changed.txt", b"attack at dusk");
    let bad = run(&["--json", "verify", &public, &sig, &changed]);
    assert_eq!(bad.status.code(), Some(1));
    let value = envelope(&bad);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)));
    assert_eq!(field(field(&value, "data"), "valid"), &Json::Bool(false));
}

#[test]
fn json_verify_rejects_bad_hex() {
    let t = Temp::new("hex");
    let msg = t.file("m.txt", b"m");
    assert_eq!(
        fail(&run(&["--json", "verify", "zz", "00", &msg]), 1, "verify"),
        "invalid"
    );
}

#[test]
fn json_seal_and_open_round_trip() {
    let t = Temp::new("seal");
    let key = t.path("box.key");
    data(&run(&["--json", "keygen", "-o", &key]));
    let plain = t.file("secret.txt", b"the secret is 42");
    let sealed = t.path("secret.enc");
    let made = data(&run(&["--json", "seal", &key, &plain, "-o", &sealed]));
    assert_eq!(
        field(&made, "bytes").as_i64(),
        Some(fs::read(&sealed).unwrap().len() as i64)
    );

    let opened = t.path("opened.txt");
    let out = data(&run(&["--json", "open", &key, &sealed, "-o", &opened]));
    assert_eq!(fs::read(&opened).unwrap(), b"the secret is 42");
    assert_eq!(field(&out, "bytes").as_i64(), Some(16));

    let text = data(&run(&["--json", "open", &key, &sealed]));
    assert_eq!(text_field(&text, "plaintext"), "the secret is 42");
}

#[test]
fn json_open_binary_returns_hex() {
    let t = Temp::new("binary");
    let key = t.path("k.key");
    data(&run(&["--json", "keygen", "-o", &key]));
    let plain = t.file("bin.dat", &[0xff, 0x00, 0x80]);
    let sealed = t.path("bin.enc");
    data(&run(&["--json", "seal", &key, &plain, "-o", &sealed]));
    let value = data(&run(&["--json", "open", &key, &sealed]));
    assert_eq!(text_field(&value, "plaintext_hex"), "ff0080");
}

#[test]
fn json_open_tampered_is_invalid() {
    let t = Temp::new("tamper");
    let key = t.path("k.key");
    data(&run(&["--json", "keygen", "-o", &key]));
    let plain = t.file("p.txt", b"payload");
    let sealed = t.path("p.enc");
    data(&run(&["--json", "seal", &key, &plain, "-o", &sealed]));
    let mut bytes = fs::read(&sealed).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&sealed, bytes).unwrap();
    assert_eq!(
        fail(&run(&["--json", "open", &key, &sealed]), 1, "open"),
        "invalid"
    );
}

#[test]
fn json_seal_without_out_is_usage() {
    let t = Temp::new("usage");
    let key = t.path("k.key");
    data(&run(&["--json", "keygen", "-o", &key]));
    let plain = t.file("p.txt", b"x");
    let output = run(&["--json", "seal", &key, &plain]);
    assert_eq!(fail(&output, 2, "seal"), "usage");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let output = run(&["--json", "nosuch"]);
    assert_eq!(fail(&output, 2, ""), "usage");
    let value = envelope(&output);
    let hint = value.get("error").unwrap().get("hint").unwrap();
    assert_eq!(hint.as_str(), Some("run: sphinx --describe"));
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let key = t.path("id.key");
    let out = run(&["keygen", "-o", &key]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.ends_with('\n'));
    assert!(is_hex(text.trim_end(), 64), "{text}");
    let bad = run(&["verify", "00", "00", &key]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).starts_with("sphinx: invalid public key"));
}
