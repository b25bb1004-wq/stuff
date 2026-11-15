//! Security tests for drillcore remote protocol and object id parsing.

#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use drillcore::remote::{clone, decode_pack, encode_pack, serve, RemoteError};
use drillcore::{
    encode_commit, encode_tree, from_hex, hash_object, to_hex, Commit, DirStore, Kind, MemStore,
    Mode, Repo, Signature, Store, TreeEntry,
};
use onyx::sha256::Sha256;
use outcrop::server::{Config, Router, Server};
use outcrop::{parse_response, write_request, Limits, Request};

fn sig(name: &str, time: i64) -> Signature {
    Signature {
        name: name.to_string(),
        email: format!("{name}@example.com"),
        time,
        tz_minutes: 0,
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("drillcore_sec_{label}_{pid}_{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start_server<S: Store + Send + 'static>(repo: Arc<Mutex<Repo<S>>>) -> (Server, String) {
    let mut router = Router::new();
    serve(repo, &mut router, "/repo");
    let cfg = Config {
        addr: "127.0.0.1:0".to_string(),
        threads: 4,
        ..Config::default()
    };
    let srv = Server::start(cfg, router).unwrap();
    let addr = srv.local_addr();
    (srv, format!("{addr}"))
}

fn raw_http_post(addr: &str, path: &str, body: Vec<u8>) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut headers = vec![
        ("Host".to_string(), addr.as_bytes().to_vec()),
        ("Connection".to_string(), b"close".to_vec()),
    ];
    if body.is_empty() {
        headers.push(("Content-Length".to_string(), b"0".to_vec()));
    }
    let req = Request {
        method: "POST".to_string(),
        target: format!("/repo{path}"),
        minor: 1,
        headers,
        body,
    };
    stream.write_all(&write_request(&req)).unwrap();
    stream.flush().unwrap();

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let lim = Limits {
        max_head: 16 * 1024,
        max_headers: 100,
        max_body: 66 * 1024 * 1024,
    };
    let (resp, _) = parse_response(&buf, &lim, false).unwrap();
    (resp.status, resp.body)
}

fn sign_pack(payload: &[u8]) -> Vec<u8> {
    let mut out = payload.to_vec();
    let mut h = Sha256::new();
    h.update(payload);
    out.extend_from_slice(&h.finalize());
    out
}

#[test]
fn push_rejects_dot_and_dotlock_ref_components() {
    let dir = TempDirGuard(temp_dir("dot_ref"));
    let mut repo = Repo::init(DirStore::new(&dir.0));
    repo.add("f.txt", b"hello\n", false).unwrap();
    let c1 = repo.commit("init", &sig("srv", 1)).unwrap();

    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(Arc::clone(&shared));

    for bad in ["a/.", "a/./b", "a/.hidden", "a/.lock/b", "a.lock/b"] {
        let header = format!("{} {} {bad}\n\n", to_hex(&[0u8; 32]), to_hex(&c1));
        let mut req = header.into_bytes();
        req.extend_from_slice(&encode_pack(&[]));
        let (status, resp) = raw_http_post(&addr, "/push", req);
        assert_eq!(status, 200);
        assert_eq!(
            String::from_utf8(resp).unwrap(),
            format!("ng {bad} bad-name\n"),
            "branch {bad:?} must be rejected as bad-name"
        );
    }

    srv.stop();
}

#[test]
fn push_rejects_tree_entry_kind_mismatch_on_reused_id() {
    let repo = Repo::init(MemStore::new());
    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(Arc::clone(&shared));

    let empty_tree = encode_tree(&[]);
    let empty_tree_id = hash_object(Kind::Tree, &empty_tree);

    // Same object id referenced as both Dir (Tree) and File (Blob) in one tree.
    let mixed_tree = encode_tree(&[
        TreeEntry {
            mode: Mode::Dir,
            name: "as_dir".to_string(),
            id: empty_tree_id,
        },
        TreeEntry {
            mode: Mode::File,
            name: "as_file".to_string(),
            id: empty_tree_id,
        },
    ]);
    let mixed_tree_id = hash_object(Kind::Tree, &mixed_tree);

    let commit = encode_commit(&Commit {
        tree: mixed_tree_id,
        parents: Vec::new(),
        author: sig("att", 10),
        committer: sig("att", 10),
        message: "kind confusion".to_string(),
    });
    let commit_id = hash_object(Kind::Commit, &commit);

    let pack = encode_pack(&[
        (Kind::Tree, empty_tree),
        (Kind::Tree, mixed_tree),
        (Kind::Commit, commit),
    ]);
    let header = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&commit_id));
    let mut req = header.into_bytes();
    req.extend_from_slice(&pack);

    let (status, resp) = raw_http_post(&addr, "/push", req);
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(resp).unwrap(), "ng main missing\n");
    assert!(shared.lock().unwrap().branches().is_empty());

    srv.stop();
}

#[test]
fn push_rejects_tree_with_dot_or_dotdot_entry() {
    let repo = Repo::init(MemStore::new());
    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(Arc::clone(&shared));

    let blob = b"pwned\n".to_vec();
    let blob_id = hash_object(Kind::Blob, &blob);

    for bad_entry in ["..", "."] {
        let evil_tree = encode_tree(&[TreeEntry {
            mode: Mode::File,
            name: bad_entry.to_string(),
            id: blob_id,
        }]);
        let evil_tree_id = hash_object(Kind::Tree, &evil_tree);

        let commit = encode_commit(&Commit {
            tree: evil_tree_id,
            parents: Vec::new(),
            author: sig("att", 10),
            committer: sig("att", 10),
            message: "dotdot tree".to_string(),
        });
        let commit_id = hash_object(Kind::Commit, &commit);

        let pack = encode_pack(&[
            (Kind::Blob, blob.clone()),
            (Kind::Tree, evil_tree),
            (Kind::Commit, commit),
        ]);
        let header = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&commit_id));
        let mut req = header.into_bytes();
        req.extend_from_slice(&pack);

        let (status, resp) = raw_http_post(&addr, "/push", req);
        assert_eq!(status, 200);
        assert_eq!(String::from_utf8(resp).unwrap(), "ng main missing\n");
    }

    srv.stop();
}

#[test]
fn pack_decode_rejects_hostile_counts_and_lengths() {
    // 1. Huge count claiming u32::MAX objects with valid SHA-256 trailer and max_objects=MAX.
    let mut payload = Vec::new();
    payload.extend_from_slice(b"DPK1");
    payload.extend_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        decode_pack(&sign_pack(&payload), usize::MAX),
        Err(RemoteError::Protocol)
    );

    // 2. Entry claiming 4 GiB length on a tiny payload (must not allocate or panic).
    let mut huge_len = Vec::new();
    huge_len.extend_from_slice(b"DPK1");
    huge_len.extend_from_slice(&1u32.to_be_bytes());
    huge_len.push(1);
    huge_len.extend_from_slice(&u32::MAX.to_be_bytes());
    huge_len.extend_from_slice(b"tiny");
    assert_eq!(
        decode_pack(&sign_pack(&huge_len), 100),
        Err(RemoteError::Protocol)
    );

    // 3. Invalid kind byte (0, 4, 255).
    for bad_kind in [0u8, 4u8, 255u8] {
        let mut p = Vec::new();
        p.extend_from_slice(b"DPK1");
        p.extend_from_slice(&1u32.to_be_bytes());
        p.push(bad_kind);
        p.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(decode_pack(&sign_pack(&p), 100), Err(RemoteError::Protocol));
    }

    // 4. Trailing bytes inside signed payload after declared object count.
    let mut trailing = Vec::new();
    trailing.extend_from_slice(b"DPK1");
    trailing.extend_from_slice(&0u32.to_be_bytes());
    trailing.extend_from_slice(b"extra");
    assert_eq!(
        decode_pack(&sign_pack(&trailing), 100),
        Err(RemoteError::Protocol)
    );

    // 5. Wrong magic and truncated packs.
    assert_eq!(
        decode_pack(&sign_pack(b"DPK2\x00\x00\x00\x00"), 100),
        Err(RemoteError::Protocol)
    );
    let valid = encode_pack(&[(Kind::Blob, b"abc".to_vec())]);
    for n in 0..valid.len() {
        assert_eq!(decode_pack(&valid[..n], 100), Err(RemoteError::Protocol));
    }
}

#[test]
fn pack_decode_rejects_corrupted_sha256_trailer() {
    let pack = encode_pack(&[
        (Kind::Blob, b"hello".to_vec()),
        (Kind::Tree, encode_tree(&[])),
    ]);
    for idx in 0..pack.len() {
        let mut bad = pack.clone();
        bad[idx] ^= 0x80;
        assert_eq!(decode_pack(&bad, 100), Err(RemoteError::Protocol));
    }
}

#[test]
fn serve_push_rejects_oversized_pack_and_object_count() {
    let repo = Repo::init(MemStore::new());
    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(shared);

    // Claiming 100_001 objects -> 413 Payload Too Large.
    let header = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&[1u8; 32]));
    let mut req = header.into_bytes();
    req.extend_from_slice(b"DPK1");
    req.extend_from_slice(&100_001u32.to_be_bytes());
    let (status, _) = raw_http_post(&addr, "/push", req);
    assert_eq!(status, 413);

    // Malformed / truncated push body -> 400 Bad Request.
    let (status2, _) = raw_http_post(&addr, "/push", b"bad header\n".to_vec());
    assert_eq!(status2, 400);

    srv.stop();
}

#[test]
fn serve_push_rejects_missing_objects_and_wrong_root_kind() {
    let repo = Repo::init(MemStore::new());
    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(Arc::clone(&shared));

    // 1. Blob id pushed as branch tip -> ng main missing.
    let blob = b"not a commit".to_vec();
    let blob_id = hash_object(Kind::Blob, &blob);
    let pack1 = encode_pack(&[(Kind::Blob, blob)]);
    let mut req1 = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&blob_id)).into_bytes();
    req1.extend_from_slice(&pack1);
    let (s1, r1) = raw_http_post(&addr, "/push", req1);
    assert_eq!(s1, 200);
    assert_eq!(String::from_utf8(r1).unwrap(), "ng main missing\n");

    // 2. Commit referencing missing tree -> ng main missing.
    let c_missing_tree = encode_commit(&Commit {
        tree: [0x44u8; 32],
        parents: Vec::new(),
        author: sig("a", 1),
        committer: sig("a", 1),
        message: "m".to_string(),
    });
    let c_id = hash_object(Kind::Commit, &c_missing_tree);
    let pack2 = encode_pack(&[(Kind::Commit, c_missing_tree)]);
    let mut req2 = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&c_id)).into_bytes();
    req2.extend_from_slice(&pack2);
    let (s2, r2) = raw_http_post(&addr, "/push", req2);
    assert_eq!(s2, 200);
    assert_eq!(String::from_utf8(r2).unwrap(), "ng main missing\n");

    // 3. Commit with missing parent -> ng main missing.
    let t = encode_tree(&[]);
    let t_id = hash_object(Kind::Tree, &t);
    let c_missing_parent = encode_commit(&Commit {
        tree: t_id,
        parents: vec![[0x55u8; 32]],
        author: sig("a", 1),
        committer: sig("a", 1),
        message: "m".to_string(),
    });
    let c_p_id = hash_object(Kind::Commit, &c_missing_parent);
    let pack3 = encode_pack(&[(Kind::Tree, t), (Kind::Commit, c_missing_parent)]);
    let mut req3 = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&c_p_id)).into_bytes();
    req3.extend_from_slice(&pack3);
    let (s3, r3) = raw_http_post(&addr, "/push", req3);
    assert_eq!(s3, 200);
    assert_eq!(String::from_utf8(r3).unwrap(), "ng main missing\n");

    srv.stop();
}

#[test]
fn serve_fetch_rejects_malformed_requests_and_missing_wants() {
    let mut repo = Repo::init(MemStore::new());
    repo.add("a.txt", b"ok\n", false).unwrap();
    let _c1 = repo.commit("c1", &sig("srv", 1)).unwrap();

    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(shared);

    // Non-UTF8 body -> 400.
    let (s1, _) = raw_http_post(&addr, "/fetch", vec![0xff, 0xfe, 0xfd]);
    assert_eq!(s1, 400);

    // Unknown command -> 400.
    let (s2, _) = raw_http_post(&addr, "/fetch", b"evil 0000\n".to_vec());
    assert_eq!(s2, 400);

    // Non-existent want id -> 400.
    let (s3, _) = raw_http_post(
        &addr,
        "/fetch",
        format!("want {}\n", to_hex(&[0x99u8; 32])).into_bytes(),
    );
    assert_eq!(s3, 400);

    // Non-hex want id -> 400.
    let (s4, _) = raw_http_post(
        &addr,
        "/fetch",
        format!("want {}\n", "z".repeat(64)).into_bytes(),
    );
    assert_eq!(s4, 400);

    srv.stop();
}

#[test]
fn serve_push_rejects_traversal_and_absolute_ref_names() {
    let mut repo = Repo::init(MemStore::new());
    repo.add("f.txt", b"v1\n", false).unwrap();
    let c1 = repo.commit("c1", &sig("srv", 1)).unwrap();

    let shared = Arc::new(Mutex::new(repo));
    let (srv, addr) = start_server(shared);

    let long_name = "a".repeat(201);
    let bad_names = [
        "../escape",
        "a/../b",
        "/abs/path",
        "\\win\\path",
        "a//b",
        "-dash",
        ".dot",
        "main.lock",
        "has space",
        "has\0nul",
        long_name.as_str(),
    ];

    for bad in bad_names {
        let mut req = format!("{} {} {bad}\n\n", to_hex(&[0u8; 32]), to_hex(&c1)).into_bytes();
        req.extend_from_slice(&encode_pack(&[]));
        let (status, resp) = raw_http_post(&addr, "/push", req);
        assert_eq!(status, 200);
        assert_eq!(
            String::from_utf8(resp).unwrap(),
            format!("ng {bad} bad-name\n")
        );
    }

    srv.stop();
}

#[test]
fn clone_ignores_hostile_remote_ref_names() {
    let mut srv_repo = Repo::init(MemStore::new());
    srv_repo.add("hello.txt", b"world\n", false).unwrap();
    let c1 = srv_repo.commit("init", &sig("srv", 1)).unwrap();
    let c1_hex = to_hex(&c1);
    let pack = encode_pack(&[
        (Kind::Blob, b"world\n".to_vec()),
        (
            Kind::Tree,
            srv_repo
                .get(
                    &srv_repo
                        .files(&c1)
                        .is_ok()
                        .then(|| {
                            // Pull raw tree from store via commit decode
                            drillcore::decode_commit(&srv_repo.get(&c1).unwrap().1)
                                .unwrap()
                                .tree
                        })
                        .unwrap(),
                )
                .unwrap()
                .1,
        ),
        (Kind::Commit, srv_repo.get(&c1).unwrap().1),
    ]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 4096];
            let n = stream.read(&mut req_buf).unwrap();
            let req_text = String::from_utf8_lossy(&req_buf[..n]);
            let body = if req_text.starts_with("GET /refs ") {
                format!("{c1_hex} ../escaped\n{c1_hex} /abs\n{c1_hex} a/./b\n{c1_hex} main\n")
                    .into_bytes()
            } else {
                pack.clone()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
        }
    });

    let dir = TempDirGuard(temp_dir("clone_hostile_refs"));
    let cloned = clone(&format!("http://{addr}"), DirStore::new(&dir.0)).unwrap();
    handle.join().unwrap();

    let branches = cloned.branches();
    assert_eq!(branches, vec![("main".to_string(), c1)]);
    assert!(!dir.0.join("escaped").exists());
    assert!(!dir.0.join("refs/heads/a/b").exists());
}

#[test]
fn object_id_from_hex_rejects_wrong_length_and_non_hex() {
    assert_eq!(from_hex(""), None);
    assert_eq!(from_hex(&"a".repeat(63)), None);
    assert_eq!(from_hex(&"a".repeat(65)), None);
    assert_eq!(from_hex(&"a".repeat(128)), None);
    assert_eq!(from_hex(&"g".repeat(64)), None);
    assert_eq!(from_hex(&"G".repeat(64)), None);
    assert_eq!(from_hex(&format!("{} ", "0".repeat(63))), None);
    assert_eq!(from_hex(&format!("{}\0", "0".repeat(63))), None);
    // 64 bytes of 2-byte UTF-8 character 'é' (32 chars) and 64 chars (128 bytes).
    assert_eq!(from_hex(&"é".repeat(32)), None);
    assert_eq!(from_hex(&"é".repeat(64)), None);
}
