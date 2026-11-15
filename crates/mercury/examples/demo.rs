//! Demo: parse and write HTTP/1.1 messages, serve one request on 127.0.0.1 with an
//! ephemeral port, and show the WebSocket key and frame helpers. No external network.

use std::io::{Read, Write};
use std::net::TcpStream;

use mercury::server::{Config, Router, Server};
use mercury::ws::{accept_key, encode_frame, Frame};
use mercury::{parse_request, parse_response, write_request, Limits, Request, Response};

fn text(status: u16, reason: &str, body: String) -> Response {
    Response {
        minor: 1,
        status,
        reason: reason.to_string(),
        headers: vec![("Content-Type".to_string(), b"text/plain".to_vec())],
        body: body.into_bytes(),
    }
}

fn main() -> Result<(), String> {
    let lim = Limits::default();

    // 1. Parse a complete request from raw bytes.
    let wire = b"GET /hello?x=1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let (req, used) = parse_request(wire, &lim).map_err(|e| format!("{e:?}"))?;
    println!(
        "parsed {} {} (HTTP/1.{}), {} of {} bytes used",
        req.method,
        req.target,
        req.minor,
        used,
        wire.len()
    );

    // 2. A partial request is reported as Incomplete, not as an error.
    let partial = parse_request(b"GET / HTTP/1.1\r\nHost", &lim);
    println!("partial head gives: {partial:?}");

    // 3. Serve one request on a real socket with the threaded server.
    let mut router = Router::new();
    router.route(
        "GET",
        "/hello",
        Box::new(|r: &Request| text(200, "OK", format!("you asked for {}", r.target))),
    );
    let cfg = Config {
        threads: 2,
        ..Config::default()
    };
    let server = Server::start(cfg, router).map_err(|e| e.to_string())?;
    println!("server bound to 127.0.0.1 on an ephemeral port");

    let client_req = Request {
        method: "GET".to_string(),
        target: "/hello".to_string(),
        minor: 1,
        headers: vec![
            ("Host".to_string(), b"localhost".to_vec()),
            ("Connection".to_string(), b"close".to_vec()),
        ],
        body: Vec::new(),
    };
    let mut sock = TcpStream::connect(server.local_addr()).map_err(|e| e.to_string())?;
    sock.write_all(&write_request(&client_req))
        .map_err(|e| e.to_string())?;
    let mut reply = Vec::new();
    sock.read_to_end(&mut reply).map_err(|e| e.to_string())?;

    let (resp, _) = parse_response(&reply, &lim, false).map_err(|e| format!("{e:?}"))?;
    let body = String::from_utf8_lossy(&resp.body);
    println!("client got status {} {}", resp.status, resp.reason);
    println!("body: {body}");
    let stats = server.stats();
    println!(
        "server counted {} request(s) on {} connection(s)",
        stats.requests, stats.accepted
    );
    server.stop();
    println!("server stopped");

    // 4. WebSocket helpers: the RFC 6455 example key and a small unmasked text frame.
    println!(
        "accept key for RFC 6455 example: {}",
        accept_key("dGhlIHNhbXBsZSBub25jZQ==")
    );
    let frame = Frame {
        fin: true,
        opcode: 0x1,
        payload: b"Hi".to_vec(),
    };
    let bytes = encode_frame(&frame, None);
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    println!("text frame \"Hi\" encodes to: {}", hex.join(" "));
    Ok(())
}
