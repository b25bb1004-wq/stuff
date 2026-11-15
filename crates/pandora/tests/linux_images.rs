//! Real Linux container tests for image pull, rootfs build, detached run, ps, kill, and rm.
//! Skips (with a printed reason) where the kernel blocks unprivileged user namespaces.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use argo::manifest::Manifest;
use argo::pkg::{self, Package};
use argo::registry::Registry;
use argo::version::{Req, Version};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pandora-limg-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn hex64(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn publish(addr: SocketAddr, seed: &[u8; 32], p: &Package) {
    let blob = pkg::sign(p, seed);
    let target = format!("/pkg/{}/{}", p.manifest.name, p.manifest.version);
    let mut msg = format!(
        "PUT {target} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        blob.len()
    )
    .into_bytes();
    msg.extend_from_slice(&blob);
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(&msg).unwrap();
    s.flush().unwrap();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    loop {
        match s.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(e) if !buf.is_empty() && e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => panic!("read failed: {e}"),
        }
    }
    let resp = String::from_utf8_lossy(&buf);
    assert!(
        resp.starts_with("HTTP/1.1 201 "),
        "unexpected publish response: {resp}"
    );
}

fn collect_bin_and_deps(host_bin: &Path, pkg_rel: &str, files: &mut BTreeMap<String, Vec<u8>>) {
    let real = fs::canonicalize(host_bin).unwrap();
    files.insert(pkg_rel.to_string(), fs::read(&real).unwrap());
    let ldd = Command::new("ldd").arg(&real).output().unwrap();
    for line in String::from_utf8_lossy(&ldd.stdout).lines() {
        if let Some(p) = line.split_whitespace().find(|w| w.starts_with('/')) {
            if let Ok(rel) = Path::new(p).strip_prefix("/") {
                let rel_str = rel.to_string_lossy().into_owned();
                files.entry(rel_str).or_insert_with(|| fs::read(p).unwrap());
            }
        }
    }
}

fn run_or_skip(out: Output) -> Option<Output> {
    let err = String::from_utf8_lossy(&out.stderr);
    let blocked = err.contains("unshare failed")
        || err.contains("making / private failed")
        || (out.status.code() == Some(125) && err.contains("Permission denied"));
    if blocked {
        eprintln!(
            "SKIP: unprivileged user namespaces are blocked here: {}",
            err.trim()
        );
        return None;
    }
    Some(out)
}

#[test]
fn pull_run_detached_ps_kill_rm() {
    let dir = tmp("e2e");
    let store = dir.join("store");
    let seed = [0x52u8; 32];
    let pk = sphinx::ed25519::public_key(&seed);

    let mut base_files = BTreeMap::new();
    collect_bin_and_deps(Path::new("/bin/sh"), "bin/sh", &mut base_files);
    let sleep_bin = if Path::new("/bin/sleep").exists() {
        Path::new("/bin/sleep")
    } else {
        Path::new("/usr/bin/sleep")
    };
    collect_bin_and_deps(sleep_bin, "bin/sleep", &mut base_files);

    let base = Package {
        manifest: Manifest {
            name: "base".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![],
        },
        files: base_files.into_iter().collect(),
    };
    let app = Package {
        manifest: Manifest {
            name: "app".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![("base".into(), Req::parse("^1").unwrap())],
        },
        files: vec![("app/hello.sh".into(), b"echo hello from app\n".to_vec())],
    };

    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    publish(reg.addr(), &seed, &base);
    publish(reg.addr(), &seed, &app);

    let exe = env!("CARGO_BIN_EXE_pandora");
    let pull_out = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .args([
            "pull",
            "--registry",
            &reg.addr().to_string(),
            "--trust",
            &hex64(&pk),
            "app",
        ])
        .output()
        .unwrap();
    assert_eq!(
        pull_out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&pull_out.stderr)
    );

    let fg_out = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .args(["run", "--image", "app", "--", "/bin/sh", "/app/hello.sh"])
        .output()
        .unwrap();
    let Some(fg_out) = run_or_skip(fg_out) else {
        reg.stop();
        let _ = fs::remove_dir_all(&dir);
        return;
    };
    assert_eq!(
        fg_out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&fg_out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&fg_out.stdout).trim(),
        "hello from app"
    );

    let det_out = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .args([
            "run",
            "-d",
            "--name",
            "sleeper",
            "--image",
            "app",
            "--",
            "/bin/sleep",
            "30",
        ])
        .output()
        .unwrap();
    assert_eq!(
        det_out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&det_out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&det_out.stdout).trim(), "sleeper");

    let ps1 = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .arg("ps")
        .output()
        .unwrap();
    assert_eq!(ps1.status.code(), Some(0));
    let ps1_txt = String::from_utf8_lossy(&ps1.stdout);
    assert!(
        ps1_txt.lines().any(|l| l == "sleeper  app  running"),
        "expected sleeper running in:\n{ps1_txt}"
    );

    let kill_out = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .args(["kill", "sleeper"])
        .output()
        .unwrap();
    assert_eq!(
        kill_out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&kill_out.stderr)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut exited_137 = false;
    while Instant::now() < deadline {
        let ps2 = Command::new(exe)
            .args(["--store"])
            .arg(&store)
            .arg("ps")
            .output()
            .unwrap();
        let ps2_txt = String::from_utf8_lossy(&ps2.stdout);
        if ps2_txt.lines().any(|l| l == "sleeper  app  exited(137)") {
            exited_137 = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(exited_137, "sleeper did not reach exited(137) within 5s");

    let rm_out = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .args(["rm", "sleeper"])
        .output()
        .unwrap();
    assert_eq!(
        rm_out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rm_out.stderr)
    );

    let ps3 = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .arg("ps")
        .output()
        .unwrap();
    let ps3_txt = String::from_utf8_lossy(&ps3.stdout);
    assert!(!ps3_txt.contains("sleeper"), "{ps3_txt}");

    // Leader checks: a removed container cannot be killed; a stale "running" state whose supervisor and pid
    // are gone reads as exited(-1), and kill refuses it.
    let pd = |args: &[&str]| {
        Command::new(exe)
            .args(["--store"])
            .arg(&store)
            .args(args)
            .output()
            .unwrap()
    };
    assert_ne!(pd(&["kill", "sleeper"]).status.code(), Some(0));
    let ghost = store.join("containers").join("ghost");
    fs::create_dir_all(ghost.join("rootfs")).unwrap();
    fs::write(
        ghost.join("state"),
        "image=app
status=running
pid=4194000
",
    )
    .unwrap();
    let ps4 = String::from_utf8_lossy(&pd(&["ps"]).stdout).into_owned();
    assert!(ps4.lines().any(|l| l == "ghost  app  exited(-1)"), "{ps4}");
    assert_ne!(pd(&["kill", "ghost"]).status.code(), Some(0));
    assert_eq!(pd(&["rm", "ghost"]).status.code(), Some(0));

    reg.stop();
    let _ = fs::remove_dir_all(&dir);
}
