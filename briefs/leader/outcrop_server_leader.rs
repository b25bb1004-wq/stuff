//! Leader check for T37: mixed keep-alive load with body echo, path traversal including Windows
//! forms, slow clients against a small connection cap, error statuses, and stop under load.
#![cfg(feature = "std")]
#![allow(clippy::unwrap_used)]
use outcrop::server::{Config, Router, Server};
use outcrop::{parse_response, Limits, Request, Response};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn watchdog(secs: u64, what: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        eprintln!("WATCHDOG: {what} still running after {secs} s");
        std::process::abort();
    });
}

fn resp(status: u16, body: &[u8]) -> Response {
    Response {
        minor: 1,
        status,
        reason: "X".into(),
        headers: vec![],
        body: body.to_vec(),
    }
}

fn router() -> Router {
    let mut r = Router::new();
    r.route("GET", "/hi", Box::new(|_| resp(200, b"hi")));
    r.route("POST", "/echo", Box::new(|q: &Request| resp(200, &q.body)));
    r.route(
        "GET",
        "/slow",
        Box::new(|_| {
            std::thread::sleep(Duration::from_millis(700));
            resp(200, b"slow")
        }),
    );
    r
}

fn connect(a: SocketAddr) -> TcpStream {
    let s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

/// Read `n` responses (head-only for HEAD requests when `head` is set) from the stream.
fn read_n(s: &mut TcpStream, n: usize, head: bool) -> Vec<Response> {
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    while out.len() < n {
        if let Ok((r, used)) = parse_response(&buf, &Limits::default(), head) {
            buf.drain(..used);
            out.push(r);
            continue;
        }
        let k = s.read(&mut chunk).unwrap_or(0);
        if k == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..k]);
    }
    out
}

fn one(a: SocketAddr, raw: &str) -> Option<Response> {
    let mut s = connect(a);
    s.write_all(raw.as_bytes()).ok()?;
    read_n(&mut s, 1, raw.starts_with("HEAD")).pop()
}

#[test]
fn leader_mixed_load_with_body_echo() {
    watchdog(120, "load");
    let srv = Server::start(
        Config {
            threads: 8,
            ..Config::default()
        },
        router(),
    )
    .unwrap();
    let a = srv.local_addr();
    let conns = Arc::new(AtomicU64::new(0));
    let reqs = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();
    let hs: Vec<_> = (0..48u64)
        .map(|t| {
            let (conns, reqs) = (Arc::clone(&conns), Arc::clone(&reqs));
            std::thread::spawn(move || {
                for i in 0..300u64 {
                    let k = 1 + ((t * 7 + i) % 3) as usize; // 1..=3 pipelined requests
                    let mut s = connect(a);
                    let mut raw = String::new();
                    for j in 0..k {
                        let body = format!("t{t}i{i}j{j}");
                        let close = if j + 1 == k { "Connection: close\r\n" } else { "" };
                        raw += &format!(
                            "POST /echo HTTP/1.1\r\nContent-Length: {}\r\n{close}\r\n{body}",
                            body.len()
                        );
                    }
                    s.write_all(raw.as_bytes()).unwrap();
                    let rs = read_n(&mut s, k, false);
                    assert_eq!(rs.len(), k, "thread {t} conn {i}");
                    for (j, r) in rs.iter().enumerate() {
                        assert_eq!(r.status, 200);
                        assert_eq!(r.body, format!("t{t}i{i}j{j}").as_bytes());
                    }
                    conns.fetch_add(1, SeqCst);
                    reqs.fetch_add(k as u64, SeqCst);
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while srv.stats().open != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let st = srv.stats();
    eprintln!("load: {:?} in {:?}", st, t0.elapsed());
    assert_eq!(st.open, 0, "leaked connections: {st:?}");
    assert_eq!(st.accepted, conns.load(SeqCst));
    assert_eq!(st.requests, reqs.load(SeqCst));
    srv.stop();
}

#[test]
fn leader_static_traversal_never_escapes() {
    watchdog(60, "static");
    let root = std::env::temp_dir().join(format!("outcrop-leader-{}", std::process::id()));
    let pubd = root.join("pub");
    std::fs::create_dir_all(pubd.join("sub")).unwrap();
    std::fs::write(root.join("secret.txt"), "TOPSECRET").unwrap();
    std::fs::write(pubd.join("index.html"), "<p>home</p>").unwrap();
    std::fs::write(pubd.join("sub").join("a.css"), "b{}").unwrap();
    let mut r = Router::new();
    r.static_dir("/s", pubd.clone());
    let srv = Server::start(Config::default(), r).unwrap();
    let a = srv.local_addr();

    let get = |p: &str| one(a, &format!("GET {p} HTTP/1.1\r\nConnection: close\r\n\r\n")).unwrap();
    let idx = get("/s/");
    assert_eq!((idx.status, idx.body.as_slice()), (200, &b"<p>home</p>"[..]));
    let css = get("/s/sub/a.css");
    assert_eq!(css.status, 200);
    assert!(css.header("content-type").unwrap().starts_with(b"text/css"));
    let head = one(a, "HEAD /s/sub/a.css HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap();
    assert_eq!((head.status, head.body.len()), (200, 0));
    assert_eq!(head.header("content-length"), Some(&b"3"[..]));

    let secret = root.join("secret.txt").display().to_string();
    let abs = secret.replace('\\', "/");
    for p in [
        "/s/../secret.txt",
        "/s/sub/../../secret.txt",
        "/s/%2e%2e/secret.txt",
        "/s/%2E%2E%2Fsecret.txt",
        "/s/sub/..%2f..%2fsecret.txt",
        "/s/..%5csecret.txt",
        "/s/sub\\..\\..\\secret.txt",
        "/s/sub%5c..%5c..%5csecret.txt",
        &format!("/s/{abs}"),
        &format!("/s//{abs}"),
        "/s/index.html%00.png",
        "/s/C:/Windows/win.ini",
        "/s/C:%5cWindows%5cwin.ini",
        "/s/%252e%252e/secret.txt",
    ] {
        let r = get(p);
        assert_eq!(r.status, 404, "{p}");
        assert!(
            !String::from_utf8_lossy(&r.body).contains("TOPSECRET"),
            "{p} leaked"
        );
    }
    srv.stop();
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn leader_error_statuses() {
    watchdog(60, "errors");
    let srv = Server::start(Config::default(), router()).unwrap();
    let a = srv.local_addr();
    assert_eq!(one(a, "GARBAGE\r\n\r\n").unwrap().status, 400);
    let big = format!("GET /hi HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(20_000));
    let st = one(a, &big).unwrap().status;
    assert!(st == 431 || st == 413, "huge head gave {st}");
    let cl = "POST /echo HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n";
    assert_eq!(one(a, cl).unwrap().status, 413);
    let m = one(a, "DELETE /hi HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(m.status, 405);
    assert!(m.header("allow").is_some_and(|v| v.windows(3).any(|w| w == b"GET")));
    assert_eq!(one(a, "GET /nope HTTP/1.1\r\n\r\n").unwrap().status, 404);
    srv.stop();
}

#[test]
fn leader_slow_clients_cannot_starve_the_server() {
    watchdog(60, "slow clients");
    let srv = Server::start(
        Config {
            threads: 4,
            read_timeout_ms: 300,
            max_connections: 16,
            ..Config::default()
        },
        router(),
    )
    .unwrap();
    let a = srv.local_addr();
    let mut slow: Vec<TcpStream> = (0..40).map(|_| connect(a)).collect();
    for s in &mut slow {
        let _ = s.write_all(b"GET /hi HTTP/1.1\r\nX-");
    }
    // While they dribble, a normal client gets an answer (200, or 503 at the cap) promptly.
    let t = Instant::now();
    let r = one(a, "GET /hi HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap();
    assert!(r.status == 200 || r.status == 503, "got {}", r.status);
    assert!(t.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(900));
    // After the timeout every slow socket is closed by the server.
    for (i, s) in slow.iter_mut().enumerate() {
        let _ = s.write_all(b"a");
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut b = [0u8; 512];
        let closed = matches!(s.read(&mut b), Ok(0) | Err(_)) || {
            // a 408 or 400 before close is fine; then the socket must reach EOF
            matches!(s.read(&mut b), Ok(0) | Err(_))
        };
        assert!(closed, "slow socket {i} still open");
    }
    let r = one(a, "GET /hi HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap();
    assert_eq!(r.status, 200);
    srv.stop();
}

#[test]
fn leader_stop_under_load() {
    watchdog(60, "stop under load");
    let srv = Server::start(Config::default(), router()).unwrap();
    let a = srv.local_addr();
    let run = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let hs: Vec<_> = (0..16)
        .map(|_| {
            let run = Arc::clone(&run);
            std::thread::spawn(move || {
                while run.load(SeqCst) {
                    let Ok(mut s) = TcpStream::connect(a) else {
                        break;
                    };
                    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    if s.write_all(b"GET /hi HTTP/1.1\r\n\r\n").is_err() {
                        break;
                    }
                    let _ = read_n(&mut s, 1, false);
                }
            })
        })
        .collect();
    let mut inflight = connect(a);
    std::thread::sleep(Duration::from_millis(200));
    inflight.write_all(b"GET /slow HTTP/1.1\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    srv.stop();
    let took = t.elapsed();
    run.store(false, SeqCst);
    let r = read_n(&mut inflight, 1, false);
    assert_eq!(r.len(), 1, "in-flight request lost at stop");
    assert_eq!(r[0].body, b"slow");
    assert!(took < Duration::from_millis(2700), "stop took {took:?}");
    for h in hs {
        h.join().unwrap();
    }
}
