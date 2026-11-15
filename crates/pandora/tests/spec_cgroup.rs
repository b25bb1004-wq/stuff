//! Spec validation, size parsing and cgroup file writing (runs on every platform).
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;

use pandora::{parse_size, Cgroup, Limits, Spec, SpecError};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pandora-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn sizes() {
    assert_eq!(parse_size("0"), Ok(Some(0)));
    assert_eq!(parse_size("4096"), Ok(Some(4096)));
    assert_eq!(parse_size("64M"), Ok(Some(64 << 20)));
    assert_eq!(parse_size("2k"), Ok(Some(2048)));
    assert_eq!(parse_size("1G"), Ok(Some(1 << 30)));
    assert_eq!(parse_size("max"), Ok(None));
    for bad in [
        "",
        "M",
        "-1",
        "1.5M",
        "12X",
        " 1",
        "18446744073709551616",
        "17179869184G",
    ] {
        assert!(
            matches!(parse_size(bad), Err(SpecError::BadNumber(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn sizes_never_panic() {
    assay::forall(2000, 0x9A2D_0051, |r| {
        let mut b = vec![0u8; r.below(24)];
        r.fill(&mut b);
        let s = String::from_utf8_lossy(&b);
        let _ = parse_size(&s);
    });
}

#[test]
fn validation() {
    let ok = Spec::new("/srv/box", vec!["/bin/sh".into()]);
    assert_eq!(ok.validate(), Ok(()));
    let mut s = ok.clone();
    s.rootfs = "srv/box".into();
    assert_eq!(s.validate(), Err(SpecError::RootfsNotAbsolute));
    let mut s = ok.clone();
    s.argv.clear();
    assert_eq!(s.validate(), Err(SpecError::NoCommand));
    for h in ["", "-x", "a b", "a/b", &"x".repeat(65)] {
        let mut s = ok.clone();
        s.hostname = h.to_string();
        assert_eq!(s.validate(), Err(SpecError::BadHostname), "{h:?}");
    }
    let mut s = ok.clone();
    s.hostname = "web-1.local".into();
    assert_eq!(s.validate(), Ok(()));
    let mut s = ok.clone();
    s.env.push(("A=B".into(), "c".into()));
    assert_eq!(s.validate(), Err(SpecError::BadEnvKey));
    let mut s = ok.clone();
    s.argv.push("a\0b".into());
    assert_eq!(s.validate(), Err(SpecError::NulByte));
    for cpu in [(0, 100_000), (50_000, 999), (1, 1_000_001)] {
        let mut s = ok.clone();
        s.limits.cpu_max = Some(cpu);
        assert_eq!(s.validate(), Err(SpecError::BadCpu), "{cpu:?}");
    }
}

#[test]
fn cgroup_writes_limits_and_procs() {
    let base = tmp("cg");
    let limits = Limits {
        memory_max: Some(64 << 20),
        pids_max: Some(32),
        cpu_max: Some((50_000, 100_000)),
    };
    let cg = Cgroup::create(&base, "box1", &limits).unwrap();
    assert_eq!(
        fs::read_to_string(base.join("cgroup.subtree_control")).unwrap(),
        "+memory +pids +cpu"
    );
    assert_eq!(
        fs::read_to_string(cg.path().join("memory.max")).unwrap(),
        "67108864"
    );
    assert_eq!(
        fs::read_to_string(cg.path().join("pids.max")).unwrap(),
        "32"
    );
    assert_eq!(
        fs::read_to_string(cg.path().join("cpu.max")).unwrap(),
        "50000 100000"
    );
    cg.add(4242).unwrap();
    assert_eq!(
        fs::read_to_string(cg.path().join("cgroup.procs")).unwrap(),
        "4242"
    );
    assert!(
        Cgroup::create(&base, "box1", &limits).is_err(),
        "existing group must not be reused"
    );
    drop(cg);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn cgroup_only_writes_what_is_set() {
    let base = tmp("cg2");
    let limits = Limits {
        pids_max: Some(8),
        ..Limits::default()
    };
    let cg = Cgroup::create(&base, "only-pids", &limits).unwrap();
    assert_eq!(
        fs::read_to_string(base.join("cgroup.subtree_control")).unwrap(),
        "+pids"
    );
    assert!(cg.path().join("pids.max").exists());
    assert!(!cg.path().join("memory.max").exists());
    assert!(Limits::default().is_empty() && Limits::default().files().is_empty());
    drop(cg);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn cgroup_rejects_bad_names() {
    let base = tmp("cg3");
    for name in ["", ".", "..", "a/b", "a\\b", "x\0"] {
        assert!(
            Cgroup::create(&base, name, &Limits::default()).is_err(),
            "{name:?}"
        );
    }
    let _ = fs::remove_dir_all(&base);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn run_is_unsupported_off_linux() {
    let s = Spec::new("/srv/box", vec!["/bin/sh".into()]);
    let e = pandora::run(&s, std::path::Path::new("/sys/fs/cgroup")).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::Unsupported);
}
