// Agent-interface tests for the aurum binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_aurum");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("aurum_aci_{name}_{}_{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
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

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Parse stdout as exactly one envelope and validate it.
fn envelope(output: &Output) -> Json {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.ends_with('\n'), "no newline: {stdout:?}");
    assert_eq!(stdout.matches('\n').count(), 1, "not one line: {stdout:?}");
    let value = Json::parse(stdout.trim_end()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{stdout}");
    value
}

/// The data of a successful envelope; checks the exit code is 0.
fn data(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(true)), "{value:?}");
    value.get("data").unwrap().clone()
}

/// The error code of a failed envelope, checking the exit code and that the command name is `cmd`.
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

fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A new keyfile through --json; returns its path and address.
fn keyfile(t: &Temp, name: &str) -> (PathBuf, String) {
    let path = t.join(name);
    let value = data(&run(&["--json", "keygen", "-o", s(&path)]));
    let address = field(&value, "address").as_str().unwrap().to_string();
    (path, address)
}

#[test]
fn describe_is_a_valid_manifest() {
    let out = run(&["--describe"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let names: Vec<&str> = field(&manifest, "commands")
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.get("name").and_then(Json::as_str).unwrap())
        .collect();
    assert_eq!(names, ["keygen", "tx", "merkle", "verify", "demo"]);
}

#[test]
fn json_keygen_writes_key_and_reports_address() {
    let t = Temp::new("keygen");
    let path = t.join("alice.key");
    let value = data(&run(&["--json", "keygen", "-o", s(&path)]));
    let address = field(&value, "address").as_str().unwrap();
    assert!(is_hex64(address), "{address}");
    assert!(field(&value, "path")
        .as_str()
        .unwrap()
        .ends_with("alice.key"));
    // The seed goes to the file only, never into the envelope.
    assert!(value.get("seed").is_none());
    let key = fs::read_to_string(&path).unwrap();
    assert!(is_hex64(key.trim()), "{key}");
}

#[test]
fn json_tx_signs_a_transfer() {
    let t = Temp::new("tx");
    let (key, _) = keyfile(&t, "alice.key");
    let (_, bob) = keyfile(&t, "bob.key");
    let out = t.join("transfer.tx");
    let value = data(&run(&[
        "--json",
        "tx",
        s(&key),
        &bob,
        "100",
        "0",
        "-o",
        s(&out),
    ]));
    assert!(is_hex64(field(&value, "hash").as_str().unwrap()));
    assert!(field(&value, "bytes").as_i64().unwrap() > 0);
    assert!(!fs::read_to_string(&out).unwrap().trim().is_empty());
}

#[test]
fn json_merkle_root_and_proof() {
    let t = Temp::new("merkle");
    let a = t.join("a.bin");
    let b = t.join("b.bin");
    fs::write(&a, b"alpha").unwrap();
    fs::write(&b, b"beta").unwrap();
    let root = data(&run(&["--json", "merkle", s(&a), s(&b)]));
    let root_hex = field(&root, "root").as_str().unwrap().to_string();
    assert!(is_hex64(&root_hex));
    assert_eq!(field(&root, "count"), &Json::Int(2));

    let proof = data(&run(&["--json", "merkle", "--proof", "0", s(&a), s(&b)]));
    assert_eq!(field(&proof, "root").as_str(), Some(root_hex.as_str()));
    assert_eq!(field(&proof, "index"), &Json::Int(0));
    let page = field(&proof, "proof");
    assert_eq!(field(page, "more"), &Json::Bool(false));
    let items = field(page, "items").as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(is_hex64(items[0].as_str().unwrap()));
}

#[test]
fn json_demo_and_verify_agree() {
    let t = Temp::new("chain");
    let chain = t.join("demo.chain");
    let demo = data(&run(&["--json", "demo", "-o", s(&chain)]));
    assert_eq!(field(&demo, "height"), &Json::Int(3));
    assert!(is_hex64(field(&demo, "hash").as_str().unwrap()));
    let verified = data(&run(&["--json", "verify", s(&chain)]));
    assert_eq!(field(&verified, "hash"), field(&demo, "hash"));
    assert_eq!(field(&verified, "root"), field(&demo, "root"));
    assert_eq!(field(&verified, "height"), &Json::Int(3));
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let t = Temp::new("bad");
    let missing = t.join("missing.chain");
    assert_eq!(
        fail(&run(&["--json", "verify", s(&missing)]), 1, "verify"),
        "not_found"
    );
    let garbage = t.join("garbage.chain");
    fs::write(&garbage, b"not a chain").unwrap();
    assert_eq!(
        fail(&run(&["--json", "verify", s(&garbage)]), 1, "verify"),
        "invalid"
    );
    assert_eq!(
        fail(&run(&["--json", "keygen", "-o", "-"]), 2, "keygen"),
        "usage"
    );
    assert_eq!(
        fail(&run(&["--json", "demo", "-o", "-"]), 2, "demo"),
        "usage"
    );
    let a = t.join("a.bin");
    fs::write(&a, b"alpha").unwrap();
    assert_eq!(
        fail(
            &run(&["--json", "merkle", "--proof", "9", s(&a)]),
            1,
            "merkle"
        ),
        "invalid"
    );
    let (key, _) = keyfile(&t, "alice.key");
    let bad_to = run(&[
        "--json",
        "tx",
        s(&key),
        "zz",
        "1",
        "0",
        "-o",
        s(&t.join("x.tx")),
    ]);
    assert_eq!(fail(&bad_to, 1, "tx"), "invalid");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let out = run(&["--json", "nosuch"]);
    assert_eq!(out.status.code(), Some(2));
    let value = envelope(&out);
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(""));
    assert_eq!(
        value
            .get("error")
            .unwrap()
            .get("code")
            .and_then(Json::as_str),
        Some("usage")
    );
}

#[test]
fn json_path_with_spaces_and_backslashes() {
    let t = Temp::new("spaces");
    let dir = t.join("a b");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bob.key");
    let value = data(&run(&["--json", "keygen", "-o", s(&path)]));
    let shown = field(&value, "path").as_str().unwrap();
    assert!(!shown.contains('\\'), "{shown}");
    assert!(shown.ends_with("a b/bob.key"), "{shown}");
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let a = t.join("a.bin");
    let b = t.join("b.bin");
    fs::write(&a, b"alpha").unwrap();
    fs::write(&b, b"beta").unwrap();
    let text = run(&["merkle", s(&a), s(&b)]);
    assert_eq!(text.status.code(), Some(0));
    let root = String::from_utf8_lossy(&text.stdout).trim().to_string();
    let json = data(&run(&["--json", "merkle", s(&a), s(&b)]));
    assert_eq!(field(&json, "root").as_str(), Some(root.as_str()));

    let chain = t.join("demo.chain");
    let demo = run(&["demo", "-o", s(&chain)]);
    let lines = String::from_utf8_lossy(&demo.stdout).into_owned();
    assert!(lines.starts_with("height 3\n"), "{lines}");
}
