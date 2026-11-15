//! Security review tests for argo trust boundaries.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use argo::client::{self, InstallError};
use argo::lock;
use argo::manifest::Manifest;
use argo::pkg::{self, Package, PkgError};
use argo::solve::{self, Index};
use argo::version::{Req, Version};
use sphinx::ed25519;

fn temp_dir(label: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!("argo-sec-{label}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn make_pkg(name: &str, ver: &str, deps: Vec<(&str, &str)>, files: Vec<(&str, &[u8])>) -> Package {
    Package {
        manifest: Manifest {
            name: name.to_string(),
            version: Version::parse(ver).unwrap(),
            dependencies: deps
                .into_iter()
                .map(|(d, r)| (d.to_string(), Req::parse(r).unwrap()))
                .collect(),
        },
        files: files
            .into_iter()
            .map(|(p, d)| (p.to_string(), d.to_vec()))
            .collect(),
    }
}

fn raw_body_with_entry(manifest: &Manifest, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let m = manifest.to_string();
    let mut out = Vec::new();
    out.extend_from_slice(b"CAIRNPKG1\n");
    out.extend_from_slice(&(m.len() as u32).to_le_bytes());
    out.extend_from_slice(m.as_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (p, d) in entries {
        out.extend_from_slice(&(p.len() as u16).to_le_bytes());
        out.extend_from_slice(p.as_bytes());
        out.extend_from_slice(&(d.len() as u32).to_le_bytes());
        out.extend_from_slice(d);
    }
    out
}

#[test]
fn pkg_decode_rejects_traversal_abs_drive_backslash_and_control_paths() {
    let manifest = make_pkg("demo", "1.0.0", vec![], vec![]).manifest;
    for bad in [
        "..",
        ".",
        "../secret",
        "a/../b",
        "a/..",
        "a/./b",
        "/etc/passwd",
        "//etc/passwd",
        "trailing/",
        "a//b",
        "C:foo",
        "C:/Windows/System32",
        "C:\\Windows\\System32",
        "\\\\server\\share\\file",
        "a\\b",
        "a\0b",
        "a\nb",
        "a\rb",
        "a\tb",
        "a\x7fb",
        "",
    ] {
        let body = raw_body_with_entry(&manifest, &[(bad, b"x")]);
        assert!(
            matches!(pkg::decode(&body), Err(PkgError::BadPath(_))),
            "expected BadPath for {bad:?}"
        );
    }
    let overlong = "a".repeat(256);
    let body = raw_body_with_entry(&manifest, &[(&overlong, b"x")]);
    assert!(matches!(pkg::decode(&body), Err(PkgError::BadPath(_))));
}

#[test]
fn pkg_decode_rejects_duplicate_and_unsorted_entries() {
    let manifest = make_pkg("demo", "1.0.0", vec![], vec![]).manifest;
    let dup = raw_body_with_entry(&manifest, &[("a.txt", b"1"), ("a.txt", b"2")]);
    assert!(matches!(pkg::decode(&dup), Err(PkgError::Malformed(_))));

    let unsorted = raw_body_with_entry(&manifest, &[("b.txt", b"1"), ("a.txt", b"2")]);
    assert!(matches!(
        pkg::decode(&unsorted),
        Err(PkgError::Malformed(_))
    ));
}

#[test]
fn pkg_decode_rejects_hostile_sizes_and_truncation_without_oom() {
    let manifest = make_pkg("demo", "1.0.0", vec![], vec![]).manifest;
    let m_str = manifest.to_string();

    // 1. manifest_len = u32::MAX
    let mut bad_mlen = Vec::new();
    bad_mlen.extend_from_slice(b"CAIRNPKG1\n");
    bad_mlen.extend_from_slice(&u32::MAX.to_le_bytes());
    bad_mlen.extend_from_slice(b"short");
    assert!(matches!(
        pkg::decode(&bad_mlen),
        Err(PkgError::Malformed(_))
    ));

    // 2. count = u32::MAX
    let mut bad_count = Vec::new();
    bad_count.extend_from_slice(b"CAIRNPKG1\n");
    bad_count.extend_from_slice(&(m_str.len() as u32).to_le_bytes());
    bad_count.extend_from_slice(m_str.as_bytes());
    bad_count.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        pkg::decode(&bad_count),
        Err(PkgError::Malformed(_))
    ));

    // 3. dlen = u32::MAX
    let mut bad_dlen = Vec::new();
    bad_dlen.extend_from_slice(b"CAIRNPKG1\n");
    bad_dlen.extend_from_slice(&(m_str.len() as u32).to_le_bytes());
    bad_dlen.extend_from_slice(m_str.as_bytes());
    bad_dlen.extend_from_slice(&1u32.to_le_bytes());
    bad_dlen.extend_from_slice(&4u16.to_le_bytes());
    bad_dlen.extend_from_slice(b"file");
    bad_dlen.extend_from_slice(&u32::MAX.to_le_bytes());
    bad_dlen.extend_from_slice(b"abc");
    assert!(matches!(
        pkg::decode(&bad_dlen),
        Err(PkgError::Malformed(_))
    ));

    // 4. Truncated at every prefix of a valid package
    let valid = pkg::encode(&make_pkg(
        "demo",
        "1.0.0",
        vec![],
        vec![("a.txt", b"hello")],
    ))
    .unwrap();
    for len in 0..valid.len() {
        assert!(
            pkg::decode(&valid[..len]).is_err(),
            "truncated body at {len} unexpectedly succeeded"
        );
    }
}

#[test]
fn pkg_open_rejects_untrusted_bad_sig_and_noncanonical() {
    let seed = [0x33u8; 32];
    let pk = ed25519::public_key(&seed);
    let other_pk = ed25519::public_key(&[0x44u8; 32]);
    let p = make_pkg("demo", "1.0.0", vec![], vec![("a.txt", b"data")]);
    let blob = pkg::sign(&p, &seed).unwrap();

    assert_eq!(pkg::open(&blob, &[other_pk]), Err(PkgError::UntrustedKey));
    assert_eq!(pkg::open(&blob, &[]), Err(PkgError::UntrustedKey));

    let mut tampered = blob.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    assert_eq!(pkg::open(&tampered, &[pk]), Err(PkgError::BadSignature));

    // Re-sign a non-canonical body (trailing byte) using the trusted key: open must still reject!
    let mut non_canon_body = pkg::encode(&p).unwrap();
    non_canon_body.push(0);
    let sig = ed25519::sign(&seed, &non_canon_body);
    let mut non_canon_blob = Vec::new();
    non_canon_blob.extend_from_slice(b"CAIRNSIG1\n");
    non_canon_blob.extend_from_slice(&pk);
    non_canon_blob.extend_from_slice(&sig);
    non_canon_blob.extend_from_slice(&non_canon_body);
    assert!(matches!(
        pkg::open(&non_canon_blob, &[pk]),
        Err(PkgError::Malformed(_))
    ));
}

#[test]
fn version_and_req_extremes_and_malformed() {
    let max_v =
        Version::parse("18446744073709551615.18446744073709551615.18446744073709551615").unwrap();
    let zero_v = Version::parse("0.0.0").unwrap();
    assert!(max_v > zero_v);

    for bad_ver in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "01.0.0",
        "1.02.0",
        "1.0.03",
        "+1.0.0",
        "-1.0.0",
        "1.2.3-alpha",
        "1.2.3+build",
        "18446744073709551616.0.0",
        "99999999999999999999999999999.0.0",
        "1.2. 3",
    ] {
        assert!(
            Version::parse(bad_ver).is_err(),
            "expected err for {bad_ver:?}"
        );
    }

    let r_max = Req::parse(">=18446744073709551615").unwrap();
    assert!(r_max.matches(&max_v));
    assert!(!r_max.matches(&zero_v));

    let r_gt_max = Req::parse(">18446744073709551615").unwrap();
    assert!(!r_gt_max.matches(&max_v));

    let r_lt_zero = Req::parse("<0.0.0").unwrap();
    assert!(!r_lt_zero.matches(&zero_v));

    let r_zero = Req::parse("^0.0.0").unwrap();
    assert!(r_zero.matches(&zero_v));
    assert!(!r_zero.matches(&Version::parse("0.0.1").unwrap()));

    for bad_req in [
        "",
        ",",
        "1.0.0,",
        ",1.0.0",
        ">",
        ">=",
        "<=",
        "^",
        "~",
        "=",
        ">>1.0.0",
        "==1.0.0",
        "=*",
        ">=1.*",
        "1.*.2",
        "*.*",
        "*.*.*",
        "1.2.3.4",
        "^18446744073709551616",
    ] {
        assert!(Req::parse(bad_req).is_err(), "expected err for {bad_req:?}");
    }
}

#[test]
fn manifest_and_lock_reject_hostile_inputs() {
    for bad_manifest in [
        "",
        "[dependencies]\na = \"1.0.0\"\n",
        "[package]\nname = \"BadName\"\nversion = \"1.0.0\"\n",
        "[package]\nname = \"../evil\"\nversion = \"1.0.0\"\n",
        "[package]\nname = \"foo\"\nname = \"bar\"\nversion = \"1.0.0\"\n",
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n[package]\n",
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n[unknown]\n",
        "[package]\nname = \"foo\"bar\"\nversion = \"1.0.0\"\n",
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n[dependencies]\nfoo = \"1.0.0\"\nfoo = \"2.0.0\"\n",
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n[dependencies]\n../evil = \"*\"\n",
    ] {
        assert!(
            Manifest::parse(bad_manifest).is_err(),
            "expected manifest parse error for:\n{bad_manifest}"
        );
    }

    for bad_lock in [
        "",
        "# wrong header\n",
        "\n# argo lock v1\n",
        "# argo lock v1\nfoo = \"1.0.0\"\nfoo = \"2.0.0\"\n",
        "# argo lock v1\n../evil = \"1.0.0\"\n",
        "# argo lock v1\nfoo = \"not-a-ver\"\n",
        "# argo lock v1\n# comment\n",
    ] {
        assert!(
            lock::read(bad_lock).is_err(),
            "expected lock parse error for:\n{bad_lock}"
        );
    }
}

#[test]
fn solver_terminates_on_dependency_cycles() {
    let v1 = Version::parse("1.0.0").unwrap();
    let v2 = Version::parse("2.0.0").unwrap();
    let r_any = Req::parse("*").unwrap();
    let r_v1 = Req::parse("^1.0").unwrap();
    let r_v2 = Req::parse("^2.0").unwrap();

    // 1. Self-cycle compatible and incompatible
    let mut idx = Index::new();
    idx.add("a", v1, vec![("a".to_string(), r_v1.clone())]);
    let start = Instant::now();
    let sol = solve::solve(&idx, &[("a".to_string(), r_any.clone())], None).unwrap();
    assert_eq!(sol.get("a"), Some(&v1));
    assert!(start.elapsed() < Duration::from_secs(1));

    let mut idx_bad_self = Index::new();
    idx_bad_self.add("a", v1, vec![("a".to_string(), r_v2.clone())]);
    let start = Instant::now();
    assert!(solve::solve(&idx_bad_self, &[("a".to_string(), r_any.clone())], None).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));

    // 2. 3-node cycle with backtracking
    let mut idx_cycle = Index::new();
    idx_cycle.add("a", v2, vec![("b".to_string(), r_v2.clone())]);
    idx_cycle.add("a", v1, vec![("b".to_string(), r_v1.clone())]);
    idx_cycle.add("b", v2, vec![("c".to_string(), r_v2.clone())]);
    idx_cycle.add("b", v1, vec![("c".to_string(), r_v1.clone())]);
    idx_cycle.add("c", v2, vec![("a".to_string(), r_v1.clone())]); // forces conflict on a=2.0.0
    idx_cycle.add("c", v1, vec![("a".to_string(), r_v1.clone())]);
    let start = Instant::now();
    let sol = solve::solve(&idx_cycle, &[("a".to_string(), r_any.clone())], None).unwrap();
    assert_eq!(sol.get("a"), Some(&v1));
    assert_eq!(sol.get("b"), Some(&v1));
    assert_eq!(sol.get("c"), Some(&v1));
    assert!(start.elapsed() < Duration::from_secs(1));

    // 3. Unsatisfiable cycle
    let mut idx_unsat = Index::new();
    idx_unsat.add("a", v1, vec![("b".to_string(), r_v1.clone())]);
    idx_unsat.add("b", v1, vec![("c".to_string(), r_v1.clone())]);
    idx_unsat.add("c", v1, vec![("a".to_string(), r_v2)]);
    let start = Instant::now();
    assert!(solve::solve(&idx_unsat, &[("a".to_string(), r_any)], None).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn remove_rejects_traversal_and_in_use_packages() {
    let root = temp_dir("remove");
    fs::write(
        root.join("argo.lock"),
        lock::write(&BTreeMap::from([(
            "app".to_string(),
            Version::parse("1.0.0").unwrap(),
        )])),
    )
    .unwrap();
    assert!(matches!(
        client::remove(&root, "../app"),
        Err(InstallError::NotInstalled(_))
    ));
    assert!(matches!(
        client::remove(&root, ".."),
        Err(InstallError::NotInstalled(_))
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn install_rejects_index_dependency_mismatch() {
    let seed = [0x55u8; 32];
    let pk = ed25519::public_key(&seed);

    let app_pkg = make_pkg(
        "app",
        "1.0.0",
        vec![("sec", "^2.0.0")],
        vec![("bin/app", b"ok")],
    );
    let sec1_pkg = make_pkg("sec", "1.0.0", vec![], vec![("lib/sec", b"old")]);
    let app_blob = pkg::sign(&app_pkg, &seed).unwrap();
    let sec1_blob = pkg::sign(&sec1_pkg, &seed).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let done_bg = Arc::clone(&done);

    let handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while !done_bg.load(Ordering::Relaxed) {
            listener.set_nonblocking(true).ok();
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).ok();
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let (status, body): (&str, Vec<u8>) = if req.starts_with("GET /index ") {
                        // Lying unsigned index: claims app depends on sec ^1.0.0 instead of ^2.0.0
                        ("200 OK", b"app\t1.0.0\tsec\t^1.0.0\nsec\t1.0.0\n".to_vec())
                    } else if req.starts_with("GET /pkg/app/1.0.0 ") {
                        ("200 OK", app_blob.clone())
                    } else if req.starts_with("GET /pkg/sec/1.0.0 ") {
                        ("200 OK", sec1_blob.clone())
                    } else {
                        ("404 Not Found", b"not found".to_vec())
                    };
                    let hdr = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(hdr.as_bytes());
                    let _ = stream.write_all(&body);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let root = temp_dir("dep-mismatch");
    let res = client::install(
        addr,
        &[("app".to_string(), Req::parse("1.0.0").unwrap())],
        &root,
        &[pk],
    );
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();
    let _ = fs::remove_dir_all(&root);

    assert!(
        res.is_err(),
        "install must reject signed package whose dependencies contradict /index solution, got: {res:?}"
    );
}

#[test]
fn install_rejects_invalid_package_name_in_index_and_deps() {
    let seed = [0x66u8; 32];
    let pk = ed25519::public_key(&seed);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen_targets = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_bg = Arc::clone(&seen_targets);
    let done = Arc::new(AtomicBool::new(false));
    let done_bg = Arc::clone(&done);

    let handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while !done_bg.load(Ordering::Relaxed) {
            listener.set_nonblocking(true).ok();
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).ok();
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if let Some(first_line) = req.lines().next() {
                        seen_bg.lock().unwrap().push(first_line.to_string());
                    }
                    let (status, body) = if req.starts_with("GET /index ") {
                        (
                            "200 OK",
                            b"../../evil\t1.0.0\napp\t1.0.0\t../../evil\t*\n".to_vec(),
                        )
                    } else {
                        ("404 Not Found", b"nope".to_vec())
                    };
                    let hdr = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(hdr.as_bytes());
                    let _ = stream.write_all(&body);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let root = temp_dir("bad-name");
    let res = client::install(
        addr,
        &[("../../evil".to_string(), Req::parse("*").unwrap())],
        &root,
        &[pk],
    );
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();
    let _ = fs::remove_dir_all(&root);

    assert!(res.is_err());
    let targets = seen_targets.lock().unwrap();
    assert!(
        !targets.iter().any(|t| t.contains("..")),
        "client sent path-traversal HTTP request target: {targets:?}"
    );
}

#[test]
fn decode_rejects_file_dir_path_collision() {
    let manifest = make_pkg("demo", "1.0.0", vec![], vec![]).manifest;
    let body = raw_body_with_entry(
        &manifest,
        &[("a", b"file"), ("a-x", b"mid"), ("a/b", b"sub")],
    );
    assert!(
        pkg::decode(&body).is_err(),
        "decode must reject path prefix collision where 'a' is both a file and a directory"
    );
}

#[test]
fn install_rejects_oversized_content_length_and_chunked_flood() {
    let seed = [0x77u8; 32];
    let pk = ed25519::public_key(&seed);

    // 1. Huge Content-Length (1 GiB) with slow peer keeping the stream open.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let done_bg = Arc::clone(&done);
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1073741824\r\nConnection: close\r\n\r\nhello",
            );
            let start = Instant::now();
            while !done_bg.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(2) {
                thread::sleep(Duration::from_millis(10));
            }
        }
    });

    let root = temp_dir("huge-cl");
    let t0 = Instant::now();
    let res = client::install(
        addr,
        &[("app".to_string(), Req::parse("1.0.0").unwrap())],
        &root,
        &[pk],
    );
    let elapsed = t0.elapsed();
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();
    let _ = fs::remove_dir_all(&root);
    assert!(res.is_err());
    assert!(
        elapsed < Duration::from_secs(1),
        "oversized Content-Length took {elapsed:?} (should reject immediately)"
    );

    // 2. Chunked flood with chunk-extension framing overhead exceeding 64 MiB on the wire
    // while keeping decoded body under 64 MiB (< 32 MiB) and chunk count small (~16k chunks).
    let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr2 = listener2.local_addr().unwrap();
    let bytes_sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let bytes_sent_bg = Arc::clone(&bytes_sent);
    let handle2 = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener2.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            );
            let mut one_chunk = Vec::new();
            one_chunk.extend_from_slice(b"800;pad=");
            one_chunk.resize(one_chunk.len() + 4096, b'x');
            one_chunk.extend_from_slice(b"\r\n");
            one_chunk.resize(one_chunk.len() + 2048, b'a');
            one_chunk.extend_from_slice(b"\r\n");
            let batch = one_chunk.repeat(16); // ~98 KiB wire, 32 KiB body
            for _ in 0..800 {
                if stream.write_all(&batch).is_err() {
                    break;
                }
                bytes_sent_bg.fetch_add(batch.len(), Ordering::Relaxed);
            }
            let _ = stream.write_all(b"0\r\n\r\n");
        }
    });

    let root2 = temp_dir("chunked-flood");
    let t1 = Instant::now();
    let res2 = client::install(
        addr2,
        &[("app".to_string(), Req::parse("1.0.0").unwrap())],
        &root2,
        &[pk],
    );
    let elapsed2 = t1.elapsed();
    let _ = handle2.join();
    let _ = fs::remove_dir_all(&root2);
    assert!(res2.is_err());
    assert!(
        elapsed2 < Duration::from_secs(1),
        "chunked flood took {elapsed2:?}"
    );
    let sent = bytes_sent.load(Ordering::Relaxed);
    assert!(
        sent <= 65 * 1024 * 1024,
        "client accepted {sent} bytes of chunked wire overhead (> 64 MiB)"
    );
}

#[test]
fn solver_avoids_exponential_blowup_on_hostile_index() {
    let v1 = Version::parse("1.0.0").unwrap();
    let v2 = Version::parse("2.0.0").unwrap();
    let r_any = Req::parse("*").unwrap();
    let r_v1 = Req::parse("^1.0").unwrap();
    let r_v2 = Req::parse("^2.0").unwrap();

    // 1. Solvable index where top 2.0.0 pulls 26 branching packages + an unsatisfiable dep,
    //    while top 1.0.0 has no deps. Without CBJ/forward checking this explores 2^26 states.
    let mut idx = Index::new();
    let mut bad_deps = Vec::new();
    for i in 0..26 {
        let name = format!("p{i}");
        idx.add(&name, v2, vec![]);
        idx.add(&name, v1, vec![]);
        bad_deps.push((name, r_any.clone()));
    }
    bad_deps.push(("missing".to_string(), r_v1.clone()));
    idx.add("top", v2, bad_deps);
    idx.add("top", v1, vec![]);

    let t0 = Instant::now();
    let sol = solve::solve(&idx, &[("top".to_string(), r_any.clone())], None).unwrap();
    assert_eq!(sol.get("top"), Some(&v1));
    assert!(t0.elapsed() < Duration::from_secs(1));

    // 2. Unsatisfiable conflict behind 26 unrelated branching packages.
    let mut idx_conflict = Index::new();
    idx_conflict.add("a", v2, vec![("c".to_string(), r_v2.clone())]);
    idx_conflict.add("a", v1, vec![("c".to_string(), r_v2.clone())]);
    idx_conflict.add("c", v2, vec![]);
    idx_conflict.add("c", v1, vec![]);
    let mut root_conflict = vec![("a".to_string(), r_any.clone())];
    for i in 0..26 {
        let name = format!("q{i}");
        idx_conflict.add(&name, v2, vec![]);
        idx_conflict.add(&name, v1, vec![]);
        root_conflict.push((name, r_any.clone()));
    }
    idx_conflict.add("b", v1, vec![("c".to_string(), r_v1.clone())]);
    root_conflict.push(("b".to_string(), r_any.clone()));

    let t1 = Instant::now();
    assert!(solve::solve(&idx_conflict, &root_conflict, None).is_err());
    assert!(t1.elapsed() < Duration::from_secs(1));

    // 3. Coupled unsatisfiable graph where every variable touches a shared sink constraint.
    let mut idx_coupled = Index::new();
    let mut root_coupled = Vec::new();
    for i in 0..26 {
        let name = format!("x{i}");
        let next = if i + 1 < 26 {
            vec![(format!("x{}", i + 1), r_any.clone())]
        } else {
            vec![("sink".to_string(), r_v1.clone())]
        };
        idx_coupled.add(&name, v2, next.clone());
        idx_coupled.add(&name, v1, next);
        root_coupled.push((name, r_any.clone()));
    }
    idx_coupled.add("sink", v2, vec![]);
    idx_coupled.add(
        "x0",
        v2,
        vec![
            ("sink".to_string(), r_v2.clone()),
            ("x1".to_string(), r_any.clone()),
        ],
    );
    idx_coupled.add(
        "x0",
        v1,
        vec![
            ("sink".to_string(), r_v2),
            ("x1".to_string(), r_any.clone()),
        ],
    );

    let t2 = Instant::now();
    assert!(solve::solve(&idx_coupled, &root_coupled, None).is_err());
    assert!(t2.elapsed() < Duration::from_secs(1));

    // 4. Coupled pigeonhole PHP(11, 10): 11 pigeons into 10 holes (11! = 39.9M states).
    //    CBJ cannot skip any ancestor; step budget must abort cleanly in < 1 s.
    let mut idx_php = Index::new();
    let mut root_php = Vec::new();
    for h in 0..10u64 {
        let h_name = format!("hole_{h}");
        for p in 0..11u64 {
            idx_php.add(
                &h_name,
                Version {
                    major: 1,
                    minor: 0,
                    patch: p,
                },
                vec![],
            );
        }
    }
    for p in 0..11u64 {
        let p_name = format!("pigeon_{p}");
        let p_req = Req::parse(&format!("=1.0.{p}")).unwrap();
        for h in 0..10u64 {
            idx_php.add(
                &p_name,
                Version {
                    major: 1,
                    minor: 0,
                    patch: h,
                },
                vec![(format!("hole_{h}"), p_req.clone())],
            );
        }
        root_php.push((p_name, r_any.clone()));
    }
    let t3 = Instant::now();
    assert!(solve::solve(&idx_php, &root_php, None).is_err());
    assert!(t3.elapsed() < Duration::from_secs(1));
}

#[test]
fn install_completes_without_waiting_for_slow_peer_eof() {
    let seed = [0x88u8; 32];
    let pk = ed25519::public_key(&seed);
    let pkg = make_pkg("app", "1.0.0", vec![], vec![("bin/app", b"ok")]);
    let blob = pkg::sign(&pkg, &seed).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let done_bg = Arc::clone(&done);

    let handle = thread::spawn(move || {
        let mut sockets = Vec::new();
        let mut buf = [0u8; 4096];
        listener.set_nonblocking(true).ok();
        while !done_bg.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).ok();
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.starts_with("GET /index ") {
                        let body = b"app\t1.0.0\n";
                        let mut chunked = Vec::new();
                        chunked.extend_from_slice(
                            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n",
                        );
                        chunked
                            .extend_from_slice(format!("{:x};pad=xyz\r\n", body.len()).as_bytes());
                        chunked.extend_from_slice(body);
                        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
                        let _ = stream.write_all(&chunked);
                    } else if req.starts_with("GET /pkg/app/1.0.0 ") {
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                            blob.len()
                        );
                        let _ = stream.write_all(hdr.as_bytes());
                        let _ = stream.write_all(&blob);
                    }
                    // Deliberately keep socket open to simulate a peer that does not send FIN.
                    sockets.push(stream);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let root = temp_dir("keep-open");
    let t0 = Instant::now();
    let res = client::install(
        addr,
        &[("app".to_string(), Req::parse("1.0.0").unwrap())],
        &root,
        &[pk],
    );
    let elapsed = t0.elapsed();
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();
    let _ = fs::remove_dir_all(&root);
    assert!(res.is_ok(), "expected install to succeed, got {res:?}");
    assert!(
        elapsed < Duration::from_secs(1),
        "install waited {elapsed:?} on slow/keep-open peer"
    );
}
