//! Integration tests for WebSocket, byte-range static files, and streaming routes.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use outcrop::server::{Config, Router, Server};
use outcrop::ws::{accept_key, decode_frame, encode_frame, Frame, Message, WsError};
use outcrop::{parse_response, Limits, Response};

fn start_server(router: Router) -> Server {
    Server::start(
        Config {
            threads: 4,
            read_timeout_ms: 2000,
            ..Config::default()
        },
        router,
    )
    .unwrap()
}

fn connect(addr: SocketAddr) -> TcpStream {
    let s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    s
}

fn read_until_eof(stream: &mut TcpStream) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    out
}

fn read_http_head(stream: &mut TcpStream) -> (Response, Vec<u8>) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head_end = pos + 4;
            let (resp, _) = parse_response(&buf[..head_end], &Limits::default(), true).unwrap();
            return (resp, buf[head_end..].to_vec());
        }
        let n = stream.read(&mut tmp).unwrap();
        assert!(n > 0, "unexpected EOF before HTTP head");
        buf.extend_from_slice(&tmp[..n]);
    }
}

fn ws_handshake(addr: SocketAddr, path: &str, key: &str) -> (TcpStream, Vec<u8>) {
    let mut stream = connect(addr);
    let req = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: WebSocket\r\n\
         Connection: keep-alive, Upgrade\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: {key}\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let (resp, leftover) = read_http_head(&mut stream);
    assert_eq!(resp.status, 101);
    assert_eq!(
        resp.header("sec-websocket-accept").unwrap(),
        accept_key(key).as_bytes()
    );
    (stream, leftover)
}

fn client_recv_frame(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Frame {
    let mut tmp = [0u8; 4096];
    loop {
        if let Some((f, used)) = decode_frame(buf, false, 32 * 1024 * 1024).unwrap() {
            buf.drain(..used);
            return f;
        }
        let n = stream.read(&mut tmp).unwrap();
        assert!(n > 0, "unexpected EOF waiting for WS frame");
        buf.extend_from_slice(&tmp[..n]);
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("outcrop_t38_{tag}_{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn websocket_handshake_validation() {
    let mut router = Router::new();
    router.websocket("/ws", Box::new(|_ws| {}));
    let srv = start_server(router);
    let addr = srv.local_addr();

    // Plain GET -> 400
    let mut s1 = connect(addr);
    s1.write_all(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let raw1 = read_until_eof(&mut s1);
    let (r1, _) = parse_response(&raw1, &Limits::default(), false).unwrap();
    assert_eq!(r1.status, 400);

    // Wrong version -> 426 with Sec-WebSocket-Version: 13
    let mut s2 = connect(addr);
    s2.write_all(
        b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 8\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let raw2 = read_until_eof(&mut s2);
    let (r2, _) = parse_response(&raw2, &Limits::default(), false).unwrap();
    assert_eq!(r2.status, 426);
    assert_eq!(r2.header("sec-websocket-version").unwrap(), b"13");

    // Bad key (not 16 bytes) -> 400
    let mut s3 = connect(addr);
    s3.write_all(
        b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: c2hvcnQ=\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let raw3 = read_until_eof(&mut s3);
    let (r3, _) = parse_response(&raw3, &Limits::default(), false).unwrap();
    assert_eq!(r3.status, 400);

    // Wrong method -> 405
    let mut s4 = connect(addr);
    s4.write_all(b"POST /ws HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let raw4 = read_until_eof(&mut s4);
    let (r4, _) = parse_response(&raw4, &Limits::default(), false).unwrap();
    assert_eq!(r4.status, 405);
}

#[test]
fn websocket_live_echo_and_close_handshake() {
    let (tx_closed, rx_closed) = mpsc::channel();
    let mut router = Router::new();
    router.websocket(
        "/echo",
        Box::new(move |mut ws| loop {
            match ws.recv() {
                Ok(Message::Text(t)) => ws.send(&Message::Text(t)).unwrap(),
                Ok(Message::Binary(b)) => ws.send(&Message::Binary(b)).unwrap(),
                Ok(Message::Close(_)) => {
                    let second = ws.recv();
                    let _ = tx_closed.send(second);
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }),
    );

    let srv = start_server(router);
    let (mut stream, mut buf) = ws_handshake(srv.local_addr(), "/echo", "dGhlIHNhbXBsZSBub25jZQ==");
    let mask = [0x12, 0x34, 0x56, 0x78];

    // 1. Text echo
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: true,
                opcode: 0x1,
                payload: b"hello ws".to_vec(),
            },
            Some(mask),
        ))
        .unwrap();
    let f = client_recv_frame(&mut stream, &mut buf);
    assert_eq!(
        f,
        Frame {
            fin: true,
            opcode: 0x1,
            payload: b"hello ws".to_vec()
        }
    );

    // 2. Binary echo
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: true,
                opcode: 0x2,
                payload: vec![1, 2, 3, 4],
            },
            Some(mask),
        ))
        .unwrap();
    let f = client_recv_frame(&mut stream, &mut buf);
    assert_eq!(
        f,
        Frame {
            fin: true,
            opcode: 0x2,
            payload: vec![1, 2, 3, 4]
        }
    );

    // 3. Fragmented text + interleaved Ping + Pong (skipped)
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: false,
                opcode: 0x1,
                payload: b"Hel".to_vec(),
            },
            Some(mask),
        ))
        .unwrap();
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: true,
                opcode: 0x9,
                payload: b"ping!".to_vec(),
            },
            Some(mask),
        ))
        .unwrap();
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: true,
                opcode: 0xA,
                payload: b"ignored".to_vec(),
            },
            Some(mask),
        ))
        .unwrap();
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: true,
                opcode: 0x0,
                payload: b"lo".to_vec(),
            },
            Some(mask),
        ))
        .unwrap();

    let pong = client_recv_frame(&mut stream, &mut buf);
    assert_eq!(
        pong,
        Frame {
            fin: true,
            opcode: 0xA,
            payload: b"ping!".to_vec()
        }
    );
    let reassembled = client_recv_frame(&mut stream, &mut buf);
    assert_eq!(
        reassembled,
        Frame {
            fin: true,
            opcode: 0x1,
            payload: b"Hello".to_vec()
        }
    );

    // 4. Close handshake
    let mut close_payload = 1000u16.to_be_bytes().to_vec();
    close_payload.extend_from_slice(b"bye");
    stream
        .write_all(&encode_frame(
            &Frame {
                fin: true,
                opcode: 0x8,
                payload: close_payload.clone(),
            },
            Some(mask),
        ))
        .unwrap();
    let echoed_close = client_recv_frame(&mut stream, &mut buf);
    assert_eq!(
        echoed_close,
        Frame {
            fin: true,
            opcode: 0x8,
            payload: close_payload
        }
    );
    assert_eq!(rx_closed.recv().unwrap(), Err(WsError::Closed));
}

#[test]
fn websocket_protocol_error_close_codes() {
    let mut router = Router::new();
    router.websocket(
        "/err",
        Box::new(|mut ws| {
            let _ = ws.recv();
        }),
    );
    let srv = start_server(router);
    let addr = srv.local_addr();
    let mask = [0xAA, 0xBB, 0xCC, 0xDD];

    let send_bad_and_expect_code = |wire: Vec<u8>, expected_code: u16| {
        let (mut stream, mut buf) = ws_handshake(addr, "/err", "dGhlIHNhbXBsZSBub25jZQ==");
        stream.write_all(&wire).unwrap();
        stream.flush().unwrap();
        let f = client_recv_frame(&mut stream, &mut buf);
        assert_eq!(f.opcode, 0x8);
        assert!(f.payload.len() >= 2);
        let code = u16::from_be_bytes([f.payload[0], f.payload[1]]);
        assert_eq!(code, expected_code);
    };

    // Unmasked client frame -> 1002
    send_bad_and_expect_code(
        encode_frame(
            &Frame {
                fin: true,
                opcode: 0x1,
                payload: b"hi".to_vec(),
            },
            None,
        ),
        1002,
    );

    // RSV bit set -> 1002
    let mut rsv_wire = encode_frame(
        &Frame {
            fin: true,
            opcode: 0x1,
            payload: b"hi".to_vec(),
        },
        Some(mask),
    );
    rsv_wire[0] |= 0x40;
    send_bad_and_expect_code(rsv_wire, 1002);

    // Unknown opcode 0x3 -> 1002
    send_bad_and_expect_code(
        encode_frame(
            &Frame {
                fin: true,
                opcode: 0x3,
                payload: vec![],
            },
            Some(mask),
        ),
        1002,
    );

    // Fragmented control frame -> 1002
    send_bad_and_expect_code(
        encode_frame(
            &Frame {
                fin: false,
                opcode: 0x9,
                payload: vec![],
            },
            Some(mask),
        ),
        1002,
    );

    // >125-byte control frame -> 1002
    send_bad_and_expect_code(
        encode_frame(
            &Frame {
                fin: true,
                opcode: 0x9,
                payload: vec![0u8; 126],
            },
            Some(mask),
        ),
        1002,
    );

    // Continuation without start -> 1002
    send_bad_and_expect_code(
        encode_frame(
            &Frame {
                fin: true,
                opcode: 0x0,
                payload: b"x".to_vec(),
            },
            Some(mask),
        ),
        1002,
    );

    // New data frame inside fragmented message -> 1002
    let mut nested = encode_frame(
        &Frame {
            fin: false,
            opcode: 0x1,
            payload: b"a".to_vec(),
        },
        Some(mask),
    );
    nested.extend_from_slice(&encode_frame(
        &Frame {
            fin: true,
            opcode: 0x2,
            payload: b"b".to_vec(),
        },
        Some(mask),
    ));
    send_bad_and_expect_code(nested, 1002);

    // Invalid UTF-8 in text message -> 1007
    send_bad_and_expect_code(
        encode_frame(
            &Frame {
                fin: true,
                opcode: 0x1,
                payload: vec![0xFF, 0xFE],
            },
            Some(mask),
        ),
        1007,
    );

    // Message over 16 MiB limit -> 1009 (header-only triggers TooBig immediately)
    let mut big_hdr = vec![0x82, 0xFF];
    big_hdr.extend_from_slice(&((16 * 1024 * 1024 + 1) as u64).to_be_bytes());
    big_hdr.extend_from_slice(&mask);
    send_bad_and_expect_code(big_hdr, 1009);
}

#[test]
fn static_range_requests_table() {
    let dir = temp_dir("ranges");
    fs::write(dir.join("alpha.txt"), b"abcdefghijklmnopqrstuvwxyz").unwrap();

    let mut router = Router::new();
    router.static_dir("/files", dir.clone());
    let srv = start_server(router);
    let addr = srv.local_addr();

    let send_req = |method: &str, range: Option<&str>| -> Response {
        let mut s = connect(addr);
        let range_line = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
        let req = format!(
            "{method} /files/alpha.txt HTTP/1.1\r\nHost: localhost\r\n{range_line}Connection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes()).unwrap();
        let raw = read_until_eof(&mut s);
        parse_response(&raw, &Limits::default(), method == "HEAD")
            .unwrap()
            .0
    };

    type RangeCase<'a> = (
        &'a str,
        Option<&'a str>,
        u16,
        Option<&'a [u8]>,
        &'a [u8],
        &'a [u8],
    );
    let cases: &[RangeCase<'_>] = &[
        ("GET", None, 200, None, b"26", b"abcdefghijklmnopqrstuvwxyz"),
        ("HEAD", None, 200, None, b"26", b""),
        (
            "GET",
            Some("bytes=0-3"),
            206,
            Some(b"bytes 0-3/26"),
            b"4",
            b"abcd",
        ),
        (
            "GET",
            Some("bytes=20-"),
            206,
            Some(b"bytes 20-25/26"),
            b"6",
            b"uvwxyz",
        ),
        (
            "GET",
            Some("bytes=-5"),
            206,
            Some(b"bytes 21-25/26"),
            b"5",
            b"vwxyz",
        ),
        (
            "GET",
            Some("bytes=20-100"),
            206,
            Some(b"bytes 20-25/26"),
            b"6",
            b"uvwxyz",
        ),
        (
            "GET",
            Some("bytes=-100"),
            206,
            Some(b"bytes 0-25/26"),
            b"26",
            b"abcdefghijklmnopqrstuvwxyz",
        ),
        (
            "GET",
            Some("bytes=26-"),
            416,
            Some(b"bytes */26"),
            b"0",
            b"",
        ),
        (
            "GET",
            Some("bytes=30-40"),
            416,
            Some(b"bytes */26"),
            b"0",
            b"",
        ),
        ("GET", Some("bytes=-0"), 416, Some(b"bytes */26"), b"0", b""),
        (
            "GET",
            Some("bytes=0-1, 4-5"),
            200,
            None,
            b"26",
            b"abcdefghijklmnopqrstuvwxyz",
        ),
        (
            "GET",
            Some("bytes=10-5"),
            200,
            None,
            b"26",
            b"abcdefghijklmnopqrstuvwxyz",
        ),
        (
            "GET",
            Some("items=0-5"),
            200,
            None,
            b"26",
            b"abcdefghijklmnopqrstuvwxyz",
        ),
        (
            "HEAD",
            Some("bytes=0-3"),
            206,
            Some(b"bytes 0-3/26"),
            b"4",
            b"",
        ),
    ];

    for &(method, range, status, cr, cl, body) in cases {
        let resp = send_req(method, range);
        assert_eq!(resp.status, status, "case {method} {range:?}");
        assert_eq!(resp.header("accept-ranges").unwrap(), b"bytes");
        assert_eq!(resp.header("content-range"), cr);
        assert_eq!(resp.header("content-length").unwrap(), cl);
        assert_eq!(resp.body.as_slice(), body);
    }

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn stream_routes_chunked_http10_and_panic() {
    let mut router = Router::new();
    router.stream(
        "GET",
        "/stream",
        Box::new(|_req, write| {
            write(b"one").unwrap();
            write(b"").unwrap();
            write(b"two").unwrap();
            write(b"three").unwrap();
            200
        }),
    );
    router.stream("GET", "/empty-404", Box::new(|_req, _write| 404));
    router.stream(
        "GET",
        "/panic",
        Box::new(|_req, write| {
            write(b"partial").unwrap();
            panic!("boom");
        }),
    );

    let srv = start_server(router);
    let addr = srv.local_addr();

    // 1. HTTP/1.1 chunked
    let mut s1 = connect(addr);
    s1.write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let raw1 = read_until_eof(&mut s1);
    let raw1_str = String::from_utf8_lossy(&raw1);
    assert!(raw1_str.contains("3\r\none\r\n"));
    assert!(raw1_str.contains("3\r\ntwo\r\n"));
    assert!(raw1_str.contains("5\r\nthree\r\n"));
    assert!(raw1_str.ends_with("0\r\n\r\n"));
    let (resp1, _) = parse_response(&raw1, &Limits::default(), false).unwrap();
    assert_eq!(resp1.status, 200);
    assert_eq!(
        resp1.header("content-type").unwrap(),
        b"application/octet-stream"
    );
    assert_eq!(resp1.body, b"onetwothree");

    // 2. Empty stream returning 404
    let mut s2 = connect(addr);
    s2.write_all(b"GET /empty-404 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let raw2 = read_until_eof(&mut s2);
    let (resp2, _) = parse_response(&raw2, &Limits::default(), false).unwrap();
    assert_eq!(resp2.status, 404);
    assert!(resp2.body.is_empty());

    // 3. HTTP/1.0 close-delimited
    let mut s3 = connect(addr);
    s3.write_all(b"GET /stream HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let raw3 = read_until_eof(&mut s3);
    let head_end = raw3.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let head3 = std::str::from_utf8(&raw3[..head_end]).unwrap();
    assert!(head3.starts_with("HTTP/1.0 200"));
    assert!(!head3.to_ascii_lowercase().contains("transfer-encoding"));
    assert_eq!(&raw3[head_end..], b"onetwothree");

    // 4. Panic mid-stream: no terminating 0 chunk
    let mut s4 = connect(addr);
    s4.write_all(b"GET /panic HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let raw4 = read_until_eof(&mut s4);
    assert!(raw4.windows(7).any(|w| w == b"partial"));
    assert!(!raw4.ends_with(b"0\r\n\r\n"));
}
