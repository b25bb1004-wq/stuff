// Agent-interface tests for the mercury binary: --describe and --json (docs/ai-computer/ACI.md).
#![allow(missing_docs, clippy::unwrap_used)]

use aci::json::Json;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_mercury");
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("mercury_aci_{name}_{}_{id}", std::process::id()));
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

/// Kills the serving child when the test ends, even on failure.
struct Server(Child);

impl Drop for Server {
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

/// Start `mercury --json serve DIR --listen 127.0.0.1:0`, read the first line and return the bound address.
fn start_server(dir: &Path) -> (Server, String) {
    let mut child = Command::new(BIN)
        .args(["--json", "serve", s(dir), "--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let stdout = child.stdout.take().unwrap();
    BufReader::new(stdout).read_line(&mut line).unwrap();
    let server = Server(child);
    let value = Json::parse(line.trim_end()).unwrap();
    assert_eq!(aci::envelope::validate_envelope(&value), Ok(()), "{line}");
    let listen = field(value.get("data").unwrap(), "listen")
        .as_str()
        .unwrap()
        .to_string();
    (server, listen)
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
    assert_eq!(names, ["get", "parse", "serve"]);
    for c in commands {
        let expect = match field(c, "name").as_str().unwrap() {
            "parse" => "read",
            _ => "net",
        };
        assert_eq!(field(c, "effect").as_str(), Some(expect), "{c:?}");
    }
}

#[test]
fn json_get_fetches_a_served_file() {
    let t = Temp::new("serve");
    fs::write(t.join("file.txt"), b"hello from mercury").unwrap();
    let (_server, listen) = start_server(&t.0);
    let d = data(&run(&[
        "--json",
        "get",
        &format!("http://{listen}/file.txt"),
    ]));
    assert_eq!(field(&d, "status"), &Json::Int(200));
    let headers = field(&d, "headers").as_array().unwrap();
    let has_length = headers.iter().any(|h| {
        field(h, "name")
            .as_str()
            .unwrap()
            .eq_ignore_ascii_case("content-length")
    });
    assert!(has_length, "no content-length: {headers:?}");
    assert_eq!(field(&d, "text").as_str(), Some("hello from mercury"));
}

#[test]
fn json_get_missing_file_is_status_404_and_ok() {
    let t = Temp::new("missing");
    let (_server, listen) = start_server(&t.0);
    let d = data(&run(&[
        "--json",
        "get",
        &format!("http://{listen}/nope.txt"),
    ]));
    assert_eq!(field(&d, "status"), &Json::Int(404));
}

#[test]
fn json_get_large_body_is_capped() {
    let t = Temp::new("big");
    fs::write(t.join("big.txt"), vec![b'x'; 20000]).unwrap();
    let (_server, listen) = start_server(&t.0);
    let d = data(&run(&[
        "--json",
        "get",
        &format!("http://{listen}/big.txt"),
    ]));
    assert_eq!(field(&d, "text").as_str().unwrap().len(), 16384);
    assert_eq!(field(&d, "truncated"), &Json::Bool(true));
    assert_eq!(field(&d, "bytes"), &Json::Int(20000));
}

#[test]
fn json_get_out_writes_the_body_to_a_file() {
    let t = Temp::new("out");
    fs::write(t.join("file.txt"), b"body bytes").unwrap();
    let (_server, listen) = start_server(&t.0);
    let dest = t.join("saved copy.txt");
    let url = format!("http://{listen}/file.txt");
    let d = data(&run(&["--json", "get", &url, "--out", s(&dest)]));
    assert_eq!(field(&d, "path").as_str(), Some(fwd(&dest).as_str()));
    assert!(!fwd(&dest).contains('\\'));
    assert_eq!(field(&d, "bytes"), &Json::Int(10));
    assert_eq!(fs::read(&dest).unwrap(), b"body bytes");
    assert!(d.get("text").is_none());
}

#[test]
fn json_get_refused_connection_is_io_with_address_hint() {
    let (code, hint) = fail(&run(&["--json", "get", "http://127.0.0.1:1/"]), 1, "get");
    assert_eq!(code, "io");
    assert!(hint.unwrap().contains("127.0.0.1:1"));
}

#[test]
fn json_get_https_is_unsupported() {
    let (code, hint) = fail(
        &run(&["--json", "get", "https://example.invalid/"]),
        1,
        "get",
    );
    assert_eq!(code, "unsupported");
    assert_eq!(
        hint.as_deref(),
        Some("https is not built yet; use an http:// URL")
    );
}

#[test]
fn json_get_malformed_url_is_invalid() {
    for url in ["ftp://example.com/", "http:///path", "http://h:0/"] {
        let (code, _) = fail(&run(&["--json", "get", url]), 1, "get");
        assert_eq!(code, "invalid", "{url}");
    }
}

#[test]
fn json_parse_request_and_response() {
    let t = Temp::new("parse");
    let req = t.join("request.http");
    fs::write(
        &req,
        b"GET /hello?x=1 HTTP/1.1\r\nHost: example.com\r\n\r\n",
    )
    .unwrap();
    let d = data(&run(&["--json", "parse", s(&req)]));
    assert_eq!(field(&d, "kind").as_str(), Some("request"));
    assert_eq!(field(&d, "method").as_str(), Some("GET"));
    assert_eq!(field(&d, "path").as_str(), Some("/hello?x=1"));
    assert_eq!(field(&d, "status"), &Json::Null);
    assert_eq!(field(&d, "version").as_str(), Some("HTTP/1.1"));
    let first = &field(&d, "headers").as_array().unwrap()[0];
    assert_eq!(field(first, "name").as_str(), Some("Host"));
    assert_eq!(field(first, "text").as_str(), Some("example.com"));

    let resp = t.join("response.http");
    fs::write(&resp, b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
    let d = data(&run(&["--json", "parse", s(&resp)]));
    assert_eq!(field(&d, "kind").as_str(), Some("response"));
    assert_eq!(field(&d, "method"), &Json::Null);
    assert_eq!(field(&d, "path"), &Json::Null);
    assert_eq!(field(&d, "status"), &Json::Int(200));
    assert_eq!(field(&d, "text").as_str(), Some("ok"));
}

#[test]
fn json_parse_bad_input_is_invalid_and_missing_file_is_not_found() {
    let t = Temp::new("bad");
    let bad = t.join("bad.http");
    fs::write(&bad, b"!!! not http\r\n\r\n").unwrap();
    let (code, _) = fail(&run(&["--json", "parse", s(&bad)]), 1, "parse");
    assert_eq!(code, "invalid");
    let (code, _) = fail(
        &run(&["--json", "parse", s(&t.join("gone.http"))]),
        1,
        "parse",
    );
    assert_eq!(code, "not_found");
}

#[test]
fn json_serve_bind_failure_is_io() {
    let t = Temp::new("busy");
    let hold = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = hold.local_addr().unwrap().to_string();
    let out = run(&["--json", "serve", s(&t.0), "--listen", &addr]);
    let (code, _) = fail(&out, 1, "serve");
    assert_eq!(code, "io");
}

#[test]
fn json_unknown_subcommand_is_usage() {
    let (code, _) = fail(&run(&["--json", "nosuch"]), 2, "");
    assert_eq!(code, "usage");
    let (code, _) = fail(&run(&["--json", "get"]), 2, "get");
    assert_eq!(code, "usage");
}

#[test]
fn text_mode_is_unchanged() {
    let t = Temp::new("text");
    let req = t.join("request.http");
    fs::write(&req, b"GET / HTTP/1.1\r\nHost: a\r\n\r\n").unwrap();
    let out = run(&["parse", s(&req)]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.starts_with("kind: request\nmethod: GET\n"),
        "{stdout}"
    );
    let plain = run(&["get", "http://127.0.0.1:1/"]);
    assert_eq!(plain.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&plain.stderr).starts_with("mercury: connect failed"));
}
