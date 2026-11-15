//! Integration tests for image pull, rootfs assembly, path rejection, state, and CLI (all platforms).
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use argo::manifest::Manifest;
use argo::pkg::{self, Package};
use argo::registry::Registry;
use argo::version::{Req, Version};
use pandora::image;
use pandora::state::{self, Status};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pandora-img-{name}-{}", std::process::id()));
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
    let blob = pkg::sign(p, seed).expect("sign test package");
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

fn raw_signed_blob(seed: &[u8; 32], manifest: &Manifest, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mstr = manifest.to_string();
    let mut body = Vec::new();
    body.extend_from_slice(b"CAIRNPKG1\n");
    body.extend_from_slice(&(mstr.len() as u32).to_le_bytes());
    body.extend_from_slice(mstr.as_bytes());
    body.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (path, data) in files {
        body.extend_from_slice(&(path.len() as u16).to_le_bytes());
        body.extend_from_slice(path.as_bytes());
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
    }
    let pk = sphinx::ed25519::public_key(seed);
    let sig = sphinx::ed25519::sign(seed, &body);
    let mut blob = Vec::new();
    blob.extend_from_slice(b"CAIRNSIG1\n");
    blob.extend_from_slice(&pk);
    blob.extend_from_slice(&sig);
    blob.extend_from_slice(&body);
    blob
}

#[test]
fn pull_and_build_rootfs_override_and_order() {
    let dir = tmp("pull_override");
    let store = dir.join("store");
    let dest = dir.join("rootfs");
    let seed = [0x42u8; 32];
    let pk = sphinx::ed25519::public_key(&seed);

    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    let base = Package {
        manifest: Manifest {
            name: "base".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![],
        },
        files: vec![
            ("bin/tool".into(), b"base-tool".to_vec()),
            ("etc/base.conf".into(), b"base=1".to_vec()),
        ],
    };
    let app = Package {
        manifest: Manifest {
            name: "app".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![("base".into(), Req::parse("^1").unwrap())],
        },
        files: vec![
            ("app/run.sh".into(), b"#!/bin/sh\necho hi\n".to_vec()),
            ("bin/tool".into(), b"app-override".to_vec()),
        ],
    };
    publish(reg.addr(), &seed, &base);
    publish(reg.addr(), &seed, &app);

    let sol = image::pull(reg.addr(), "app", &Req::parse("*").unwrap(), &store, &[pk]).unwrap();
    assert_eq!(sol.get("base"), Some(&Version::parse("1.0.0").unwrap()));
    assert_eq!(sol.get("app"), Some(&Version::parse("1.0.0").unwrap()));

    image::build_rootfs(&store, "app", &dest).unwrap();
    assert_eq!(fs::read(dest.join("bin/tool")).unwrap(), b"app-override");
    assert_eq!(fs::read(dest.join("etc/base.conf")).unwrap(), b"base=1");
    assert_eq!(
        fs::read(dest.join("app/run.sh")).unwrap(),
        b"#!/bin/sh\necho hi\n"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tool_mode = fs::metadata(dest.join("bin/tool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let sh_mode = fs::metadata(dest.join("app/run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let cfg_mode = fs::metadata(dest.join("etc/base.conf"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(tool_mode, 0o755);
        assert_eq!(sh_mode, 0o755);
        assert_eq!(cfg_mode, 0o644);
    }

    reg.stop();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_rootfs_topo_ties_broken_by_name() {
    let dir = tmp("topo_ties");
    let store = dir.join("store");
    let dest = dir.join("rootfs");
    let seed = [0x43u8; 32];
    let pk = sphinx::ed25519::public_key(&seed);

    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    let adep = Package {
        manifest: Manifest {
            name: "adep".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![],
        },
        files: vec![("shared.txt".into(), b"from-adep".to_vec())],
    };
    let zdep = Package {
        manifest: Manifest {
            name: "zdep".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![],
        },
        files: vec![("shared.txt".into(), b"from-zdep".to_vec())],
    };
    let app = Package {
        manifest: Manifest {
            name: "app".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![
                ("zdep".into(), Req::parse("^1").unwrap()),
                ("adep".into(), Req::parse("^1").unwrap()),
            ],
        },
        files: vec![("app.txt".into(), b"app".to_vec())],
    };
    publish(reg.addr(), &seed, &adep);
    publish(reg.addr(), &seed, &zdep);
    publish(reg.addr(), &seed, &app);

    image::pull(reg.addr(), "app", &Req::parse("*").unwrap(), &store, &[pk]).unwrap();
    image::build_rootfs(&store, "app", &dest).unwrap();
    // adep sorts before zdep, so zdep overwrites adep.
    assert_eq!(fs::read(dest.join("shared.txt")).unwrap(), b"from-zdep");
    assert_eq!(fs::read(dest.join("app.txt")).unwrap(), b"app");

    reg.stop();
    let _ = fs::remove_dir_all(&dir);
}

fn serve_once_evil(blob: Vec<u8>) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            if req.starts_with("GET /index ") {
                let body = b"evil\t1.0.0\n";
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(hdr.as_bytes());
                let _ = s.write_all(body);
            } else {
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    blob.len()
                );
                let _ = s.write_all(hdr.as_bytes());
                let _ = s.write_all(&blob);
            }
        }
    });
    (addr, handle)
}

#[test]
fn hostile_paths_rejected_and_nothing_escapes() {
    let dir = tmp("hostile");
    let seed = [0x44u8; 32];
    let pk = sphinx::ed25519::public_key(&seed);
    let manifest = Manifest {
        name: "evil".into(),
        version: Version::parse("1.0.0").unwrap(),
        dependencies: vec![],
    };

    for (idx, bad_path) in ["../x", "/x"].iter().enumerate() {
        let sub = dir.join(format!("case-{idx}"));
        let store = sub.join("store");
        fs::create_dir_all(&sub).unwrap();
        let blob = raw_signed_blob(&seed, &manifest, &[(bad_path, b"pwned")]);
        let (addr, handle) = serve_once_evil(blob);
        let res = image::pull(addr, "evil", &Req::parse("*").unwrap(), &store, &[pk]);
        let _ = handle.join();
        assert!(res.is_err(), "expected error for {bad_path:?}");
        assert!(
            !sub.join("x").exists(),
            "escaped file written for {bad_path:?}"
        );
        assert!(
            !dir.join("x").exists(),
            "escaped file written for {bad_path:?}"
        );
    }

    // Also test symlink-like regular filenames work and a hostile symlink inside store/pkgs is rejected.
    let store = dir.join("sym_store");
    let dest = dir.join("sym_dest");
    let outside = dir.join("outside_secret");
    fs::write(&outside, b"keep").unwrap();
    let pkgs = store.join("pkgs").join("ok-1.0.0");
    fs::create_dir_all(pkgs.join("app")).unwrap();
    let m = Manifest {
        name: "ok".into(),
        version: Version::parse("1.0.0").unwrap(),
        dependencies: vec![],
    };
    fs::write(store.join("pkgs").join("ok-1.0.0.manifest"), m.to_string()).unwrap();
    let mut sol = argo::solve::Solution::new();
    sol.insert("ok".into(), Version::parse("1.0.0").unwrap());
    fs::write(store.join("argo.lock"), argo::lock::write(&sol)).unwrap();
    fs::write(pkgs.join("app/..hidden"), b"normal2").unwrap();
    fs::write(pkgs.join("app/a..b.txt"), b"normal3").unwrap();
    #[cfg(unix)]
    fs::write(pkgs.join("app/foo -> bar"), b"normal").unwrap();

    image::build_rootfs(&store, "ok", &dest).unwrap();
    assert_eq!(fs::read(dest.join("app/..hidden")).unwrap(), b"normal2");
    assert_eq!(fs::read(dest.join("app/a..b.txt")).unwrap(), b"normal3");
    #[cfg(unix)]
    assert_eq!(fs::read(dest.join("app/foo -> bar")).unwrap(), b"normal");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, pkgs.join("escape")).unwrap();
        let dest2 = dir.join("sym_dest2");
        assert!(image::build_rootfs(&store, "ok", &dest2).is_err());
        assert!(!dest2.join("escape").exists());
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn state_round_trip_list_order_duplicate_and_atomic() {
    let dir = tmp("state");
    let store = dir.join("store");

    let st_z = state::create(&store, "zeta", "app").unwrap();
    assert_eq!(st_z.status, Status::Created);
    assert!(!state::container_dir(&store, "zeta")
        .join("state.tmp")
        .exists());

    let mut st_a = state::create(&store, "alpha", "base").unwrap();
    let dup = state::create(&store, "alpha", "base").unwrap_err();
    assert_eq!(dup.kind(), std::io::ErrorKind::AlreadyExists);

    st_a.status = Status::Running;
    st_a.pid = Some(std::process::id() as i32);
    state::write(&store, &st_a).unwrap();
    assert!(!state::container_dir(&store, "alpha")
        .join("state.tmp")
        .exists());

    let read_a = state::read(&store, "alpha").unwrap();
    assert_eq!(read_a, st_a);

    let list = state::list(&store).unwrap();
    let ids: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "zeta"]);

    // Running container cannot be removed.
    assert!(state::remove(&store, "alpha").is_err());

    st_a.status = Status::Exited;
    st_a.exit = Some(0);
    state::write(&store, &st_a).unwrap();
    state::remove(&store, "alpha").unwrap();
    assert!(!state::container_dir(&store, "alpha").exists());

    #[cfg(target_os = "linux")]
    {
        let dead = state::ContainerState {
            id: "zeta".into(),
            image: "app".into(),
            status: Status::Running,
            pid: Some(999_999_999),
            exit: None,
        };
        state::write(&store, &dead).unwrap();
        let reconciled = state::read(&store, "zeta").unwrap();
        assert_eq!(reconciled.status, Status::Exited);
        assert_eq!(reconciled.exit, Some(-1));
        assert_eq!(reconciled.ps_status(), "exited(-1)");
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_pull_ps_kill_rm_behavior() {
    let dir = tmp("cli");
    let store = dir.join("store");
    let seed = [0x45u8; 32];
    let pk = sphinx::ed25519::public_key(&seed);

    let reg = Registry::start("127.0.0.1:0", vec![pk]).unwrap();
    let pkg = Package {
        manifest: Manifest {
            name: "tiny".into(),
            version: Version::parse("1.0.0").unwrap(),
            dependencies: vec![],
        },
        files: vec![("bin/hi".into(), b"hi".to_vec())],
    };
    publish(reg.addr(), &seed, &pkg);

    let exe = env!("CARGO_BIN_EXE_pandora");
    let out = Command::new(exe)
        .args(["pull", "--store"])
        .arg(&store)
        .args([
            "--registry",
            &reg.addr().to_string(),
            "--trust",
            &hex64(&pk),
            "tiny",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    state::create(&store, "box1", "tiny").unwrap();

    let out = Command::new(exe)
        .args(["--store"])
        .arg(&store)
        .args(["ps"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "ID  IMAGE  STATUS");
    assert_eq!(lines[1], "box1  tiny  created");

    // kill on non-running container fails with 125.
    let out = Command::new(exe)
        .args(["kill", "--store"])
        .arg(&store)
        .args(["box1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(125));

    // non-numeric signal fails with 125.
    let out = Command::new(exe)
        .args(["kill", "--store"])
        .arg(&store)
        .args(["box1", "SIGKILL"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(125));

    // rm works on created container.
    let out = Command::new(exe)
        .args(["rm", "--store"])
        .arg(&store)
        .args(["box1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(Path::new(&store).join("argo.lock").exists());

    reg.stop();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_rootfs_handles_many_files_and_deep_paths() {
    let dir = tmp("many");
    let store = dir.join("store");
    let dest = dir.join("rootfs");
    let pkg_dir = store.join("pkgs").join("big-1.0.0");
    fs::create_dir_all(&pkg_dir).unwrap();
    let m = Manifest {
        name: "big".into(),
        version: Version::parse("1.0.0").unwrap(),
        dependencies: vec![],
    };
    fs::write(store.join("pkgs").join("big-1.0.0.manifest"), m.to_string()).unwrap();
    let mut sol = argo::solve::Solution::new();
    sol.insert("big".into(), Version::parse("1.0.0").unwrap());
    fs::write(store.join("argo.lock"), argo::lock::write(&sol)).unwrap();

    // 10k files spread across 100 directories.
    for d in 0..100 {
        let sub = pkg_dir.join(format!("d{d:02}"));
        fs::create_dir_all(&sub).unwrap();
        for f in 0..100 {
            fs::write(sub.join(format!("f{f:02}")), b"x").unwrap();
        }
    }
    // Deep nested path.
    let mut deep = pkg_dir.clone();
    for i in 0..20 {
        deep = deep.join(format!("n{i:02}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf.txt"), b"deep").unwrap();

    image::build_rootfs(&store, "big", &dest).unwrap();
    assert_eq!(fs::read(dest.join("d99/f99")).unwrap(), b"x");
    let mut dest_deep = dest.clone();
    for i in 0..20 {
        dest_deep = dest_deep.join(format!("n{i:02}"));
    }
    assert_eq!(fs::read(dest_deep.join("leaf.txt")).unwrap(), b"deep");

    let _ = fs::remove_dir_all(&dir);
}
