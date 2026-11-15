//! Leader check for T47 (argo packages, registry, install). Copied to
//! crates/argo/tests/registry_leader.rs.
#![allow(clippy::unwrap_used)]
use argo::client::{install, installed, remove, InstallError};
use argo::manifest::Manifest;
use argo::pkg::{self, Package, PkgError};
use argo::registry::Registry;
use argo::solve::{solve, Index, Solution};
use argo::version::{Req, Version};
use assay::Rng;
use mercury::server::{Config, Router, Server};
use mercury::{parse_response, write_request, Limits, Request, Response};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const SEED_A: [u8; 32] = [7; 32];
const SEED_B: [u8; 32] = [9; 32];

fn key(seed: &[u8; 32]) -> [u8; 32] {
    sphinx::ed25519::public_key(seed)
}

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn req(s: &str) -> Req {
    Req::parse(s).unwrap()
}

fn manifest(name: &str, ver: &str, deps: &[(&str, &str)]) -> Manifest {
    Manifest {
        name: name.to_string(),
        version: v(ver),
        dependencies: deps.iter().map(|(n, r)| (n.to_string(), req(r))).collect(),
    }
}

/// The leader's own encoder for the T47 body format.
fn my_body(m: &Manifest, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let text = m.to_string();
    let mut b = b"CAIRNPKG1\n".to_vec();
    b.extend((text.len() as u32).to_le_bytes());
    b.extend(text.as_bytes());
    b.extend((sorted.len() as u32).to_le_bytes());
    for (p, d) in &sorted {
        b.extend((p.len() as u16).to_le_bytes());
        b.extend(p.as_bytes());
        b.extend((d.len() as u32).to_le_bytes());
        b.extend(d);
    }
    b
}

fn my_blob(seed: &[u8; 32], body: &[u8]) -> Vec<u8> {
    let mut b = b"CAIRNSIG1\n".to_vec();
    b.extend(key(seed));
    b.extend(sphinx::ed25519::sign(seed, body));
    b.extend(body);
    b
}

fn files_for(r: &mut Rng, name: &str) -> Vec<(String, Vec<u8>)> {
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    out.insert(format!("bin/{name}"), r.bytes(300));
    for i in 0..r.below(5) {
        let dir = ["lib", "share/doc", "etc", "a/b/c"][r.below(4)];
        out.insert(format!("{dir}/f{i}-{}.dat", r.below(1000)), r.bytes(2000));
    }
    out.into_iter().collect()
}

fn tmp(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "t47-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every file under `dir`, relative path -> bytes (directories listed with no content).
fn snapshot(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let rel = p
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if p.is_dir() {
                out.insert(rel, None);
                stack.push(p);
            } else {
                out.insert(rel, Some(std::fs::read(&p).unwrap()));
            }
        }
    }
    out
}

fn http(addr: SocketAddr, method: &str, target: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let req = Request {
        method: method.to_string(),
        target: target.to_string(),
        minor: 1,
        headers: vec![
            ("Host".to_string(), b"localhost".to_vec()),
            ("Connection".to_string(), b"close".to_vec()),
        ],
        body: body.to_vec(),
    };
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(&write_request(&req)).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let lim = Limits {
        max_body: 256 << 20,
        ..Limits::default()
    };
    let (resp, _) = parse_response(&buf, &lim, method == "HEAD").unwrap();
    (resp.status, resp.body)
}

fn publish(addr: SocketAddr, blob: &[u8], name: &str, ver: &str) -> u16 {
    http(addr, "PUT", &format!("/pkg/{name}/{ver}"), blob).0
}

#[test]
fn leader_format_matches_and_round_trips() {
    let mut r = Rng::new(0x47f0_0001);
    for i in 0..200 {
        let name = format!("p{}", r.below(50));
        let m = manifest(
            &name,
            &format!("{}.{}.{}", r.below(3), r.below(10), r.below(10)),
            &[("dep-a", "^1.2"), ("dep_b", ">=0.3.0, <2.0.0")][..r.below(3)],
        );
        let files = files_for(&mut r, &name);
        let p = Package {
            manifest: m.clone(),
            files: files.clone(),
        };
        let body = my_body(&m, &files);
        assert_eq!(pkg::encode(&p).unwrap(), body, "case {i}: encode differs");
        assert_eq!(pkg::decode(&body), Ok(p.clone()), "case {i}: decode");
        let blob = my_blob(&SEED_A, &body);
        assert_eq!(pkg::sign(&p, &SEED_A).unwrap(), blob, "case {i}: sign");
        assert_eq!(
            pkg::open(&blob, &[key(&SEED_B), key(&SEED_A)]),
            Ok(p.clone())
        );
        assert_eq!(
            pkg::open(&blob, &[key(&SEED_B)]),
            Err(PkgError::UntrustedKey)
        );
    }
    // Non-canonical bodies.
    let m = manifest("x", "1.0.0", &[]);
    let two = vec![("a".to_string(), vec![1]), ("b".to_string(), vec![2])];
    let mut unsorted = my_body(&m, &[]);
    unsorted.truncate(unsorted.len() - 4);
    unsorted.extend(2u32.to_le_bytes());
    for (p, d) in two.iter().rev() {
        unsorted.extend((p.len() as u16).to_le_bytes());
        unsorted.extend(p.as_bytes());
        unsorted.extend((d.len() as u32).to_le_bytes());
        unsorted.extend(d);
    }
    assert!(
        matches!(pkg::decode(&unsorted), Err(PkgError::Malformed(_))),
        "unsorted"
    );
    let mut dup = my_body(&m, &[]);
    dup.truncate(dup.len() - 4);
    dup.extend(2u32.to_le_bytes());
    for _ in 0..2 {
        dup.extend(1u16.to_le_bytes());
        dup.extend(b"a");
        dup.extend(0u32.to_le_bytes());
    }
    assert!(
        matches!(pkg::decode(&dup), Err(PkgError::Malformed(_))),
        "duplicate"
    );
    let mut trailing = my_body(&m, &two);
    trailing.push(0);
    assert!(
        matches!(pkg::decode(&trailing), Err(PkgError::Malformed(_))),
        "trailing"
    );
    let good = my_body(&m, &two);
    for cut in 0..good.len() {
        assert!(
            matches!(pkg::decode(&good[..cut]), Err(PkgError::Malformed(_))),
            "truncated at {cut}"
        );
    }
    // Manifest text that parses but is not canonical.
    let text = "[package]\nname = \"x\"\nversion = \"1.0.0\"\n\n";
    let mut b = b"CAIRNPKG1\n".to_vec();
    b.extend((text.len() as u32).to_le_bytes());
    b.extend(text.as_bytes());
    b.extend(0u32.to_le_bytes());
    if Manifest::parse(text).is_ok() {
        assert!(
            matches!(pkg::decode(&b), Err(PkgError::Malformed(_))),
            "non-canonical manifest"
        );
    }
}

#[test]
fn leader_path_rules() {
    let m = manifest("x", "1.0.0", &[]);
    let bad = [
        "",
        "/etc/passwd",
        "../evil",
        "a/../../evil",
        "a/./b",
        ".",
        "..",
        "a//b",
        "a/",
        "a\\b",
        "c:/x",
        "a\0b",
        "a\nb",
        "a\tb",
    ];
    for p in bad {
        let body = my_body(&m, &[(p.to_string(), vec![1, 2, 3])]);
        let blob = my_blob(&SEED_A, &body);
        assert_eq!(
            pkg::open(&blob, &[key(&SEED_A)]),
            Err(PkgError::BadPath(p.to_string())),
            "path {p:?}"
        );
    }
    let long = "a".repeat(256);
    let body = my_body(&m, &[(long.clone(), vec![])]);
    assert_eq!(pkg::decode(&body), Err(PkgError::BadPath(long)));
    let ok = ["a", ".hidden", "a/b/c.txt", "x..y", "...", &"a".repeat(255)];
    for p in ok {
        let body = my_body(&m, &[(p.to_string(), vec![9])]);
        assert!(pkg::decode(&body).is_ok(), "path {p:?} should be accepted");
    }
}

#[test]
fn leader_every_byte_flip_is_refused() {
    let m = manifest("flip", "0.1.0", &[("dep", "^1")]);
    let files = vec![
        ("bin/flip".to_string(), b"hello".to_vec()),
        ("lib/x".to_string(), vec![0; 40]),
    ];
    let blob = my_blob(&SEED_A, &my_body(&m, &files));
    let trusted = [key(&SEED_A)];
    for i in 0..blob.len() {
        for bit in [0x01u8, 0x80] {
            let mut b = blob.clone();
            b[i] ^= bit;
            let got = pkg::open(&b, &trusted);
            let want = if i < 10 {
                matches!(got, Err(PkgError::Malformed(_)))
            } else if i < 42 {
                got == Err(PkgError::UntrustedKey)
            } else {
                got == Err(PkgError::BadSignature)
            };
            assert!(want, "flip byte {i} bit {bit:#x}: {got:?}");
        }
    }
    assert!(matches!(
        pkg::open(&blob[..100], &trusted),
        Err(PkgError::Malformed(_))
    ));
    assert!(matches!(
        pkg::open(b"", &trusted),
        Err(PkgError::Malformed(_))
    ));
}

#[test]
fn leader_registry_status_codes() {
    let reg = Registry::start("127.0.0.1:0", vec![key(&SEED_A)]).unwrap();
    let a = reg.addr();
    assert_eq!(http(a, "GET", "/index", b""), (200, Vec::new()));
    let m = manifest("alpha", "1.0.0", &[]);
    let p = Package {
        manifest: m.clone(),
        files: vec![("bin/alpha".to_string(), b"A".to_vec())],
    };
    let blob = pkg::sign(&p, &SEED_A).unwrap();
    assert_eq!(publish(a, &blob, "alpha", "1.1.0"), 400, "version mismatch");
    assert_eq!(publish(a, &blob, "beta", "1.0.0"), 400, "name mismatch");
    assert_eq!(publish(a, b"junk", "alpha", "1.0.0"), 400, "junk");
    assert_eq!(
        publish(a, &pkg::sign(&p, &SEED_B).unwrap(), "alpha", "1.0.0"),
        403,
        "untrusted"
    );
    let mut flipped = blob.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    assert_eq!(publish(a, &flipped, "alpha", "1.0.0"), 403, "bad signature");
    let evil = my_blob(&SEED_A, &my_body(&m, &[("../x".to_string(), vec![])]));
    assert_eq!(publish(a, &evil, "alpha", "1.0.0"), 400, "bad path");
    assert_eq!(http(a, "GET", "/pkg/alpha/1.0.0", b"").0, 404);
    assert_eq!(publish(a, &blob, "alpha", "1.0.0"), 201);
    assert_eq!(publish(a, &blob, "alpha", "1.0.0"), 409, "immutable");
    assert_eq!(http(a, "GET", "/pkg/alpha/1.0.0", b""), (200, blob.clone()));
    assert_eq!(http(a, "GET", "/pkg/alpha/2.0.0", b"").0, 404);
    let m2 = manifest(
        "alpha",
        "0.9.0",
        &[("zeta", ">=1.0.0, <3.0.0"), ("beta", "^2")],
    );
    let p2 = Package {
        manifest: m2,
        files: vec![],
    };
    assert_eq!(
        publish(a, &pkg::sign(&p2, &SEED_A).unwrap(), "alpha", "0.9.0"),
        201
    );
    let (st, idx) = http(a, "GET", "/index", b"");
    assert_eq!(st, 200);
    let want = format!(
        "alpha\t0.9.0\tzeta\t{}\tbeta\t{}\nalpha\t1.0.0\n",
        req(">=1.0.0, <3.0.0"),
        req("^2")
    );
    assert_eq!(String::from_utf8(idx).unwrap(), want);
    assert_eq!(http(a, "GET", "/nothing", b"").0, 404);
    assert_eq!(http(a, "DELETE", "/index", b"").0, 405);
    reg.stop();
}

/// A random acyclic dependency graph over p0..p7, published to the registry.
fn publish_graph(r: &mut Rng, a: SocketAddr) -> (Index, BTreeMap<(String, Version), Package>) {
    let mut index = Index::new();
    let mut all = BTreeMap::new();
    for n in 0..8 {
        let name = format!("p{n}");
        for minor in 0..1 + r.below(3) {
            let ver = format!("1.{minor}.{}", r.below(3));
            if all.contains_key(&(name.clone(), v(&ver))) {
                continue;
            }
            let mut deps = Vec::new();
            for d in 0..n {
                if r.chance(1, 3) {
                    deps.push((
                        format!("p{d}"),
                        req(if r.chance(1, 2) { "^1" } else { "^1.0" }),
                    ));
                }
            }
            let m = Manifest {
                name: name.clone(),
                version: v(&ver),
                dependencies: deps.clone(),
            };
            let p = Package {
                manifest: m,
                files: files_for(r, &name),
            };
            assert_eq!(
                publish(a, &pkg::sign(&p, &SEED_A).unwrap(), &name, &ver),
                201
            );
            index.add(&name, v(&ver), deps);
            all.insert((name.clone(), v(&ver)), p);
        }
    }
    (index, all)
}

fn check_tree(root: &Path, sol: &Solution, all: &BTreeMap<(String, Version), Package>) {
    let mut want = BTreeMap::new();
    for (name, ver) in sol {
        let p = &all[&(name.clone(), *ver)];
        want.insert("pkgs".to_string(), None);
        let dir = format!("pkgs/{name}-{ver}");
        for (path, data) in &p.files {
            let mut acc = dir.clone();
            want.insert(acc.clone(), None);
            let parts: Vec<&str> = path.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                acc = format!("{acc}/{part}");
                want.insert(acc.clone(), None);
            }
            want.insert(format!("{dir}/{path}"), Some(data.clone()));
        }
        want.insert(
            format!("{dir}.manifest"),
            Some(p.manifest.to_string().into_bytes()),
        );
    }
    want.insert(
        "argo.lock".to_string(),
        Some(argo::lock::write(sol).into_bytes()),
    );
    let got = snapshot(root);
    for (k, val) in &want {
        assert_eq!(got.get(k), Some(val), "{k} in {root:?}");
    }
    let extra: Vec<&String> = got.keys().filter(|k| !want.contains_key(*k)).collect();
    assert!(extra.is_empty(), "unexpected entries {extra:?}");
}

#[test]
fn leader_install_lock_and_remove() {
    let mut r = Rng::new(0x0047_1a57_0001);
    let reg = Registry::start("127.0.0.1:0", vec![key(&SEED_A)]).unwrap();
    let a = reg.addr();
    let (index, all) = publish_graph(&mut r, a);
    let trusted = [key(&SEED_A)];
    for case in 0..6 {
        let root = tmp("inst");
        let deps: Vec<(String, Req)> = (0..1 + r.below(3))
            .map(|_| (format!("p{}", r.below(8)), req("^1")))
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect();
        let sol = install(a, &deps, &root, &trusted).unwrap();
        assert_eq!(sol, solve(&index, &deps, None).unwrap(), "case {case}");
        check_tree(&root, &sol, &all);
        assert_eq!(installed(&root).unwrap(), sol);
        // Installing again changes nothing.
        let before = snapshot(&root);
        assert_eq!(install(a, &deps, &root, &trusted).unwrap(), sol);
        assert_eq!(
            snapshot(&root),
            before,
            "case {case}: reinstall changed files"
        );
    }
    // Lock preference across a newer publish.
    let root = tmp("lock");
    let deps = vec![("p0".to_string(), req("^1"))];
    let first = install(a, &deps, &root, &trusted).unwrap();
    let newer = Package {
        manifest: manifest("p0", "1.9.9", &[]),
        files: vec![("bin/p0".to_string(), b"new".to_vec())],
    };
    assert_eq!(
        publish(a, &pkg::sign(&newer, &SEED_A).unwrap(), "p0", "1.9.9"),
        201
    );
    assert_eq!(
        install(a, &deps, &root, &trusted).unwrap(),
        first,
        "lock kept"
    );
    let fresh = tmp("lock2");
    assert_eq!(
        install(a, &deps, &fresh, &trusted).unwrap()["p0"],
        v("1.9.9")
    );
    // remove: dependents, leaf, not installed.
    let root = tmp("rm");
    let s = Registry::start("127.0.0.1:0", vec![key(&SEED_A)]).unwrap();
    for (n, ver, deps) in [
        ("base", "1.0.0", vec![]),
        ("mid", "1.0.0", vec![("base", "^1")]),
        ("top", "1.0.0", vec![("mid", "^1"), ("base", "^1")]),
    ] {
        let p = Package {
            manifest: manifest(n, ver, &deps),
            files: vec![(format!("bin/{n}"), n.as_bytes().to_vec())],
        };
        assert_eq!(
            publish(s.addr(), &pkg::sign(&p, &SEED_A).unwrap(), n, ver),
            201
        );
    }
    install(s.addr(), &[("top".to_string(), req("^1"))], &root, &trusted).unwrap();
    match remove(&root, "base") {
        Err(InstallError::InUse(d)) => assert_eq!(d, vec!["mid".to_string(), "top".to_string()]),
        other => panic!("remove base: {other:?}"),
    }
    assert!(matches!(
        remove(&root, "nope"),
        Err(InstallError::NotInstalled(_))
    ));
    remove(&root, "top").unwrap();
    assert!(!root.join("pkgs/top-1.0.0").exists());
    assert!(!root.join("pkgs/top-1.0.0.manifest").exists());
    let left = installed(&root).unwrap();
    assert_eq!(
        left.keys().cloned().collect::<Vec<_>>(),
        vec!["base", "mid"]
    );
    assert_eq!(
        std::fs::read_to_string(root.join("argo.lock")).unwrap(),
        argo::lock::write(&left)
    );
    s.stop();
    reg.stop();
}

/// Which `PkgError` a malicious-registry case must produce.
type ErrTest = fn(&PkgError) -> bool;

/// A registry that serves whatever the leader wants.
fn evil_server(index: String, blobs: Vec<(String, Vec<u8>)>) -> Server {
    let mut router = Router::new();
    let resp = |status: u16, body: Vec<u8>| Response {
        minor: 1,
        status,
        reason: String::new(),
        headers: vec![],
        body,
    };
    router.route(
        "GET",
        "/index",
        Box::new(move |_r: &Request| resp(200, index.clone().into_bytes())),
    );
    let blobs = Arc::new(blobs);
    router.route(
        "GET",
        "/pkg/*",
        Box::new(
            move |r: &Request| match blobs.iter().find(|(t, _)| *t == r.target) {
                Some((_, b)) => resp(200, b.clone()),
                None => resp(404, vec![]),
            },
        ),
    );
    let cfg = Config {
        limits: Limits {
            max_body: 64 << 20,
            ..Limits::default()
        },
        ..Config::default()
    };
    Server::start(cfg, router).unwrap()
}

#[test]
fn leader_malicious_registry_is_refused() {
    let trusted = [key(&SEED_A)];
    let good = Package {
        manifest: manifest("lib", "1.0.0", &[]),
        files: vec![("lib/x".to_string(), b"good".to_vec())],
    };
    let app = |files: Vec<(String, Vec<u8>)>| Package {
        manifest: manifest("app", "1.0.0", &[("lib", "^1")]),
        files,
    };
    let index = "app\t1.0.0\tlib\t^1\nlib\t1.0.0\n".replace("^1", &req("^1").to_string());
    let good_app = pkg::sign(
        &app(vec![("bin/app".to_string(), b"app".to_vec())]),
        &SEED_A,
    )
    .unwrap();
    let mut flipped = good_app.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0x20;
    let untrusted = pkg::sign(
        &app(vec![("bin/app".to_string(), b"app".to_vec())]),
        &SEED_B,
    )
    .unwrap();
    let traversal = my_blob(
        &SEED_A,
        &my_body(
            &app(vec![]).manifest,
            &[("../escaped".to_string(), b"pwned".to_vec())],
        ),
    );
    let swapped = pkg::sign(
        &Package {
            manifest: manifest("other", "1.0.0", &[]),
            files: vec![],
        },
        &SEED_A,
    )
    .unwrap();
    let cases: Vec<(&str, Vec<u8>, ErrTest)> = vec![
        ("flipped byte", flipped, |e| *e == PkgError::BadSignature),
        ("untrusted key", untrusted, |e| *e == PkgError::UntrustedKey),
        ("path traversal", traversal, |e| {
            matches!(e, PkgError::BadPath(_))
        }),
        ("swapped package", swapped, |e| {
            matches!(e, PkgError::Malformed(_))
        }),
    ];
    for (what, bad, ok) in cases {
        let parent = tmp("evil");
        let root = parent.join("root");
        std::fs::create_dir_all(&root).unwrap();
        // Start from an installed lib so "unchanged" means something.
        let pre = evil_server(
            "lib\t1.0.0\n".to_string(),
            vec![(
                "/pkg/lib/1.0.0".to_string(),
                pkg::sign(&good, &SEED_A).unwrap(),
            )],
        );
        install(
            pre.local_addr(),
            &[("lib".to_string(), req("^1"))],
            &root,
            &trusted,
        )
        .unwrap();
        pre.stop();
        let before = snapshot(&parent);
        let srv = evil_server(
            index.clone(),
            vec![
                (
                    "/pkg/lib/1.0.0".to_string(),
                    pkg::sign(&good, &SEED_A).unwrap(),
                ),
                ("/pkg/app/1.0.0".to_string(), bad),
            ],
        );
        let got = install(
            srv.local_addr(),
            &[("app".to_string(), req("^1"))],
            &root,
            &trusted,
        );
        srv.stop();
        match got {
            Err(InstallError::Package(name, e)) => {
                assert_eq!(name, "app", "{what}");
                assert!(ok(&e), "{what}: wrong error {e:?}");
            }
            other => panic!("{what}: expected a package error, got {other:?}"),
        }
        assert_eq!(snapshot(&parent), before, "{what}: files changed");
    }
    // Control: the honest version installs.
    let root = tmp("evil-ok");
    let srv = evil_server(
        index,
        vec![
            (
                "/pkg/lib/1.0.0".to_string(),
                pkg::sign(&good, &SEED_A).unwrap(),
            ),
            ("/pkg/app/1.0.0".to_string(), good_app),
        ],
    );
    let sol = install(
        srv.local_addr(),
        &[("app".to_string(), req("^1"))],
        &root,
        &trusted,
    )
    .unwrap();
    srv.stop();
    assert_eq!(sol.len(), 2);
    assert_eq!(
        std::fs::read(root.join("pkgs/app-1.0.0/bin/app")).unwrap(),
        b"app"
    );
}

#[test]
fn leader_concurrent_installers_and_publishers() {
    let mut r = Rng::new(0x0047_c0c0_0001);
    let reg = Registry::start("127.0.0.1:0", vec![key(&SEED_A)]).unwrap();
    let a = reg.addr();
    let (index, all) = publish_graph(&mut r, a);
    let all = Arc::new(all);
    let mut hs = Vec::new();
    for t in 0..8 {
        let index_sol = solve(&index, &[(format!("p{}", 7 - t % 8), req("^1"))], None).unwrap();
        let all = Arc::clone(&all);
        hs.push(std::thread::spawn(move || {
            let root = tmp("conc");
            let deps = vec![(format!("p{}", 7 - t % 8), req("^1"))];
            for _ in 0..3 {
                let sol = install(a, &deps, &root, &[key(&SEED_A)]).unwrap();
                assert_eq!(sol, index_sol);
                check_tree(&root, &sol, &all);
            }
        }));
    }
    for t in 0..4 {
        hs.push(std::thread::spawn(move || {
            for k in 0..10 {
                let name = format!("q{t}");
                let ver = format!("0.{k}.0");
                let p = Package {
                    manifest: manifest(&name, &ver, &[]),
                    files: vec![("x".to_string(), vec![k as u8; 1000])],
                };
                assert_eq!(
                    publish(a, &pkg::sign(&p, &SEED_A).unwrap(), &name, &ver),
                    201
                );
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
    let (_, idx) = http(a, "GET", "/index", b"");
    let text = String::from_utf8(idx).unwrap();
    assert_eq!(text.lines().filter(|l| l.starts_with('q')).count(), 40);
    reg.stop();
}
