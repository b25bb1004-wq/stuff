//! Leader check for T45: three clones syncing to one file state; 8 racing pushers (winners only,
//! linear history, losers told stale/non-fast-forward, then all land by pull+push); a commit
//! pushed without its tree is refused with refs unchanged; malformed bodies get 400; fetch with a
//! have returns exactly the new objects; fuzzed bodies never break the server.
#![cfg(feature = "std")]
#![allow(clippy::unwrap_used)]
use assay::Rng;
use drillcore::remote::{clone, decode_pack, encode_pack, serve, RemoteError};
use drillcore::{decode_commit, encode_commit, to_hex, Commit, Kind, MemStore, Repo, Signature};
use outcrop::server::{Config, Router, Server};
use outcrop::{parse_response, Limits};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};

fn who(t: i64) -> Signature {
    Signature {
        name: "L".into(),
        email: "l@x".into(),
        time: t,
        tz_minutes: 0,
    }
}

type Shared = Arc<Mutex<Repo<MemStore>>>;

fn start() -> (Server, Shared, String) {
    let mut r = Repo::init(MemStore::default());
    r.add("README", b"strata\n", false).unwrap();
    r.commit("root\n", &who(1)).unwrap();
    let shared = Arc::new(Mutex::new(r));
    let mut router = Router::new();
    serve(Arc::clone(&shared), &mut router, "/repo");
    let srv = Server::start(Config::default(), router).unwrap();
    let url = format!("http://{}/repo", srv.local_addr());
    (srv, shared, url)
}

fn post(a: SocketAddr, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let (resp, _) = parse_response(&buf, &Limits::default(), false).unwrap();
    (resp.status, resp.body)
}

fn refs(sh: &Shared) -> Vec<(String, [u8; 32])> {
    sh.lock().unwrap().branches()
}

#[test]
fn leader_three_clones_converge() {
    let (srv, sh, url) = start();
    let mut a = clone(&url, MemStore::default()).unwrap();
    let mut b = clone(&url, MemStore::default()).unwrap();
    a.add("a.txt", b"from a\n", false).unwrap();
    a.add("README", b"strata\nedited by a\n", false).unwrap();
    a.commit("a\n", &who(10)).unwrap();
    a.push(&url, "main").unwrap();
    b.add("b.txt", b"from b\n", true).unwrap();
    b.commit("b\n", &who(11)).unwrap();
    match b.push(&url, "main") {
        Err(RemoteError::Rejected(r)) => {
            assert!(r.contains("non-fast-forward") || r.contains("stale"), "{r}")
        }
        other => panic!("expected rejection, got {other:?}"),
    }
    let before = refs(&sh);
    b.pull(&url, "main", &who(12)).unwrap();
    assert_eq!(refs(&sh), before, "pull must not move server refs");
    b.push(&url, "main").unwrap();
    a.pull(&url, "main", &who(13)).unwrap();
    let c = clone(&url, MemStore::default()).unwrap();
    let tip = sh.lock().unwrap().head().unwrap();
    for (name, r) in [("a", &a), ("b", &b), ("c", &c)] {
        assert_eq!(r.head(), Some(tip), "{name} head");
        assert_eq!(
            r.files(&tip).unwrap(),
            sh.lock().unwrap().files(&tip).unwrap()
        );
    }
    let files = c.files(&tip).unwrap();
    assert_eq!(files["README"].0, b"strata\nedited by a\n");
    assert_eq!(files["b.txt"], (b"from b\n".to_vec(), true));
    // New branch push, then a bad name.
    let mut a2 = a;
    a2.branch("feature/x", &tip).unwrap();
    a2.checkout("feature/x").unwrap();
    a2.add("f", b"f\n", false).unwrap();
    a2.commit("f\n", &who(14)).unwrap();
    a2.push(&url, "feature/x").unwrap();
    assert!(refs(&sh).iter().any(|(n, _)| n == "feature/x"));
    srv.stop();
}

#[test]
fn leader_racing_pushers() {
    let (srv, sh, url) = start();
    let n = 8;
    let barrier = Arc::new(std::sync::Barrier::new(n));
    let hs: Vec<_> = (0..n)
        .map(|i| {
            let (url, barrier) = (url.clone(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut r = clone(&url, MemStore::default()).unwrap();
                r.add(&format!("w{i}"), format!("{i}\n").as_bytes(), false)
                    .unwrap();
                r.commit(&format!("w{i}\n"), &who(100 + i as i64)).unwrap();
                barrier.wait();
                let first = r.push(&url, "main");
                let won = first.is_ok();
                if let Err(e) = &first {
                    match e {
                        RemoteError::Rejected(s)
                            if s.contains("stale") || s.contains("non-fast-forward") => {}
                        other => panic!("loser {i} got {other:?}"),
                    }
                }
                let mut tries = 0;
                let mut res = first;
                while res.is_err() {
                    tries += 1;
                    assert!(tries < 40, "worker {i} never landed");
                    r.pull(&url, "main", &who(200 + i as i64)).unwrap();
                    res = r.push(&url, "main");
                }
                won
            })
        })
        .collect();
    let winners = hs
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|w| *w)
        .count();
    assert!(winners >= 1);
    let repo = sh.lock().unwrap();
    let tip = repo.head().unwrap();
    let files = repo.files(&tip).unwrap();
    for i in 0..n {
        assert_eq!(
            files[format!("w{i}").as_str()].0,
            format!("{i}\n").into_bytes()
        );
    }
    // Every worker's commit is reachable from the tip (a loser's merge has its own commit as first
    // parent, so the first-parent chain alone may be shorter than n).
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![tip];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let (_, data) = repo.get(&id).unwrap();
        stack.extend(decode_commit(&data).unwrap().parents);
        assert!(seen.len() < 1000);
    }
    assert!(seen.len() > n, "history too short: {} commits", seen.len());
    drop(repo);
    srv.stop();
}

#[test]
fn leader_forged_and_malformed_pushes_rejected() {
    let (srv, sh, url) = start();
    let a = srv.local_addr();
    let before = refs(&sh);
    let tip = before[0].1;
    // A commit whose tree the server does not have, pushed alone.
    let forged = Commit {
        tree: [7u8; 32],
        parents: vec![tip],
        author: who(5),
        committer: who(5),
        message: "forged\n".into(),
    };
    let data = encode_commit(&forged);
    let id = drillcore::hash_object(Kind::Commit, &data);
    let mut body = format!("{} {} main\n\n", to_hex(&tip), to_hex(&id)).into_bytes();
    body.extend(encode_pack(&[(Kind::Commit, data.clone())]));
    let (st, out) = post(a, "/repo/push", &body);
    assert_eq!(st, 200);
    let out = String::from_utf8_lossy(&out).to_string();
    assert!(out.starts_with("ng main missing"), "{out}");
    assert_eq!(refs(&sh), before);
    // Ref pointing at an id that is in no pack at all.
    let mut body = format!("{} {} main\n\n", to_hex(&tip), to_hex(&[9u8; 32])).into_bytes();
    body.extend(encode_pack(&[]));
    let (_, out) = post(a, "/repo/push", &body);
    assert!(String::from_utf8_lossy(&out).starts_with("ng main missing"));
    // Bad names and stale old values.
    for (old, name, why) in [
        ("0".repeat(64), "../x", "bad-name"),
        ("0".repeat(64), "/lead", "bad-name"),
        ("0".repeat(64), "main", "stale"),
    ] {
        let mut body = format!("{old} {} {name}\n\n", to_hex(&tip)).into_bytes();
        body.extend(encode_pack(&[]));
        let (_, out) = post(a, "/repo/push", &body);
        let out = String::from_utf8_lossy(&out).to_string();
        assert!(
            out.starts_with(&format!("ng {name} {why}")),
            "{name}: {out}"
        );
    }
    assert_eq!(refs(&sh), before);
    // Malformed bodies -> 400, refs unchanged; fuzzed bodies never kill the server.
    assert_eq!(post(a, "/repo/push", b"garbage").0, 400);
    let mut bad_pack = format!("{} {} main\n\n", to_hex(&tip), to_hex(&tip)).into_bytes();
    let mut p = encode_pack(&[(Kind::Blob, b"x".to_vec())]);
    let last = p.len() - 1;
    p[last] ^= 1;
    bad_pack.extend(p);
    assert_eq!(post(a, "/repo/push", &bad_pack).0, 400);
    let mut r = Rng::new(0x45_f022);
    for _ in 0..300 {
        let mut b: Vec<u8> = if r.chance(1, 2) {
            format!("want {}\nhave ", to_hex(&tip)).into_bytes()
        } else {
            format!("{} {} main\n\nDPK1", to_hex(&tip), to_hex(&tip)).into_bytes()
        };
        b.extend((0..r.below(80)).map(|_| r.below(256) as u8));
        let path = if r.chance(1, 2) {
            "/repo/fetch"
        } else {
            "/repo/push"
        };
        let (st, _) = post(a, path, &b);
        assert!(st < 500, "{path} gave {st}");
    }
    assert_eq!(refs(&sh), before);
    let _ = clone(&url, MemStore::default()).unwrap();
    srv.stop();
}

#[test]
fn leader_fetch_sends_only_new_objects() {
    let (srv, sh, url) = start();
    let mut a = clone(&url, MemStore::default()).unwrap();
    let old = a.head().unwrap();
    a.add("dir/new.txt", b"new\n", false).unwrap();
    let new = a.commit("n\n", &who(20)).unwrap();
    a.push(&url, "main").unwrap();
    // commit + root tree + dir tree + blob = 4 new objects.
    let body = format!("want {}\nhave {}\n", to_hex(&new), to_hex(&old));
    let (st, pack) = post(srv.local_addr(), "/repo/fetch", body.as_bytes());
    assert_eq!(st, 200);
    let objs = decode_pack(&pack, 1000).unwrap();
    assert_eq!(
        objs.len(),
        4,
        "{:?}",
        objs.iter().map(|o| o.0).collect::<Vec<_>>()
    );
    let (st, pack) = post(
        srv.local_addr(),
        "/repo/fetch",
        format!("want {}\n", to_hex(&new)).as_bytes(),
    );
    assert_eq!(st, 200);
    assert_eq!(decode_pack(&pack, 1000).unwrap().len(), 7); // + root commit, root tree, README blob
    assert_eq!(sh.lock().unwrap().head(), Some(new));
    srv.stop();
}
