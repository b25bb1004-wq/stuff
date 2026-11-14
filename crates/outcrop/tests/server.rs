//! Integration tests for `outcrop::server` routing, keep-alive, static files, and stop.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use outcrop::server::{Config, Router, Server};
use outcrop::{parse_response, write_request, Limits, Request, Response};

fn ok_text(body: &str) -> Response {
    Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![("Content-Type".to_string(), b"text/plain".to_vec())],
        body: body.as_bytes().to_vec(),
    }
}

fn send_raw(addr: std::net::SocketAddr, raw: &[u8]) -> Response {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(raw).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let (resp, _) = parse_response(&buf, &Limits::default(), false).unwrap();
    resp
}

#[test]
fn routes_404_405_panic_and_limits() {
    let mut router = Router::new();
    router.route("GET", "/hello", Box::new(|_| ok_text("hi")));
    router.route(
        "POST",
        "/hello",
        Box::new(|req| Response {
            minor: 1,
            status: 200,
            reason: "OK".to_string(),
            headers: Vec::new(),
            body: req.body.clone(),
        }),
    );
    router.route("GET", "/api/*", Box::new(|req| ok_text(&req.target)));
    router.route("GET", "/panic", Box::new(|_| panic!("boom")));

    let cfg = Config {
        limits: Limits {
            max_head: 512,
            max_headers: 4,
            max_body: 64,
        },
        ..Config::default()
    };
    let srv = Server::start(cfg, router).unwrap();
    let addr = srv.local_addr();

    // Exact GET & POST.
    let r = send_raw(
        addr,
        b"GET /hello HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"hi");

    let r = send_raw(
        addr,
        b"POST /hello HTTP/1.1\r\nHost: x\r\nContent-Length: 4\r\nConnection: close\r\n\r\nping",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"ping");

    // Prefix route.
    let r = send_raw(
        addr,
        b"GET /api/v1/users?a=1 HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"/api/v1/users?a=1");

    // 404.
    let r = send_raw(addr, b"GET /nope HTTP/1.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 404);

    // 405 with Allow header.
    let r = send_raw(addr, b"DELETE /hello HTTP/1.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 405);
    let allow = std::str::from_utf8(r.header("allow").unwrap()).unwrap();
    assert!(allow.contains("GET") && allow.contains("POST"));

    // Panic -> 500 and connection closed, next connection still works.
    let r = send_raw(addr, b"GET /panic HTTP/1.1\r\n\r\n");
    assert_eq!(r.status, 500);
    assert_eq!(r.header("connection"), Some(&b"close"[..]));

    let r = send_raw(addr, b"GET /hello HTTP/1.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 200);

    // Parse error 400, 413, 431.
    let r = send_raw(addr, b"BADREQUESTLINE\r\n\r\n");
    assert_eq!(r.status, 400);
    let r = send_raw(addr, b"POST /hello HTTP/1.1\r\nContent-Length: 100\r\n\r\n");
    assert_eq!(r.status, 413);
    let r = send_raw(
        addr,
        b"GET /hello HTTP/1.1\r\nA: 1\r\nB: 2\r\nC: 3\r\nD: 4\r\nE: 5\r\n\r\n",
    );
    assert_eq!(r.status, 431);

    srv.stop();
}

#[test]
fn keep_alive_100_and_pipelining() {
    let mut router = Router::new();
    let count = Arc::new(AtomicUsize::new(0));
    let count_c = Arc::clone(&count);
    router.route(
        "GET",
        "/n",
        Box::new(move |req| {
            count_c.fetch_add(1, Ordering::SeqCst);
            ok_text(&req.target)
        }),
    );
    let srv = Server::start(Config::default(), router).unwrap();
    let addr = srv.local_addr();

    // 100 requests on a single keep-alive connection.
    {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buf = Vec::new();
        for i in 0..100 {
            let req = Request {
                method: "GET".to_string(),
                target: format!("/n?i={i}"),
                minor: 1,
                headers: vec![("Host".to_string(), b"localhost".to_vec())],
                body: Vec::new(),
            };
            stream.write_all(&write_request(&req)).unwrap();
            loop {
                match parse_response(&buf, &Limits::default(), false) {
                    Ok((resp, used)) => {
                        buf.drain(..used);
                        assert_eq!(resp.status, 200);
                        assert_eq!(resp.body, format!("/n?i={i}").as_bytes());
                        if i == 99 {
                            assert_eq!(resp.header("connection"), Some(&b"close"[..]));
                        }
                        break;
                    }
                    Err(outcrop::Error::Incomplete) => {
                        let mut tmp = [0u8; 1024];
                        let n = stream.read(&mut tmp).unwrap();
                        assert!(n > 0);
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    Err(e) => panic!("unexpected parse error: {e:?}"),
                }
            }
        }
    }

    // Pipelined: 5 requests in one write, all answered in order.
    {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut batch = Vec::new();
        for i in 0..5 {
            let mut headers = Vec::new();
            if i == 4 {
                headers.push(("Connection".to_string(), b"close".to_vec()));
            }
            let req = Request {
                method: "GET".to_string(),
                target: format!("/n?p={i}"),
                minor: 1,
                headers,
                body: Vec::new(),
            };
            batch.extend_from_slice(&write_request(&req));
        }
        stream.write_all(&batch).unwrap();

        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
        let mut off = 0;
        for i in 0..5 {
            let (resp, used) = parse_response(&buf[off..], &Limits::default(), false).unwrap();
            off += used;
            assert_eq!(resp.status, 200);
            assert_eq!(resp.body, format!("/n?p={i}").as_bytes());
        }
        assert_eq!(off, buf.len());
    }

    srv.stop();
}

#[test]
fn static_files_and_traversal() {
    let dir = std::env::temp_dir().join(format!("outcrop_static_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("index.html"), b"<h1>hi</h1>").unwrap();
    fs::write(dir.join("sub/data.json"), b"{\"ok\":true}").unwrap();
    fs::write(dir.join("app.wasm"), b"\x00asm").unwrap();
    fs::write(dir.join("blob.bin"), b"12345").unwrap();

    let mut router = Router::new();
    router.static_dir("/static", dir.clone());
    let srv = Server::start(Config::default(), router).unwrap();
    let addr = srv.local_addr();

    // Directory -> index.html.
    let r = send_raw(addr, b"GET /static/ HTTP/1.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"<h1>hi</h1>");
    assert!(std::str::from_utf8(r.header("content-type").unwrap())
        .unwrap()
        .starts_with("text/html"));

    // JSON and WASM and octet-stream.
    let r = send_raw(
        addr,
        b"GET /static/sub/data.json HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some(&b"application/json"[..]));
    assert_eq!(r.body, b"{\"ok\":true}");

    let r = send_raw(
        addr,
        b"GET /static/app.wasm HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some(&b"application/wasm"[..]));

    let r = send_raw(
        addr,
        b"GET /static/blob.bin HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header("content-type"),
        Some(&b"application/octet-stream"[..])
    );

    // HEAD sends headers only.
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .write_all(b"HEAD /static/blob.bin HTTP/1.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let (r, _) = parse_response(&buf, &Limits::default(), true).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-length"), Some(&b"5"[..]));
    assert!(r.body.is_empty());

    // Traversal, %2e%2e, absolute, NUL -> 404.
    for bad in [
        "/static/../etc/passwd",
        "/static/%2e%2e/etc/passwd",
        "/static/%2E%2E/%2e%2e/secret",
        "/static//etc/passwd",
        "/static/%2fetc%2fpasswd",
        "/static/foo%00bar",
    ] {
        let req = format!("GET {bad} HTTP/1.1\r\nConnection: close\r\n\r\n");
        let r = send_raw(addr, req.as_bytes());
        assert_eq!(r.status, 404, "expected 404 for {bad}");
    }

    // Non-GET/HEAD -> 405 with Allow: GET, HEAD.
    let r = send_raw(
        addr,
        b"POST /static/blob.bin HTTP/1.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 405);
    assert_eq!(r.header("allow"), Some(&b"GET, HEAD"[..]));

    srv.stop();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stop_with_idle_and_inflight() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let entered_c = Arc::clone(&entered);
    let release_c = Arc::clone(&release);

    let mut router = Router::new();
    router.route(
        "GET",
        "/slow",
        Box::new(move |_| {
            entered_c.wait();
            release_c.wait();
            ok_text("done")
        }),
    );
    router.route("GET", "/fast", Box::new(|_| ok_text("fast")));

    let srv = Server::start(Config::default(), router).unwrap();
    let addr = srv.local_addr();

    // 20 idle keep-alive connections.
    let mut idle_conns = Vec::with_capacity(20);
    for _ in 0..20 {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(b"GET /fast HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        idle_conns.push(s);
    }

    // 1 in-flight slow request.
    let slow_client = thread::spawn(move || {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(b"GET /slow HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).unwrap();
        let (resp, _) = parse_response(&buf[..n], &Limits::default(), false).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"done");
    });

    entered.wait();
    let stop_thread = thread::spawn(move || {
        let start = Instant::now();
        srv.stop();
        start.elapsed()
    });

    thread::sleep(Duration::from_millis(100));
    release.wait();
    slow_client.join().unwrap();
    let elapsed = stop_thread.join().unwrap();
    assert!(elapsed < Duration::from_secs(2), "stop took {elapsed:?}");
}
