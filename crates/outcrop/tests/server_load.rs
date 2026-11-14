//! Load, slowloris, and connection-limit integration tests for `outcrop::server`.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use outcrop::server::{Config, Router, Server};
use outcrop::{parse_response, Limits, Response};

fn ok_pong() -> Response {
    Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: Vec::new(),
        body: b"pong".to_vec(),
    }
}

#[test]
fn load_2k_connections_32_threads() {
    let mut router = Router::new();
    router.route("GET", "/ping", Box::new(|_| ok_pong()));
    let cfg = Config {
        max_connections: 4096,
        ..Config::default()
    };
    let srv = Server::start(cfg, router).unwrap();
    let addr = srv.local_addr();

    let total = 2_000usize; // leader: 10k plus the leader check exhausted Windows ephemeral ports (TIME_WAIT)
    let threads = 32usize;
    let per_thread = total / threads;
    let extra = total % threads;

    let start = Instant::now();
    let mut handles = Vec::with_capacity(threads);
    for tid in 0..threads {
        let n = per_thread + usize::from(tid < extra);
        handles.push(thread::spawn(move || {
            for _ in 0..n {
                let mut stream = TcpStream::connect(addr).unwrap();
                stream
                    .write_all(b"GET /ping HTTP/1.1\r\nConnection: close\r\n\r\n")
                    .unwrap();
                let mut buf = [0u8; 256];
                let mut filled = 0;
                loop {
                    let read = stream.read(&mut buf[filled..]).unwrap();
                    if read == 0 {
                        break;
                    }
                    filled += read;
                }
                let (resp, _) = parse_response(&buf[..filled], &Limits::default(), false).unwrap();
                assert_eq!(resp.status, 200);
                assert_eq!(resp.body, b"pong");
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Wait briefly for worker threads to drop their OpenGuard.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stats = srv.stats();
        if stats.open == 0 {
            assert_eq!(stats.accepted, total as u64);
            assert_eq!(stats.requests, total as u64);
            assert_eq!(stats.rejected, 0);
            break;
        }
        assert!(Instant::now() < deadline, "open did not reach 0: {stats:?}");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(start.elapsed() < Duration::from_secs(10));
    srv.stop();
}

#[test]
fn slowloris_50_sockets_and_normal_client() {
    let mut router = Router::new();
    router.route("GET", "/ok", Box::new(|_| ok_pong()));
    let cfg = Config {
        read_timeout_ms: 300,
        threads: 8,
        ..Config::default()
    };
    let srv = Server::start(cfg, router).unwrap();
    let addr = srv.local_addr();

    let mut slow_handles = Vec::with_capacity(50);
    for _ in 0..50 {
        slow_handles.push(thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            let req = b"GET /ok HTTP/1.1\r\nHost: slow\r\n\r\n";
            for &b in req.iter().take(5) {
                if s.write_all(&[b]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }));
    }

    // Normal client still gets answers while slow sockets are active.
    let mut got_ok = false;
    let normal_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < normal_deadline {
        if let Ok(mut s) = TcpStream::connect(addr) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            if s.write_all(b"GET /ok HTTP/1.1\r\nConnection: close\r\n\r\n")
                .is_ok()
            {
                let mut buf = Vec::new();
                if s.read_to_end(&mut buf).is_ok() {
                    if let Ok((resp, _)) = parse_response(&buf, &Limits::default(), false) {
                        if resp.status == 200 {
                            got_ok = true;
                            break;
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(got_ok, "normal client did not get 200");

    for h in slow_handles {
        h.join().unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while srv.stats().open > 0 {
        assert!(
            Instant::now() < deadline,
            "slowloris sockets still open: {:?}",
            srv.stats()
        );
        thread::sleep(Duration::from_millis(10));
    }

    srv.stop();
}

#[test]
fn max_connections_rejects_with_503() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let entered_c = Arc::clone(&entered);
    let release_c = Arc::clone(&release);

    let mut router = Router::new();
    router.route(
        "GET",
        "/hold",
        Box::new(move |_| {
            entered_c.wait();
            release_c.wait();
            ok_pong()
        }),
    );

    let cfg = Config {
        max_connections: 1,
        threads: 2,
        ..Config::default()
    };
    let srv = Server::start(cfg, router).unwrap();
    let addr = srv.local_addr();

    let first = thread::spawn(move || {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(b"GET /hold HTTP/1.1\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).unwrap();
        let (resp, _) = parse_response(&buf, &Limits::default(), false).unwrap();
        assert_eq!(resp.status, 200);
    });

    entered.wait();

    let mut rejected = TcpStream::connect(addr).unwrap();
    rejected
        .write_all(b"GET /hold HTTP/1.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    rejected.read_to_end(&mut buf).unwrap();
    let (resp, _) = parse_response(&buf, &Limits::default(), false).unwrap();
    assert_eq!(resp.status, 503);

    release.wait();
    first.join().unwrap();
    assert_eq!(srv.stats().rejected, 1);

    srv.stop();
}
