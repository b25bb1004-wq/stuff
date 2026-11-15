//! Integration tests for `odyssey::fetch`.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use mercury::{Error as HttpError, Limits};
use odyssey::fetch::{get, FetchError, MAX_BODY_BYTES};

fn spawn_raw_server<F>(handler: F) -> (SocketAddr, thread::JoinHandle<()>)
where
    F: FnOnce(TcpListener) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || handler(listener));
    (addr, handle)
}

fn read_request_head(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn request_path(req_text: &str) -> String {
    req_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string()
}

#[test]
fn fetch_basic_page() {
    let (addr, handle) = spawn_raw_server(|listener| {
        let (mut stream, _) = listener.accept().unwrap();
        let req = read_request_head(&mut stream);
        assert!(req.starts_with("GET /hello.html HTTP/1.1\r\n"));
        assert!(req.contains("Host:"));
        assert!(req.contains("Connection: close"));
        let body = b"<h1>Hello</h1>";
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
        stream.write_all(resp.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });

    let url = format!("http://{addr}/hello.html");
    let resp = get(&url, &Limits::default()).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"<h1>Hello</h1>");
    handle.join().unwrap();
}

#[test]
fn fetch_redirect_chain_five_and_relative_locations() {
    let (addr, handle) = spawn_raw_server(|listener| {
        for step in 0..6 {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_request_head(&mut stream);
            let path = request_path(&req);
            let resp = match (step, path.as_str()) {
                (0, "/start") => {
                    "HTTP/1.1 301 Moved\r\nLocation: dir/two\r\nContent-Length: 0\r\n\r\n"
                        .to_string()
                }
                (1, "/dir/two") => {
                    "HTTP/1.1 302 Found\r\nLocation: ../three\r\nContent-Length: 0\r\n\r\n"
                        .to_string()
                }
                (2, "/three") => {
                    "HTTP/1.1 303 See Other\r\nLocation: /four/page\r\nContent-Length: 0\r\n\r\n"
                        .to_string()
                }
                (3, "/four/page") => {
                    "HTTP/1.1 307 Temp\r\nLocation: ?done=1\r\nContent-Length: 0\r\n\r\n"
                        .to_string()
                }
                (4, "/four/page?done=1") => {
                    "HTTP/1.1 308 Perm\r\nLocation: final.html\r\nContent-Length: 0\r\n\r\n"
                        .to_string()
                }
                (5, "/four/final.html") => {
                    "HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nlanded!".to_string()
                }
                other => panic!("unexpected redirect step: {other:?}"),
            };
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });

    let url = format!("http://{addr}/start");
    let resp = get(&url, &Limits::default()).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"landed!");
    handle.join().unwrap();
}

#[test]
fn fetch_redirect_loop_errors_after_five() {
    let (addr, handle) = spawn_raw_server(|listener| {
        for _ in 0..6 {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request_head(&mut stream);
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        }
    });

    let url = format!("http://{addr}/loop");
    let err = get(&url, &Limits::default()).unwrap_err();
    assert!(matches!(err, FetchError::TooManyRedirects));
    handle.join().unwrap();
}

#[test]
fn fetch_nine_mib_body_errors() {
    let nine_mib = 9 * 1024 * 1024;
    let (addr, handle) = spawn_raw_server(move |listener| {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut stream);
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {nine_mib}\r\n\r\n");
        let _ = stream.write_all(head.as_bytes());
    });

    let url = format!("http://{addr}/big");
    let err = get(&url, &Limits::default()).unwrap_err();
    assert!(matches!(err, FetchError::Http(HttpError::BodyTooLarge)));
    handle.join().unwrap();
}

#[test]
fn fetch_chunked_over_limit_errors() {
    let (addr, handle) = spawn_raw_server(|listener| {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut stream);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        let chunk_payload = vec![b'a'; 1024 * 1024];
        let header = format!("{:x}\r\n", chunk_payload.len());
        for _ in 0..9 {
            if stream.write_all(header.as_bytes()).is_err()
                || stream.write_all(&chunk_payload).is_err()
                || stream.write_all(b"\r\n").is_err()
            {
                break;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n");
    });

    let url = format!("http://{addr}/big-chunked");
    let err = get(&url, &Limits::default()).unwrap_err();
    assert!(matches!(err, FetchError::Http(HttpError::BodyTooLarge)));
    handle.join().unwrap();
}

#[test]
fn fetch_stalled_after_headers_times_out() {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = Arc::clone(&stop);
    let (addr, handle) = spawn_raw_server(move |listener| {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\n\r\n")
            .unwrap();
        for _ in 0..60 {
            if stop_srv.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });

    let url = format!("http://{addr}/stall");
    let err = get(&url, &Limits::default()).unwrap_err();
    stop.store(true, Ordering::Relaxed);
    assert!(
        matches!(err, FetchError::Io(_)),
        "expected Io timeout, got {err:?}"
    );
    handle.join().unwrap();
}

#[test]
fn fetch_chunked_encoding() {
    let (addr, handle) = spawn_raw_server(|listener| {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut stream);
        let resp = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                    5\r\nHello\r\n7\r\n world!\r\n0\r\n\r\n";
        stream.write_all(resp.as_bytes()).unwrap();
    });

    let url = format!("http://{addr}/chunked");
    let resp = get(&url, &Limits::default()).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"Hello world!");
    handle.join().unwrap();
}

#[test]
fn fetch_malformed_status_line() {
    let (addr, handle) = spawn_raw_server(|listener| {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut stream);
        let _ = stream.write_all(b"GARBAGE 200 OK\r\nContent-Length: 0\r\n\r\n");
    });

    let url = format!("http://{addr}/bad");
    let err = get(&url, &Limits::default()).unwrap_err();
    assert!(matches!(err, FetchError::Http(HttpError::BadStatusLine)));
    handle.join().unwrap();
}

#[test]
fn fetch_huge_headers_and_truncation() {
    let (addr, handle) = spawn_raw_server(|listener| {
        // Request 1: huge headers
        let (mut s1, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut s1);
        let _ = s1.write_all(b"HTTP/1.1 200 OK\r\n");
        let big_val = "x".repeat(20 * 1024);
        let _ = s1.write_all(format!("X-Big: {big_val}\r\n\r\n").as_bytes());
        drop(s1);

        // Request 2: truncated fixed-length body
        let (mut s2, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut s2);
        let _ = s2.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial");
        drop(s2);
    });

    let e1 = get(&format!("http://{addr}/huge"), &Limits::default()).unwrap_err();
    assert!(matches!(e1, FetchError::Http(HttpError::HeadTooLarge)));

    let e2 = get(&format!("http://{addr}/trunc"), &Limits::default()).unwrap_err();
    assert!(matches!(e2, FetchError::Http(HttpError::Incomplete)));
    handle.join().unwrap();
}

#[test]
fn fetch_https_and_invalid_urls() {
    assert_eq!(MAX_BODY_BYTES, 8 * 1024 * 1024);
    assert!(matches!(
        get("https://127.0.0.1/secret", &Limits::default()),
        Err(FetchError::Unsupported(_))
    ));
    assert!(matches!(
        get("not-a-url", &Limits::default()),
        Err(FetchError::InvalidUrl(_))
    ));
    assert!(matches!(
        get("http://", &Limits::default()),
        Err(FetchError::InvalidUrl(_))
    ));
}

#[test]
fn fetch_close_delimited_and_redirect_to_https() {
    let (addr, handle) = spawn_raw_server(|listener| {
        let (mut s1, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut s1);
        let _ = s1.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nclose-body");
        drop(s1);

        let (mut s2, _) = listener.accept().unwrap();
        let _ = read_request_head(&mut s2);
        let _ = s2.write_all(
            b"HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1/tls\r\nContent-Length: 0\r\n\r\n",
        );
    });

    let r1 = get(&format!("http://{addr}/close"), &Limits::default()).unwrap();
    assert_eq!(r1.body, b"close-body");

    let e2 = get(&format!("http://{addr}/redir"), &Limits::default()).unwrap_err();
    assert!(matches!(e2, FetchError::Unsupported(_)));
    handle.join().unwrap();
}

/// Leader check: hostile servers sending mutated HTTP responses never panic the client.
#[test]
fn leader_fetch_mutated_responses_never_panic() {
    let base = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\nLocation: /x\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec();
    assay::forall(40, 0x0D75_E067, |r| {
        let mut resp = base.clone();
        for _ in 0..1 + r.below(6) {
            let i = r.below(resp.len());
            resp[i] = r.below(256) as u8;
        }
        resp.truncate(1 + r.below(resp.len()));
        let (addr, h) = spawn_raw_server(move |l| {
            if let Ok((mut s, _)) = l.accept() {
                let _ = read_request_head(&mut s);
                let _ = s.write_all(&resp);
            }
        });
        let _ = get(&format!("http://{addr}/m"), &Limits::default());
        h.join().unwrap();
    });
}
