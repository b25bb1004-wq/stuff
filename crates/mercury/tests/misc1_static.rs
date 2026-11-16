//! Static-file rules: Last-Modified and 304, file-plus-slash 404, raw UTF-8 paths, 404 body.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, UNIX_EPOCH};

use mercury::server::{Config, Router, Server};
use mercury::{parse_response, Limits, Response};

/// 2001-09-09 01:46:40 GMT, the file mtime used throughout.
const MTIME: u64 = 1_000_000_000;
const MTIME_HTTP: &[u8] = b"Sun, 09 Sep 2001 01:46:40 GMT";

fn send(addr: SocketAddr, raw: &[u8], head: bool) -> Response {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(raw).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    parse_response(&buf, &Limits::default(), head).unwrap().0
}

fn fixture(name: &str) -> (std::path::PathBuf, Server) {
    let dir = std::env::temp_dir().join(format!("misc1_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("index.html"), b"<h1>hi</h1>").unwrap();
    fs::write(dir.join("sub/page.html"), b"sub").unwrap();
    fs::write(dir.join("caf\u{e9}-\u{fc}n\u{ef}-\u{65e5}.txt"), b"utf8 ok").unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(dir.join("index.html"))
        .unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(MTIME))
        .unwrap();
    let mut router = Router::new();
    router.static_dir("/static", dir.clone());
    let srv = Server::start(Config::default(), router).unwrap();
    (dir, srv)
}

#[test]
fn last_modified_and_if_modified_since() {
    let (dir, srv) = fixture("lm");
    let a = srv.local_addr();

    let r = send(
        a,
        b"GET /static/index.html HTTP/1.1\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.header("last-modified"), Some(MTIME_HTTP));
    assert_eq!(r.body, b"<h1>hi</h1>");

    let r = send(
        a,
        b"HEAD /static/index.html HTTP/1.1\r\nConnection: close\r\n\r\n",
        true,
    );
    assert_eq!(r.header("last-modified"), Some(MTIME_HTTP));

    // Same time, and a later time, give 304 with no body (GET and HEAD).
    for ims in [
        "Sun, 09 Sep 2001 01:46:40 GMT",
        "Mon, 01 Jan 2040 00:00:00 GMT",
    ] {
        for method in ["GET", "HEAD"] {
            let req = format!(
                "{method} /static/index.html HTTP/1.1\r\nIf-Modified-Since: {ims}\r\nConnection: close\r\n\r\n"
            );
            let r = send(a, req.as_bytes(), true);
            assert_eq!(r.status, 304, "{method} {ims}");
            assert!(r.body.is_empty());
            assert_eq!(r.header("last-modified"), Some(MTIME_HTTP));
        }
    }

    // Earlier time, or a date we cannot parse, still gets the file.
    for ims in ["Sat, 08 Sep 2001 01:46:40 GMT", "not a date"] {
        let req = format!(
            "GET /static/index.html HTTP/1.1\r\nIf-Modified-Since: {ims}\r\nConnection: close\r\n\r\n"
        );
        let r = send(a, req.as_bytes(), false);
        assert_eq!(r.status, 200, "{ims}");
        assert_eq!(r.body, b"<h1>hi</h1>");
    }

    srv.stop();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn file_with_trailing_slash_is_404_directory_is_not() {
    let (dir, srv) = fixture("slash");
    let a = srv.local_addr();

    let r = send(
        a,
        b"GET /static/index.html/ HTTP/1.1\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(r.status, 404);
    let r = send(
        a,
        b"GET /static/sub/ HTTP/1.1\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(r.status, 404, "sub/ has no index.html");
    let r = send(
        a,
        b"GET /static/ HTTP/1.1\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(r.status, 200);

    srv.stop();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn raw_utf8_path_is_served_like_percent_encoded() {
    let (dir, srv) = fixture("utf8");
    let a = srv.local_addr();

    let raw =
        "GET /static/caf\u{e9}-\u{fc}n\u{ef}-\u{65e5}.txt HTTP/1.1\r\nConnection: close\r\n\r\n";
    let enc =
        "GET /static/caf%C3%A9-%C3%BCn%C3%AF-%E6%97%A5.txt HTTP/1.1\r\nConnection: close\r\n\r\n";
    let r1 = send(a, raw.as_bytes(), false);
    let r2 = send(a, enc.as_bytes(), false);
    assert_eq!(r1.status, 200);
    assert_eq!(r1.body, b"utf8 ok");
    assert_eq!(r2.status, r1.status);
    assert_eq!(r2.body, r1.body);

    srv.stop();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn not_found_has_text_body_with_escaped_path() {
    let (dir, srv) = fixture("body");
    let a = srv.local_addr();

    let r = send(
        a,
        b"GET /static/missing.html HTTP/1.1\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(r.status, 404);
    assert_eq!(r.body, b"404 not found: /static/missing.html\n");
    assert_eq!(
        r.header("content-type"),
        Some(&b"text/plain; charset=utf-8"[..])
    );
    assert_eq!(r.header("content-length"), Some(&b"36"[..]));

    // A raw C1 control character (U+0085, bytes C2 85) is escaped, not sent as-is.
    let r = send(
        a,
        b"GET /static/a\xc2\x85b HTTP/1.1\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(r.status, 404);
    assert_eq!(r.body, b"404 not found: /static/a\\x85b\n");

    // Unrouted paths get the same body.
    let r = send(a, b"GET /nope HTTP/1.1\r\nConnection: close\r\n\r\n", false);
    assert_eq!(r.status, 404);
    assert_eq!(r.body, b"404 not found: /nope\n");

    srv.stop();
    let _ = fs::remove_dir_all(&dir);
}
