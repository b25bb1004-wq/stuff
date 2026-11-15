//! CLI integration tests for the `mercury` binary.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mercury")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mercury_cli_test_{prefix}_{}_{}",
            std::process::id(),
            id
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn help_and_version() {
    let help = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help_out = String::from_utf8_lossy(&help.stdout);
    assert!(help_out.contains("parse"), "help missing parse: {help_out}");
    assert!(help_out.contains("serve"), "help missing serve: {help_out}");
    assert!(help_out.contains("get"), "help missing get: {help_out}");
    assert!(
        help_out.contains("EXAMPLES:"),
        "help missing examples: {help_out}"
    );

    let ver = Command::new(bin()).arg("--version").output().unwrap();
    assert_eq!(ver.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ver.stdout).trim(), "mercury 0.1.0");
}

#[test]
fn unknown_subcommand_and_usage_errors() {
    let unknown = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    let err = String::from_utf8_lossy(&unknown.stderr);
    assert!(err.contains("mercury:"), "stderr: {err}");

    let empty = Command::new(bin()).output().unwrap();
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("mercury:"));

    for sub in ["parse", "serve", "get"] {
        let out = Command::new(bin()).arg(sub).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "subcommand {sub}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("mercury:"));
    }
}

#[test]
fn parse_good_request_response_and_stdin() {
    let dir = TempDir::new("parse_good");

    let req_path = dir.path.join("req.http");
    fs::write(
        &req_path,
        b"GET /hello?x=1 HTTP/1.1\r\nHost: example.com\r\nContent-Length: 5\r\n\r\nworld",
    )
    .unwrap();
    let out = Command::new(bin())
        .args(["parse", req_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("kind: request"), "stdout: {stdout}");
    assert!(stdout.contains("method: GET"), "stdout: {stdout}");
    assert!(stdout.contains("target: /hello?x=1"), "stdout: {stdout}");
    assert!(
        stdout.contains("header: Host: example.com"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("body-len: 5"), "stdout: {stdout}");
    assert!(stdout.contains("body: world"), "stdout: {stdout}");

    let resp_path = dir.path.join("resp.http");
    fs::write(
        &resp_path,
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
    )
    .unwrap();
    let out = Command::new(bin())
        .args(["parse", resp_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("kind: response"), "stdout: {stdout}");
    assert!(stdout.contains("status: 200"), "stdout: {stdout}");
    assert!(stdout.contains("reason: OK"), "stdout: {stdout}");
    assert!(stdout.contains("body: ok"), "stdout: {stdout}");

    let mut child = Command::new(bin())
        .args(["parse", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"POST /submit HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("method: POST"), "stdout: {stdout}");
    assert!(stdout.contains("target: /submit"), "stdout: {stdout}");
}

#[test]
fn parse_bad_inputs() {
    let dir = TempDir::new("parse_bad");

    let missing = dir.path.join("does_not_exist.http");
    let out = Command::new(bin())
        .args(["parse", missing.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("mercury:"));

    let bad_file = dir.path.join("bad.http");
    fs::write(&bad_file, b"NOT HTTP AT ALL\r\n\r\n").unwrap();
    let out = Command::new(bin())
        .args(["parse", bad_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("mercury:"));
}

#[test]
fn serve_bad_dir() {
    let dir = TempDir::new("serve_bad");
    let missing = dir.path.join("missing_dir");
    let out = Command::new(bin())
        .args([
            "serve",
            missing.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("mercury:"));
}

#[test]
fn get_bad_inputs() {
    let https = Command::new(bin())
        .args(["get", "https://127.0.0.1:8080/secret"])
        .output()
        .unwrap();
    assert_eq!(https.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&https.stderr);
    assert!(
        stderr.contains("mercury: https not supported"),
        "stderr: {stderr}"
    );

    let bad_scheme = Command::new(bin())
        .args(["get", "ftp://127.0.0.1:8080/file"])
        .output()
        .unwrap();
    assert_eq!(bad_scheme.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad_scheme.stderr).contains("mercury:"));

    let unused_port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let closed = Command::new(bin())
        .args(["get", &format!("http://127.0.0.1:{unused_port}/")])
        .output()
        .unwrap();
    assert_eq!(closed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&closed.stderr).contains("mercury:"));
}

#[test]
fn serve_on_free_port_with_tcpstream_and_get() {
    let dir = TempDir::new("serve_good");
    fs::write(dir.path.join("index.html"), b"<h1>Mercury</h1>").unwrap();
    fs::write(dir.path.join("hello.txt"), b"hello from mercury\n").unwrap();

    let mut child = Command::new(bin())
        .args([
            "serve",
            dir.path.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let _guard = ChildGuard(Some(child));

    let (tx, rx) = mpsc::channel::<SocketAddr>();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if let Some(addr_str) = line.strip_prefix("listening:") {
                if let Ok(addr) = addr_str.trim().parse::<SocketAddr>() {
                    let _ = tx.send(addr);
                    break;
                }
            }
            line.clear();
        }
    });

    let addr = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("serve did not print listening address in time");

    // Verify with a raw TcpStream in a worker thread.
    let tcp_handle = thread::spawn(move || {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .write_all(b"GET /index.html HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        raw
    });
    let raw_resp = tcp_handle.join().unwrap();
    assert!(raw_resp.contains("200 OK"), "raw_resp: {raw_resp}");
    assert!(
        raw_resp.contains("<h1>Mercury</h1>"),
        "raw_resp: {raw_resp}"
    );

    // Now run `mercury get` against the live server.
    let url = format!("http://{addr}/hello.txt");
    let out = Command::new(bin()).args(["get", &url]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let get_stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        get_stdout.contains("status: 200 OK"),
        "get_stdout: {get_stdout}"
    );
    assert!(
        get_stdout.contains("hello from mercury"),
        "get_stdout: {get_stdout}"
    );
}
