#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used)]
//! `athena serve` over a real Mercury server on an ephemeral 127.0.0.1 port.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use athena::serve::{start, Served};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("athena_serve_{tag}_{}_{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(fs::canonicalize(&dir).unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One request on a fresh connection. Returns the status code and the body text.
fn send(
    port: u16,
    method: &str,
    target: &str,
    host: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut head = format!(
        "{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status: u16 = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    let payload = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, payload)
}

struct Rig {
    root: TempDir,
    web: TempDir,
    served: Served,
    port: u16,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let root = TempDir::new(&format!("{tag}_root"));
        let web = TempDir::new(&format!("{tag}_web"));
        fs::write(web.0.join("index.html"), "<!doctype html>athena-index").unwrap();
        let served = start(&root.0, &web.0, 0).unwrap();
        let port: u16 = served
            .url
            .trim_start_matches("http://127.0.0.1:")
            .split('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        Rig {
            root,
            web,
            served,
            port,
        }
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Call the API with the right token and the local host name.
    fn api(&self, method: &str, target: &str, body: &[u8]) -> (u16, String) {
        let token = self.served.token.clone();
        send(
            self.port,
            method,
            target,
            &self.host(),
            &[("X-Athena-Token", &token)],
            body,
        )
    }
}

#[test]
fn url_carries_a_token_and_the_page_is_served_without_one() {
    let rig = Rig::new("static");
    assert!(rig.served.url.contains(&format!("?t={}", rig.served.token)));
    assert_eq!(rig.served.token.len(), 64);
    let (status, body) = send(rig.port, "GET", "/", &rig.host(), &[], b"");
    assert_eq!(status, 200);
    assert!(body.contains("athena-index"));
    rig.served.server.stop();
}

#[test]
fn api_rejects_missing_or_wrong_token_with_401() {
    let rig = Rig::new("token");
    fs::write(rig.root.0.join("a.logos"), "fn main() {}\n").unwrap();
    let host = rig.host();
    let (status, _) = send(rig.port, "GET", "/api/fs/list?path=", &host, &[], b"");
    assert_eq!(status, 401);
    let (status, _) = send(
        rig.port,
        "GET",
        "/api/fs/list?path=",
        &host,
        &[("X-Athena-Token", "nope")],
        b"",
    );
    assert_eq!(status, 401);
    let (status, _) = send(
        rig.port,
        "PUT",
        "/api/fs/write?path=evil.txt",
        &host,
        &[],
        b"owned",
    );
    assert_eq!(status, 401);
    assert!(!rig.root.0.join("evil.txt").exists());
    let (status, body) = rig.api("GET", "/api/fs/list?path=", b"");
    assert_eq!(status, 200);
    assert!(body.contains("\"name\":\"a.logos\""));
    rig.served.server.stop();
}

#[test]
fn api_rejects_non_local_host_names() {
    let rig = Rig::new("host");
    let token = rig.served.token.clone();
    let (status, _) = send(
        rig.port,
        "GET",
        "/api/fs/list?path=",
        "evil.example",
        &[("X-Athena-Token", &token)],
        b"",
    );
    assert_eq!(status, 403);
    rig.served.server.stop();
}

#[test]
fn escaping_paths_are_refused_with_403_and_write_nothing() {
    let rig = Rig::new("escape");
    let outside = rig
        .root
        .0
        .parent()
        .unwrap()
        .join(format!("athena_serve_outside_{}.txt", std::process::id()));
    for target in [
        "/api/fs/list?path=..",
        "/api/fs/list?path=..%2F",
        "/api/fs/list?path=%2Fetc",
        "/api/fs/list?path=C%3A%5CWindows",
        "/api/fs/read?path=..%5Cx.txt",
        "/api/fs/list?path=a%00b",
    ] {
        let (status, _) = rig.api("GET", target, b"");
        assert_eq!(status, 403, "{target}");
    }
    let target = format!(
        "/api/fs/write?path=..%2F{}",
        outside.file_name().unwrap().to_string_lossy()
    );
    let (status, _) = rig.api("PUT", &target, b"x");
    assert_eq!(status, 403);
    assert!(!outside.exists());
    rig.served.server.stop();
}

#[test]
fn symlink_out_of_the_folder_is_refused_over_http() {
    let rig = Rig::new("link");
    let secret = rig.web.0.join("secret.txt");
    fs::write(&secret, "outside").unwrap();
    let leak = rig.root.0.join("leak.txt");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&secret, &leak).unwrap();
    }
    #[cfg(windows)]
    {
        // Creating links may need a privilege some accounts lack; the unit tests cover the rule.
        if std::os::windows::fs::symlink_file(&secret, &leak).is_err() {
            rig.served.server.stop();
            return;
        }
    }
    let (status, _) = rig.api("GET", "/api/fs/read?path=leak.txt", b"");
    assert_eq!(status, 403);
    rig.served.server.stop();
}

#[test]
fn read_write_create_rename_trash_round_trip() {
    let rig = Rig::new("trip");
    let (status, _) = rig.api("POST", "/api/fs/create?path=notes&kind=dir", b"");
    assert_eq!(status, 200);
    let (status, _) = rig.api(
        "PUT",
        "/api/fs/write?path=notes%2Fa.txt",
        "hello \u{00e9}".as_bytes(),
    );
    assert_eq!(status, 200);
    let (status, body) = rig.api("GET", "/api/fs/read?path=notes%2Fa.txt", b"");
    assert_eq!((status, body.as_str()), (200, "hello \u{00e9}"));

    let (status, _) = rig.api(
        "POST",
        "/api/fs/rename?from=notes%2Fa.txt&to=notes%2Fb.txt",
        b"",
    );
    assert_eq!(status, 200);
    let (_, listing) = rig.api("GET", "/api/fs/list?path=notes", b"");
    assert!(
        listing.contains("b.txt") && !listing.contains("a.txt"),
        "{listing}"
    );

    let (status, _) = rig.api("POST", "/api/fs/create?path=notes%2Fb.txt&kind=file", b"");
    assert_eq!(status, 409);

    let (status, body) = rig.api("POST", "/api/fs/trash?path=notes%2Fb.txt", b"");
    assert_eq!(status, 200);
    assert!(body.contains("trashed"));
    assert!(!rig.root.0.join("notes").join("b.txt").exists());
    let trashed = fs::read_dir(rig.root.0.join(".athena-trash"))
        .unwrap()
        .count();
    assert_eq!(trashed, 1);
    let (_, listing) = rig.api("GET", "/api/fs/list?path=", b"");
    assert!(
        !listing.contains("athena-trash"),
        "trash must stay hidden: {listing}"
    );
    rig.served.server.stop();
}

#[test]
fn binary_files_are_refused_for_reading() {
    let rig = Rig::new("binary");
    fs::write(rig.root.0.join("img.bin"), [0xff_u8, 0xd8, 0x00]).unwrap();
    let (status, _) = rig.api("GET", "/api/fs/read?path=img.bin", b"");
    assert_eq!(status, 415);
    rig.served.server.stop();
}

#[test]
fn start_refuses_a_missing_web_app_and_a_missing_folder() {
    let root = TempDir::new("nowebapp");
    let empty = TempDir::new("emptyweb");
    let err = start(&root.0, &empty.0, 0).err().unwrap();
    assert!(err.contains("no web app"), "{err}");
    let err = start(&root.0.join("missing"), &empty.0, 0).err().unwrap();
    assert!(err.contains("missing"), "{err}");
}
