// Agent-interface tests for the xenon binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_xenon");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("xenon_aci_{name}_{}_{id}", std::process::id()));
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

/// Kills the child when the test ends, even on failure.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The path with forward slashes, as the envelope spells every path.
fn fwd(path: &Path) -> String {
    s(path).replace('\\', "/")
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

/// The error code and hint of a failed envelope, checking the exit code and the command name.
fn fail(output: &Output, exit: i32, cmd: &str) -> (String, Option<String>) {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    let value = envelope(output);
    assert_eq!(value.get("ok"), Some(&Json::Bool(false)), "{value:?}");
    assert_eq!(value.get("cmd").and_then(Json::as_str), Some(cmd));
    let error = value.get("error").unwrap();
    let code = error.get("code").unwrap().as_str().unwrap().to_string();
    let hint = error.get("hint").and_then(Json::as_str).map(String::from);
    (code, hint)
}

fn field<'a>(value: &'a Json, key: &str) -> &'a Json {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {value:?}"))
}

fn text_of<'a>(value: &'a Json, key: &str) -> &'a str {
    field(value, key).as_str().unwrap()
}

/// A fresh key pair from `xenon --json keygen`: (secret_key, public_key).
fn keypair() -> (String, String) {
    let d = data(&run(&["--json", "keygen"]));
    (
        text_of(&d, "secret_key").to_string(),
        text_of(&d, "public_key").to_string(),
    )
}

/// Start `xenon --json echo-server 127.0.0.1:0 SECRET`; return the guard and the bound address.
fn start_echo_server(secret: &str) -> (Guard, String) {
    let mut child = Command::new(BIN)
        .args(["--json", "echo-server", "127.0.0.1:0", secret])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let stdout = child.stdout.take().unwrap();
    BufReader::new(stdout).read_line(&mut line).unwrap();
    let guard = Guard(child);
    let value = Json::parse(line.trim_end()).unwrap();
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{line}");
    let listen = text_of(value.get("data").unwrap(), "listen").to_string();
    (guard, listen)
}

/// A UDP address on 127.0.0.1 that nothing is using right now.
fn free_udp_addr() -> String {
    let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
    probe.local_addr().unwrap().to_string()
}

#[test]
fn describe_is_a_valid_manifest() {
    let out = run(&["--describe"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let manifest = Json::parse(text.trim_end()).unwrap();
    assert_eq!(aci::manifest::validate_manifest(&manifest), Ok(()));
    let commands = field(&manifest, "commands").as_array().unwrap();
    let mut names: Vec<&str> = commands
        .iter()
        .map(|c| field(c, "name").as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["echo", "echo-server", "keygen", "recv", "send"]);
    for c in commands {
        let expect = match field(c, "name").as_str().unwrap() {
            "keygen" => "read",
            _ => "net",
        };
        assert_eq!(field(c, "effect").as_str(), Some(expect), "{c:?}");
    }
}

#[test]
fn json_keygen_returns_two_hex_keys() {
    let d = data(&run(&["--json", "keygen"]));
    for key in ["secret_key", "public_key"] {
        let hex = text_of(&d, key);
        assert_eq!(hex.len(), 64, "{key}: {hex}");
        assert!(hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }
}

#[test]
fn json_echo_round_trip() {
    let (secret, public) = keypair();
    let (_server, listen) = start_echo_server(&secret);
    let d = data(&run(&["--json", "echo", &listen, &public, "hello agent"]));
    assert_eq!(text_of(&d, "text"), "hello agent");
}

#[test]
fn json_send_and_recv_transfer_a_file() {
    let t = Temp::new("transfer");
    let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    let src = t.join("in file.bin");
    fs::write(&src, &payload).unwrap();
    let (secret, public) = keypair();
    let addr = free_udp_addr();
    let dest = t.join("out dir").join("received.bin");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();

    // recv returns once the file has arrived, so its envelope is read with wait_with_output.
    let receiver = Command::new(BIN)
        .args(["--json", "recv", &addr, &secret, s(&dest)])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(300));

    let sent = data(&run(&["--json", "send", &addr, &public, s(&src)]));
    assert_eq!(field(&sent, "bytes"), &Json::Int(5000));
    assert_eq!(text_of(&sent, "path"), fwd(&src));
    let sent_hash = text_of(&sent, "hash").to_string();
    assert_eq!(sent_hash.len(), 64);
    assert!(field(&sent, "duration_ms").as_i64().is_some());

    let got = data(&receiver.wait_with_output().unwrap());
    assert_eq!(field(&got, "bytes"), &Json::Int(5000));
    assert_eq!(text_of(&got, "path"), fwd(&dest));
    assert_eq!(text_of(&got, "hash"), sent_hash);
    assert_eq!(fs::read(&dest).unwrap(), payload);
}

#[test]
fn json_bad_input_is_an_error_envelope() {
    let (code, _) = fail(
        &run(&["--json", "echo", "127.0.0.1:9", "not-hex", "x"]),
        1,
        "echo",
    );
    assert_eq!(code, "invalid");
    let (code, _) = fail(
        &run(&[
            "--json",
            "send",
            "127.0.0.1:9",
            &"0".repeat(64),
            "/no/such/file.bin",
        ]),
        1,
        "send",
    );
    assert_eq!(code, "not_found");
    let (secret, _) = keypair();
    let taken = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = taken.local_addr().unwrap().to_string();
    let (code, _) = fail(
        &run(&["--json", "echo-server", &addr, &secret]),
        1,
        "echo-server",
    );
    assert_eq!(code, "io");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let (code, hint) = fail(&run(&["--json", "nosuch"]), 2, "");
    assert_eq!(code, "usage");
    assert_eq!(hint.as_deref(), Some("run: xenon --describe"));
    let (code, _) = fail(&run(&["--json", "echo", "127.0.0.1:9"]), 2, "echo");
    assert_eq!(code, "usage");
}

#[test]
fn text_mode_is_unchanged() {
    let out = run(&["keygen"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.starts_with("secret: "), "{stdout}");
    assert!(stdout.contains("\npublic: "), "{stdout}");
    let ver = run(&["--version"]);
    assert_eq!(String::from_utf8_lossy(&ver.stdout), "xenon 0.1.0\n");
}
