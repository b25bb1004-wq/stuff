//! Integration tests for `deploy::run_web`.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use deploy::run_web;

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "deploy-web-it-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn free_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

fn wait_for_ready(addr: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("server did not become ready on {addr}");
}

struct HttpResp {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn http_get(addr: SocketAddr, path: &str) -> HttpResp {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();

    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("missing HTTP header separator");
    let head = std::str::from_utf8(&raw[..sep]).unwrap();
    let body = raw[sep + 4..].to_vec();

    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let status: u16 = status_line
        .split_ascii_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();

    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    HttpResp {
        status,
        headers,
        body,
    }
}

#[test]
fn web_serves_bundle_healthz_blocks_traversal_and_stops_cleanly() {
    let root = unique_temp_dir("serve");
    fs::create_dir_all(root.join("athena")).unwrap();
    let index_html = b"<!doctype html><title>Monolith</title>";
    let athena_html = b"<!doctype html><title>Athena</title>";
    let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
    fs::write(root.join("index.html"), index_html).unwrap();
    fs::write(root.join("athena").join("index.html"), athena_html).unwrap();
    fs::write(root.join("site.wasm"), wasm_bytes).unwrap();

    let addr = free_addr();
    let addr_str = addr.to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = Arc::clone(&stop);
    let root_srv = root.clone();

    let handle = thread::spawn(move || run_web(&addr_str, &root_srv, &stop_srv));
    wait_for_ready(addr);

    let health = http_get(addr, "/healthz");
    assert_eq!(health.status, 200);
    assert_eq!(health.body, b"ok");
    assert!(health
        .headers
        .get("content-type")
        .unwrap()
        .contains("text/plain"));

    let site = http_get(addr, "/");
    assert_eq!(site.status, 200);
    assert_eq!(site.body, index_html);
    assert!(site
        .headers
        .get("content-type")
        .unwrap()
        .contains("text/html"));

    let athena = http_get(addr, "/athena/");
    assert_eq!(athena.status, 200);
    assert_eq!(athena.body, athena_html);

    let wasm = http_get(addr, "/site.wasm");
    assert_eq!(wasm.status, 200);
    assert_eq!(wasm.body, wasm_bytes);
    assert_eq!(
        wasm.headers.get("content-type").map(String::as_str),
        Some("application/wasm")
    );

    let trav1 = http_get(addr, "/../Cargo.toml");
    assert!(matches!(trav1.status, 403 | 404));
    assert!(!String::from_utf8_lossy(&trav1.body).contains("[package]"));

    let trav2 = http_get(addr, "/%2e%2e/x");
    assert!(matches!(trav2.status, 403 | 404));

    let missing = http_get(addr, "/no-such-file.txt");
    assert_eq!(missing.status, 404);

    let stop_started = Instant::now();
    stop.store(true, Ordering::Release);
    let res = handle.join().unwrap();
    assert!(res.is_ok(), "run_web returned error: {res:?}");
    assert!(
        stop_started.elapsed() < Duration::from_secs(2),
        "server took too long to stop: {:?}",
        stop_started.elapsed()
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn run_web_refuses_missing_athena_index() {
    let root = unique_temp_dir("missing-athena");
    fs::write(root.join("index.html"), b"<html></html>").unwrap();
    let stop = AtomicBool::new(false);

    let err = run_web("127.0.0.1:0", &root, &stop).unwrap_err();
    assert!(
        err.to_string().contains("athena"),
        "expected clear error mentioning athena, got: {err}"
    );

    let _ = fs::remove_dir_all(&root);
}
