#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use assay::forall;
use mercury::server::{Config, Router, Server};

use super::http::{http_post, http_post_auth};
use super::pack::{decode_pack, encode_pack, MAX_PACK_OBJECTS};
use super::{clone, serve, RemoteError};

const TEST_TOKEN: &str = "test-push-token";
use crate::{to_hex, Kind, MemStore, Repo, Signature};

fn sig(name: &str, time: i64) -> Signature {
    Signature {
        name: name.to_string(),
        email: format!("{name}@example.com"),
        time,
        tz_minutes: 0,
    }
}

fn start_server(repo: Arc<Mutex<Repo<MemStore>>>, prefix: &str) -> (Server, String) {
    let mut router = Router::new();
    serve(repo, &mut router, prefix, Some(TEST_TOKEN));
    let cfg = Config {
        addr: "127.0.0.1:0".to_string(),
        threads: 8,
        ..Config::default()
    };
    let srv = Server::start(cfg, router).unwrap();
    let addr = srv.local_addr();
    let url = if prefix.is_empty() || prefix == "/" {
        format!("http://{addr}")
    } else {
        format!("http://{addr}/{}", prefix.trim_matches('/'))
    };
    (srv, url)
}

#[test]
fn pack_roundtrip_forall_300() {
    forall(300, 0xD121_7045, |rng| {
        let n = rng.below(16);
        let mut objs = Vec::with_capacity(n);
        for _ in 0..n {
            let kind = *rng.pick(&[Kind::Blob, Kind::Tree, Kind::Commit]);
            let data = rng.bytes(256);
            objs.push((kind, data));
        }
        let packed = encode_pack(&objs);
        let decoded = decode_pack(&packed, 1000).unwrap();
        assert_eq!(decoded, objs);
    });
}

#[test]
fn pack_corruptions_never_panic() {
    let objs = vec![
        (Kind::Blob, b"hello".to_vec()),
        (Kind::Tree, vec![1, 2, 3, 4]),
        (Kind::Commit, b"commit data".to_vec()),
    ];
    let packed = encode_pack(&objs);

    // Every single bit flip must return Err(Protocol).
    for byte_idx in 0..packed.len() {
        let mut bad = packed.clone();
        bad[byte_idx] ^= 0x01;
        assert_eq!(decode_pack(&bad, 100), Err(RemoteError::Protocol));
    }

    // Every truncation must return Err(Protocol).
    for len in 0..packed.len() {
        assert_eq!(decode_pack(&packed[..len], 100), Err(RemoteError::Protocol));
    }

    // Wrong count (claiming 0xFFFF_FFFF objects with valid hash).
    let mut huge_count = Vec::new();
    huge_count.extend_from_slice(b"DPK1");
    huge_count.extend_from_slice(&u32::MAX.to_be_bytes());
    let mut h = sphinx::sha256::Sha256::new();
    h.update(&huge_count);
    huge_count.extend_from_slice(&h.finalize());
    assert_eq!(
        decode_pack(&huge_count, usize::MAX),
        Err(RemoteError::Protocol)
    );

    // Huge length field with valid hash.
    let mut huge_len = Vec::new();
    huge_len.extend_from_slice(b"DPK1");
    huge_len.extend_from_slice(&1u32.to_be_bytes());
    huge_len.push(1);
    huge_len.extend_from_slice(&u32::MAX.to_be_bytes());
    huge_len.extend_from_slice(b"abc");
    let mut h2 = sphinx::sha256::Sha256::new();
    h2.update(&huge_len);
    huge_len.extend_from_slice(&h2.finalize());
    assert_eq!(decode_pack(&huge_len, 100), Err(RemoteError::Protocol));

    // Max objects exceeded.
    assert_eq!(decode_pack(&packed, 2), Err(RemoteError::Protocol));
}

#[test]
fn clone_pull_merge_push_and_sync() {
    let mut server_repo = Repo::init(MemStore::new());
    server_repo.add("a.txt", b"base a\n", false).unwrap();
    server_repo.add("b.txt", b"base b\n", false).unwrap();
    let base_id = server_repo.commit("init", &sig("srv", 10)).unwrap();

    let shared = Arc::new(Mutex::new(server_repo));
    let (srv, url) = start_server(Arc::clone(&shared), "/repo");

    let mut client1 = clone(&url, MemStore::new()).unwrap();
    assert_eq!(client1.head(), Some(base_id));
    assert_eq!(client1.staged().get("a.txt").unwrap().0, b"base a\n");

    // Commit on server side.
    {
        let mut g = shared.lock().unwrap();
        g.add("b.txt", b"server b\n", false).unwrap();
        g.commit("server update", &sig("srv", 20)).unwrap();
    }

    // Commit on client1 side (different file -> clean 3-way merge on pull).
    client1.add("a.txt", b"client a\n", false).unwrap();
    client1.commit("client update", &sig("c1", 30)).unwrap();

    let merge_id = client1.pull(&url, "main", &sig("c1", 40)).unwrap();
    let files = client1.files(&merge_id).unwrap();
    assert_eq!(files.get("a.txt").unwrap().0, b"client a\n");
    assert_eq!(files.get("b.txt").unwrap().0, b"server b\n");

    // Push merged result back to server.
    client1
        .push_with_token(&url, "main", Some(TEST_TOKEN))
        .unwrap();
    let srv_head = shared.lock().unwrap().head();
    assert_eq!(srv_head, Some(merge_id));

    srv.stop();
}

#[test]
fn push_rejected_stale_non_ff_and_bad_name() {
    let mut server_repo = Repo::init(MemStore::new());
    server_repo.add("f.txt", b"v1\n", false).unwrap();
    let c1 = server_repo.commit("c1", &sig("srv", 10)).unwrap();
    server_repo.add("f.txt", b"v2\n", false).unwrap();
    let c2 = server_repo.commit("c2", &sig("srv", 20)).unwrap();

    let shared = Arc::new(Mutex::new(server_repo));
    let (srv, url) = start_server(Arc::clone(&shared), "/repo");

    // Client1 clones at c2, then server advances to c3 -> client1 push is stale.
    let mut client1 = clone(&url, MemStore::new()).unwrap();
    {
        let mut g = shared.lock().unwrap();
        g.add("f.txt", b"v3\n", false).unwrap();
        g.commit("c3", &sig("srv", 30)).unwrap();
    }
    client1.add("g.txt", b"diverge\n", false).unwrap();
    let _c_div = client1.commit("diverge", &sig("c1", 40)).unwrap();

    // Manually craft a stale push (old = c1, new = _c_div) to test server stale check.
    let stale_body = format!("{} {} main\n\n", to_hex(&c1), to_hex(&_c_div));
    let mut stale_req = stale_body.into_bytes();
    stale_req.extend_from_slice(&encode_pack(&[]));
    let resp = http_post_auth(&url, "/push", stale_req, Some(TEST_TOKEN)).unwrap();
    assert_eq!(String::from_utf8(resp).unwrap(), "ng main stale\n");

    // Non-fast-forward: old = c3 (matches current), new = c1 (ancestor, not descendant).
    let c3 = shared.lock().unwrap().head().unwrap();
    let nff_body = format!("{} {} main\n\n", to_hex(&c3), to_hex(&c1));
    let mut nff_req = nff_body.into_bytes();
    nff_req.extend_from_slice(&encode_pack(&[]));
    let resp = http_post_auth(&url, "/push", nff_req, Some(TEST_TOKEN)).unwrap();
    assert_eq!(
        String::from_utf8(resp).unwrap(),
        "ng main non-fast-forward\n"
    );
    assert_eq!(shared.lock().unwrap().head(), Some(c3));

    // Bad branch names.
    for bad in ["../evil", "/lead", "trail/", "a b", "x\x01y"] {
        let bad_body = format!("{} {} {}\n\n", to_hex(&[0u8; 32]), to_hex(&c2), bad);
        let mut bad_req = bad_body.into_bytes();
        bad_req.extend_from_slice(&encode_pack(&[]));
        let resp = http_post_auth(&url, "/push", bad_req, Some(TEST_TOKEN)).unwrap();
        assert_eq!(
            String::from_utf8(resp).unwrap(),
            format!("ng {bad} bad-name\n")
        );
    }

    // Push a brand new branch.
    client1.branch("feature", &_c_div).unwrap();
    client1
        .push_with_token(&url, "feature", Some(TEST_TOKEN))
        .unwrap();
    let branches = shared.lock().unwrap().branches();
    assert!(branches
        .iter()
        .any(|(b, id)| b == "feature" && *id == _c_div));

    srv.stop();
}

#[test]
fn fetch_with_haves_returns_only_new_objects() {
    let mut server_repo = Repo::init(MemStore::new());
    server_repo
        .add("shared.txt", b"unchanged\n", false)
        .unwrap();
    server_repo.add("changing.txt", b"v1\n", false).unwrap();
    let c1 = server_repo.commit("c1", &sig("srv", 10)).unwrap();

    let shared = Arc::new(Mutex::new(server_repo));
    let (srv, url) = start_server(Arc::clone(&shared), "/repo");

    let client = clone(&url, MemStore::new()).unwrap();
    assert_eq!(client.head(), Some(c1));

    // Add c2 on server modifying only changing.txt (shared.txt blob is reused).
    let c2 = {
        let mut g = shared.lock().unwrap();
        g.add("changing.txt", b"v2\n", false).unwrap();
        g.commit("c2", &sig("srv", 20)).unwrap()
    };

    // Raw POST /fetch with want=c2, have=c1: should return ONLY 3 objects (c2 commit, new tree, new blob).
    let body = format!("want {}\nhave {}\n", to_hex(&c2), to_hex(&c1));
    let pack = http_post(&url, "/fetch", body.into_bytes()).unwrap();
    let objs = decode_pack(&pack, MAX_PACK_OBJECTS).unwrap();
    assert_eq!(objs.len(), 3);

    srv.stop();
}

#[test]
fn malformed_push_and_oversized_count_rejected() {
    let server_repo = Repo::init(MemStore::new());
    let shared = Arc::new(Mutex::new(server_repo));
    let (srv, url) = start_server(Arc::clone(&shared), "/repo");

    // Malformed body (no empty line separator) -> 400.
    assert_eq!(
        http_post_auth(
            &url,
            "/push",
            b"not-a-valid-push".to_vec(),
            Some(TEST_TOKEN)
        ),
        Err(RemoteError::Http(400))
    );

    // Over 100_000 objects -> 413.
    let mut big_pack = Vec::new();
    big_pack.extend_from_slice(b"DPK1");
    big_pack.extend_from_slice(&100_001u32.to_be_bytes());
    let header = format!("{} {} main\n\n", to_hex(&[0u8; 32]), to_hex(&[1u8; 32]));
    let mut req = header.into_bytes();
    req.extend_from_slice(&big_pack);
    assert_eq!(
        http_post_auth(&url, "/push", req, Some(TEST_TOKEN)),
        Err(RemoteError::Http(413))
    );

    srv.stop();
}

#[test]
fn concurrent_eight_threads_push() {
    let mut server_repo = Repo::init(MemStore::new());
    server_repo.add("base.txt", b"base\n", false).unwrap();
    let base_id = server_repo.commit("base", &sig("srv", 10)).unwrap();

    let shared = Arc::new(Mutex::new(server_repo));
    let (srv, url) = start_server(Arc::clone(&shared), "/repo");

    let mut handles = Vec::new();
    // every thread clones the base before any pushes, so exactly one push can fast-forward
    let gate = Arc::new(std::sync::Barrier::new(8));
    for i in 0..8 {
        let u = url.clone();
        let gate = Arc::clone(&gate);
        handles.push(std::thread::spawn(move || {
            let mut c = clone(&u, MemStore::new()).unwrap();
            c.add("base.txt", format!("thread {i}\n").as_bytes(), false)
                .unwrap();
            c.commit(&format!("commit {i}"), &sig("t", 20 + i)).unwrap();
            gate.wait();
            c.push_with_token(&u, "main", Some(TEST_TOKEN))
        }));
    }

    let mut ok_count = 0usize;
    let mut rejected_count = 0usize;
    for h in handles {
        match h.join().unwrap() {
            Ok(()) => ok_count += 1,
            Err(RemoteError::Rejected(r)) => {
                assert!(r == "stale" || r == "non-fast-forward");
                rejected_count += 1;
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert_eq!(ok_count, 1);
    assert_eq!(rejected_count, 7);

    let g = shared.lock().unwrap();
    let head = g.head().unwrap();
    let log = g.log(&head).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[1], base_id);

    srv.stop();
}
