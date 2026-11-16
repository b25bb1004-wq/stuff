//! CLI integration tests for argo.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use argo::client;
use argo::lock;
use argo::manifest::Manifest;
use argo::pkg::{self, Package};
use argo::version::{Req, Version};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_argo")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "argo_cli_{label}_{}_{}_{}",
            std::process::id(),
            nanos,
            seq
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
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

fn make_signed_pkg(
    name: &str,
    ver: &str,
    deps: Vec<(&str, &str)>,
    files: Vec<(&str, &[u8])>,
    seed: &[u8; 32],
) -> Vec<u8> {
    let manifest = Manifest {
        name: name.to_string(),
        version: Version::parse(ver).unwrap(),
        dependencies: deps
            .into_iter()
            .map(|(n, r)| (n.to_string(), Req::parse(r).unwrap()))
            .collect(),
    };
    let pkg = Package {
        manifest,
        files: files
            .into_iter()
            .map(|(p, b)| (p.to_string(), b.to_vec()))
            .collect(),
    };
    pkg::sign(&pkg, seed).unwrap()
}

#[test]
fn help_and_version_and_unknown() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    for sub in ["solve", "lock", "install", "serve"] {
        assert!(stdout.contains(sub), "help missing {sub}");
    }

    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "argo 0.1.0");

    let out = Command::new(bin()).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("argo:"), "stderr: {stderr}");
}

#[test]
fn solve_good_and_conflict() {
    let tmp = TempDir::new("solve");
    let index_path = tmp.path().join("index.txt");
    let manifest_path = tmp.path().join("Cargo.argo");

    fs::write(&index_path, "alpha\t1.0.0\nbeta\t2.0.0\talpha\t^1.0\n").unwrap();
    fs::write(
        &manifest_path,
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nbeta = \"^2.0\"\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["solve"])
        .arg(&index_path)
        .arg(&manifest_path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "alpha 1.0.0\nbeta 2.0.0\n");

    // Stdin '-' for manifest
    let mut child = Command::new(bin())
        .args(["solve"])
        .arg(&index_path)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            b"[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nalpha = \"^1.0\"\n",
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "alpha 1.0.0\n");

    // Conflict -> exit 1 with argo: message
    let bad_manifest = tmp.path().join("bad.argo");
    fs::write(
        &bad_manifest,
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nalpha = \"^9.0\"\n",
    )
    .unwrap();
    let out = Command::new(bin())
        .args(["solve"])
        .arg(&index_path)
        .arg(&bad_manifest)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("argo:"), "stderr: {stderr}");
}

#[test]
fn lock_good_and_bad() {
    let tmp = TempDir::new("lock");
    let index_path = tmp.path().join("index.txt");
    let manifest_path = tmp.path().join("Cargo.argo");
    let lock_path = tmp.path().join("argo.lock");

    fs::write(&index_path, "liba\t1.2.0\n").unwrap();
    fs::write(
        &manifest_path,
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nliba = \"^1.0\"\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["lock"])
        .arg(&index_path)
        .arg(&manifest_path)
        .arg("-o")
        .arg(&lock_path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let parsed = lock::read(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(parsed.get("liba"), Some(&Version::parse("1.2.0").unwrap()));

    // Bad manifest syntax -> exit 1
    let bad_manifest = tmp.path().join("bad.argo");
    fs::write(&bad_manifest, "not a manifest").unwrap();
    let out = Command::new(bin())
        .args(["lock"])
        .arg(&index_path)
        .arg(&bad_manifest)
        .arg("-o")
        .arg(&lock_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("argo:"));
}

#[test]
fn install_good_and_bad() {
    let tmp = TempDir::new("install");
    let pkg_dir = tmp.path().join("pkgs_src");
    let store_dir = tmp.path().join("store");
    let lock_path = tmp.path().join("argo.lock");
    fs::create_dir_all(&pkg_dir).unwrap();

    let seed = [0x42u8; 32];
    let dep_blob = make_signed_pkg(
        "core",
        "1.0.0",
        vec![],
        vec![("lib.txt", b"hello core")],
        &seed,
    );
    let app_blob = make_signed_pkg(
        "tool",
        "0.2.0",
        vec![("core", "^1.0")],
        vec![("bin/run.txt", b"run")],
        &seed,
    );
    fs::write(pkg_dir.join("core-1.0.0.pkg"), &dep_blob).unwrap();
    fs::write(pkg_dir.join("tool-0.2.0.pkg"), &app_blob).unwrap();
    fs::write(
        &lock_path,
        "# argo lock v1\ncore = \"1.0.0\"\ntool = \"0.2.0\"\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["install"])
        .arg(&lock_path)
        .arg(&store_dir)
        .arg("--from")
        .arg(&pkg_dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "core 1.0.0\ntool 0.2.0\n");

    let installed = client::installed(&store_dir).unwrap();
    assert_eq!(
        installed.get("core"),
        Some(&Version::parse("1.0.0").unwrap())
    );
    assert_eq!(
        installed.get("tool"),
        Some(&Version::parse("0.2.0").unwrap())
    );
    assert_eq!(
        fs::read(store_dir.join("pkgs/core-1.0.0/lib.txt")).unwrap(),
        b"hello core"
    );

    // Bad lock file -> exit 1
    let bad_lock = tmp.path().join("bad.lock");
    fs::write(&bad_lock, "wrong header\n").unwrap();
    let out = Command::new(bin())
        .args(["install"])
        .arg(&bad_lock)
        .arg(&store_dir)
        .arg("--from")
        .arg(&pkg_dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("argo:"));
}

#[test]
fn serve_good_and_bad() {
    let tmp = TempDir::new("serve");
    let pkg_dir = tmp.path().join("repo");
    let store_dir = tmp.path().join("store");
    fs::create_dir_all(&pkg_dir).unwrap();

    let seed = [0x77u8; 32];
    let pk = sphinx::ed25519::public_key(&seed);
    let blob = make_signed_pkg(
        "widget",
        "1.1.0",
        vec![],
        vec![("readme.txt", b"widget data")],
        &seed,
    );
    fs::write(pkg_dir.join("widget-1.1.0.pkg"), &blob).unwrap();

    let mut child = Command::new(bin())
        .args(["serve"])
        .arg(&pkg_dir)
        .args(["--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let addr_str = line.trim().strip_prefix("listening on ").unwrap();
    let addr: SocketAddr = addr_str.parse().unwrap();

    let deps = vec![("widget".to_string(), Req::parse("^1.0").unwrap())];
    let sol = client::install(addr, &deps, &store_dir, &[pk]).unwrap();
    assert_eq!(sol.get("widget"), Some(&Version::parse("1.1.0").unwrap()));

    let _ = child.kill();
    let _ = child.wait();

    // Missing directory -> exit 1
    let out = Command::new(bin())
        .args(["serve"])
        .arg(tmp.path().join("missing"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("argo:"));
}

#[test]
fn help_starts_with_tool_line() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines[0].starts_with("argo - "), "{}", lines[0]);
    assert_eq!(lines[1], "Usage:");
}
