//! Integration tests for argo packages, registry, and client.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use argo::client::{self, InstallError};
use argo::manifest::Manifest;
use argo::pkg::{self, Package, PkgError};
use argo::registry::Registry;
use argo::version::{Req, Version};
use assay::forall;
use mercury::server::{Config, Router, Server};
use mercury::{parse_response, write_request, Error as HttpError, Limits, Request, Response};
use sphinx::ed25519;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("cairn_test_{prefix}_{pid}_{id}"));
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn http_call(addr: SocketAddr, method: &str, target: &str, body: Vec<u8>) -> Response {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let req = Request {
        method: method.to_string(),
        target: target.to_string(),
        minor: 1,
        headers: vec![
            ("Host".to_string(), addr.to_string().into_bytes()),
            ("Connection".to_string(), b"close".to_vec()),
        ],
        body,
    };
    stream.write_all(&write_request(&req)).unwrap();
    stream.flush().unwrap();

    let limits = Limits {
        max_body: 64 * 1024 * 1024,
        ..Limits::default()
    };
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match parse_response(&buf, &limits, method == "HEAD") {
            Ok((resp, _)) => return resp,
            Err(HttpError::Incomplete) => {
                let n = stream.read(&mut tmp).unwrap();
                assert!(n > 0, "unexpected EOF");
                buf.extend_from_slice(&tmp[..n]);
            }
            Err(e) => panic!("bad HTTP response: {e:?}"),
        }
    }
}

fn make_pkg(name: &str, ver: &str, deps: &[(&str, &str)], files: &[(&str, &[u8])]) -> Package {
    Package {
        manifest: Manifest {
            name: name.to_string(),
            version: Version::parse(ver).unwrap(),
            dependencies: deps
                .iter()
                .map(|(d, r)| (d.to_string(), Req::parse(r).unwrap()))
                .collect(),
        },
        files: files
            .iter()
            .map(|(p, d)| (p.to_string(), d.to_vec()))
            .collect(),
    }
}

fn snapshot_dir(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, cur: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(cur).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                out.insert(format!("{rel}/"), Vec::new());
                walk(base, &path, out);
            } else {
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut out);
    }
    out
}

#[test]
fn pkg_roundtrip_property_and_byte_flip() {
    let seed = [0x5au8; 32];
    let pk = ed25519::public_key(&seed);

    forall(15, 6, |r| {
        let n_files = (r.u64() % 5) as usize;
        let mut files = Vec::new();
        for i in 0..n_files {
            let path = format!("dir{}/file_{}.bin", i % 2, i);
            let len = (r.u64() % 64) as usize;
            let data = r.bytes(len);
            files.push((path, data));
        }
        let p = Package {
            manifest: Manifest {
                name: format!("pkg{}", r.u64() % 100),
                version: Version {
                    major: r.u64() % 5,
                    minor: r.u64() % 5,
                    patch: r.u64() % 5,
                },
                dependencies: vec![("core".to_string(), Req::parse("^1.0").unwrap())],
            },
            files,
        };
        let body = pkg::encode(&p).unwrap();
        let decoded = pkg::decode(&body).unwrap();
        assert_eq!(pkg::encode(&decoded).unwrap(), body);

        let blob = pkg::sign(&p, &seed).unwrap();
        let opened = pkg::open(&blob, &[pk]).unwrap();
        assert_eq!(opened, decoded);
    });

    // Flipping each byte of a signed blob must fail, never Ok.
    let p = make_pkg("alpha", "1.0.0", &[], &[("a/b.txt", b"hello world")]);
    let blob = pkg::sign(&p, &seed).unwrap();
    for i in 0..blob.len() {
        let mut bad = blob.clone();
        bad[i] ^= 0x01;
        let res = pkg::open(&bad, &[pk]);
        assert!(res.is_err(), "flipping byte {i} unexpectedly succeeded");
    }
}

#[test]
fn registry_status_codes_and_index_order() {
    let seed = [0x33u8; 32];
    let pk = ed25519::public_key(&seed);
    let untrusted_seed = [0x44u8; 32];
    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    let addr = reg.addr();

    // Empty index
    let idx0 = http_call(addr, "GET", "/index", Vec::new());
    assert_eq!(idx0.status, 200);
    assert!(idx0.body.is_empty());

    // Unknown path -> 404; known path wrong method -> 405
    assert_eq!(http_call(addr, "GET", "/nope", Vec::new()).status, 404);
    assert_eq!(http_call(addr, "POST", "/index", Vec::new()).status, 405);
    assert_eq!(
        http_call(addr, "DELETE", "/pkg/a/1.0.0", Vec::new()).status,
        405
    );

    // 403 untrusted key
    let p_a = make_pkg("zeta", "1.0.0", &[("alpha", "^1.0")], &[("f.txt", b"hi")]);
    let bad_key_blob = pkg::sign(&p_a, &untrusted_seed).unwrap();
    assert_eq!(
        http_call(addr, "PUT", "/pkg/zeta/1.0.0", bad_key_blob).status,
        403
    );

    // 403 bad signature
    let mut bad_sig_blob = pkg::sign(&p_a, &seed).unwrap();
    bad_sig_blob[50] ^= 0x80;
    assert_eq!(
        http_call(addr, "PUT", "/pkg/zeta/1.0.0", bad_sig_blob).status,
        403
    );

    // 400 URL mismatch
    let good_blob = pkg::sign(&p_a, &seed).unwrap();
    assert_eq!(
        http_call(addr, "PUT", "/pkg/other/1.0.0", good_blob.clone()).status,
        400
    );
    assert_eq!(
        http_call(addr, "PUT", "/pkg/zeta/2.0.0", good_blob.clone()).status,
        400
    );

    // 400 malformed body
    assert_eq!(
        http_call(addr, "PUT", "/pkg/zeta/1.0.0", b"garbage".to_vec()).status,
        400
    );

    // 201 publish, 409 duplicate (even identical bytes)
    assert_eq!(
        http_call(addr, "PUT", "/pkg/zeta/1.0.0", good_blob.clone()).status,
        201
    );
    assert_eq!(
        http_call(addr, "PUT", "/pkg/zeta/1.0.0", good_blob.clone()).status,
        409
    );

    // GET /pkg/<name>/<version> byte-for-byte
    let got = http_call(addr, "GET", "/pkg/zeta/1.0.0", Vec::new());
    assert_eq!(got.status, 200);
    assert_eq!(got.body, good_blob);
    assert_eq!(
        http_call(addr, "GET", "/pkg/zeta/9.9.9", Vec::new()).status,
        404
    );

    // Publish alpha 1.0.0 and 1.1.0, check sorted /index
    let p_alpha_1 = make_pkg("alpha", "1.0.0", &[], &[("lib.rs", b"1")]);
    let p_alpha_2 = make_pkg("alpha", "1.1.0", &[], &[("lib.rs", b"2")]);
    assert_eq!(
        http_call(
            addr,
            "PUT",
            "/pkg/alpha/1.1.0",
            pkg::sign(&p_alpha_2, &seed).unwrap()
        )
        .status,
        201
    );
    assert_eq!(
        http_call(
            addr,
            "PUT",
            "/pkg/alpha/1.0.0",
            pkg::sign(&p_alpha_1, &seed).unwrap()
        )
        .status,
        201
    );

    let idx = http_call(addr, "GET", "/index", Vec::new());
    assert_eq!(idx.status, 200);
    assert_eq!(
        String::from_utf8(idx.body).unwrap(),
        "alpha\t1.0.0\nalpha\t1.1.0\nzeta\t1.0.0\talpha\t^1.0\n"
    );

    reg.stop();
}

#[test]
fn install_lock_preference_and_remove() {
    let seed = [0x71u8; 32];
    let pk = ed25519::public_key(&seed);
    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    let addr = reg.addr();

    let lib_10 = make_pkg("lib", "1.0.0", &[], &[("src/lib.txt", b"v1.0")]);
    let app_10 = make_pkg(
        "app",
        "1.0.0",
        &[("lib", "^1.0")],
        &[("bin/app", b"app1.0")],
    );
    assert_eq!(
        http_call(
            addr,
            "PUT",
            "/pkg/lib/1.0.0",
            pkg::sign(&lib_10, &seed).unwrap()
        )
        .status,
        201
    );
    assert_eq!(
        http_call(
            addr,
            "PUT",
            "/pkg/app/1.0.0",
            pkg::sign(&app_10, &seed).unwrap()
        )
        .status,
        201
    );

    let tmp = TempDir::new("install_remove");
    let root = tmp.path();

    let sol1 = client::install(
        addr,
        &[("app".to_string(), Req::parse("^1.0").unwrap())],
        root,
        &[pk],
    )
    .unwrap();
    assert_eq!(sol1.get("app"), Some(&Version::parse("1.0.0").unwrap()));
    assert_eq!(sol1.get("lib"), Some(&Version::parse("1.0.0").unwrap()));
    assert_eq!(
        fs::read(root.join("pkgs/lib-1.0.0/src/lib.txt")).unwrap(),
        b"v1.0"
    );
    assert_eq!(client::installed(root).unwrap(), sol1);

    // Publish lib 1.1.0; reinstalling should keep locked lib 1.0.0.
    let lib_11 = make_pkg("lib", "1.1.0", &[], &[("src/lib.txt", b"v1.1")]);
    assert_eq!(
        http_call(
            addr,
            "PUT",
            "/pkg/lib/1.1.0",
            pkg::sign(&lib_11, &seed).unwrap()
        )
        .status,
        201
    );
    let sol2 = client::install(
        addr,
        &[("app".to_string(), Req::parse("^1.0").unwrap())],
        root,
        &[pk],
    )
    .unwrap();
    assert_eq!(sol2, sol1);

    // Removing lib fails with InUse(["app"])
    match client::remove(root, "lib") {
        Err(InstallError::InUse(deps)) => assert_eq!(deps, vec!["app".to_string()]),
        other => panic!("expected InUse, got {other:?}"),
    }

    // Removing missing package -> NotInstalled
    assert!(matches!(
        client::remove(root, "missing"),
        Err(InstallError::NotInstalled(_))
    ));

    // Remove app, then lib
    client::remove(root, "app").unwrap();
    client::remove(root, "lib").unwrap();
    assert!(client::installed(root).unwrap().is_empty());

    reg.stop();
}

#[test]
fn malicious_registry_leaves_root_unchanged() {
    let good_seed = [0x81u8; 32];
    let good_pk = ed25519::public_key(&good_seed);
    let bad_seed = [0x82u8; 32];

    let tmp = TempDir::new("malicious");
    let root = tmp.path();
    fs::write(root.join("keep.txt"), b"untouched").unwrap();
    let before = snapshot_dir(root);

    // Craft 4 malicious responses:
    // 1. flipped byte
    let p_flip = make_pkg("badflip", "1.0.0", &[], &[("a.txt", b"ok")]);
    let mut flip_blob = pkg::sign(&p_flip, &good_seed).unwrap();
    let last = flip_blob.len() - 1;
    flip_blob[last] ^= 0xff;

    // 2. untrusted key
    let p_untrusted = make_pkg("untrusted", "1.0.0", &[], &[("a.txt", b"ok")]);
    let untrusted_blob = pkg::sign(&p_untrusted, &bad_seed).unwrap();

    // 3. validly signed package with "../escape" path (hand-built body)
    let bad_manifest = " [package]\n";
    let _ = bad_manifest;
    let m_trav = Manifest {
        name: "trav".to_string(),
        version: Version::parse("1.0.0").unwrap(),
        dependencies: vec![],
    };
    let m_text = m_trav.to_string();
    let mut trav_body = Vec::new();
    trav_body.extend_from_slice(b"CAIRNPKG1\n");
    trav_body.extend_from_slice(&(m_text.len() as u32).to_le_bytes());
    trav_body.extend_from_slice(m_text.as_bytes());
    trav_body.extend_from_slice(&1u32.to_le_bytes());
    let bad_path = b"../escape";
    trav_body.extend_from_slice(&(bad_path.len() as u16).to_le_bytes());
    trav_body.extend_from_slice(bad_path);
    trav_body.extend_from_slice(&2u32.to_le_bytes());
    trav_body.extend_from_slice(b"no");
    let trav_sig = ed25519::sign(&good_seed, &trav_body);
    let mut trav_blob = Vec::new();
    trav_blob.extend_from_slice(b"CAIRNSIG1\n");
    trav_blob.extend_from_slice(&good_pk);
    trav_blob.extend_from_slice(&trav_sig);
    trav_blob.extend_from_slice(&trav_body);

    // 4. swapped package (validly signed "other" served as "swapped")
    let p_other = make_pkg("other", "1.0.0", &[], &[("a.txt", b"ok")]);
    let swapped_blob = pkg::sign(&p_other, &good_seed).unwrap();

    let index_body = b"badflip\t1.0.0\nuntrusted\t1.0.0\ntrav\t1.0.0\nswapped\t1.0.0\n".to_vec();
    let mut router = Router::new();
    router.route(
        "GET",
        "/index",
        Box::new(move |_| Response {
            minor: 1,
            status: 200,
            reason: "OK".to_string(),
            headers: vec![(
                "Content-Length".to_string(),
                index_body.len().to_string().into_bytes(),
            )],
            body: index_body.clone(),
        }),
    );
    router.route(
        "GET",
        "/pkg/*",
        Box::new(move |req| {
            let body = match req.target.as_str() {
                "/pkg/badflip/1.0.0" => flip_blob.clone(),
                "/pkg/untrusted/1.0.0" => untrusted_blob.clone(),
                "/pkg/trav/1.0.0" => trav_blob.clone(),
                "/pkg/swapped/1.0.0" => swapped_blob.clone(),
                _ => Vec::new(),
            };
            Response {
                minor: 1,
                status: 200,
                reason: "OK".to_string(),
                headers: vec![(
                    "Content-Length".to_string(),
                    body.len().to_string().into_bytes(),
                )],
                body,
            }
        }),
    );

    let srv = Server::start(Config::default(), router).unwrap();
    let addr = srv.local_addr();

    let r1 = client::install(
        addr,
        &[("badflip".to_string(), Req::parse("1.0.0").unwrap())],
        root,
        &[good_pk],
    );
    assert!(matches!(
        r1,
        Err(InstallError::Package(ref n, PkgError::BadSignature)) if n == "badflip"
    ));
    assert_eq!(snapshot_dir(root), before);

    let r2 = client::install(
        addr,
        &[("untrusted".to_string(), Req::parse("1.0.0").unwrap())],
        root,
        &[good_pk],
    );
    assert!(matches!(
        r2,
        Err(InstallError::Package(ref n, PkgError::UntrustedKey)) if n == "untrusted"
    ));
    assert_eq!(snapshot_dir(root), before);

    let r3 = client::install(
        addr,
        &[("trav".to_string(), Req::parse("1.0.0").unwrap())],
        root,
        &[good_pk],
    );
    assert!(matches!(
        r3,
        Err(InstallError::Package(ref n, PkgError::BadPath(_))) if n == "trav"
    ));
    assert_eq!(snapshot_dir(root), before);

    let r4 = client::install(
        addr,
        &[("swapped".to_string(), Req::parse("1.0.0").unwrap())],
        root,
        &[good_pk],
    );
    assert!(matches!(
        r4,
        Err(InstallError::Package(ref n, PkgError::Malformed(_))) if n == "swapped"
    ));
    assert_eq!(snapshot_dir(root), before);

    srv.stop();
}

#[test]
fn concurrent_publishers_and_installers() {
    let seed = [0x91u8; 32];
    let pk = ed25519::public_key(&seed);
    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    let addr = reg.addr();

    // Publish 8 base packages concurrently.
    let mut pub_handles = Vec::new();
    for i in 0..8 {
        pub_handles.push(thread::spawn(move || {
            let name = format!("worker{i}");
            let p = make_pkg(&name, "1.0.0", &[], &[("data.txt", name.as_bytes())]);
            let blob = pkg::sign(&p, &seed).unwrap();
            let target = format!("/pkg/{name}/1.0.0");
            let resp = http_call(addr, "PUT", &target, blob);
            assert_eq!(resp.status, 201);
        }));
    }
    for h in pub_handles {
        h.join().unwrap();
    }

    // Install from 8 concurrent clients.
    let tmp = Arc::new(TempDir::new("concurrent"));
    let mut inst_handles = Vec::new();
    for i in 0..8 {
        let tmp = Arc::clone(&tmp);
        inst_handles.push(thread::spawn(move || {
            let root = tmp.path().join(format!("client_{i}"));
            let name = format!("worker{i}");
            let sol = client::install(
                addr,
                &[(name.clone(), Req::parse("^1.0").unwrap())],
                &root,
                &[pk],
            )
            .unwrap();
            assert_eq!(sol.get(&name), Some(&Version::parse("1.0.0").unwrap()));
            assert_eq!(
                fs::read(root.join(format!("pkgs/{name}-1.0.0/data.txt"))).unwrap(),
                name.as_bytes()
            );
        }));
    }
    for h in inst_handles {
        h.join().unwrap();
    }

    reg.stop();
}
