//! Real containers on Linux: a rootfs holding /bin/sh and its libraries, run through the pandora binary.
//! Skips (with a printed reason) where the kernel blocks unprivileged user namespaces.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn rootfs(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pandora-rootfs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let sh = fs::canonicalize("/bin/sh").unwrap();
    let mut files = vec![sh.clone()];
    let ldd = Command::new("ldd").arg(&sh).output().unwrap();
    for line in String::from_utf8_lossy(&ldd.stdout).lines() {
        if let Some(p) = line.split_whitespace().find(|w| w.starts_with('/')) {
            files.push(PathBuf::from(p));
        }
    }
    for f in &files {
        let dst = root.join(f.strip_prefix("/").unwrap());
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::copy(f, &dst).unwrap();
    }
    let bin_sh = root.join("bin/sh");
    if !bin_sh.exists() {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::copy(&sh, &bin_sh).unwrap();
    }
    root
}

fn pandora(root: &Path, extra: &[&str], script: &str) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pandora"));
    c.arg("run")
        .arg("--rootfs")
        .arg(root)
        .args(extra)
        .args(["--", "/bin/sh", "-c", script]);
    c.output().unwrap()
}

/// None when user namespaces are blocked here (prints why), else the run's output.
fn run_or_skip(root: &Path, extra: &[&str], script: &str) -> Option<Output> {
    let out = pandora(root, extra, script);
    let err = String::from_utf8_lossy(&out.stderr);
    // Blocked outright (unshare), or allowed but powerless (AppArmor's restrict_unprivileged_userns makes the
    // first mount or the id-map write fail). Anything later in setup is a real failure.
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

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn isolated_pid_hostname_root_and_env() {
    let root = rootfs("iso");
    let script = "echo pid=$$; read h < /proc/sys/kernel/hostname; echo host=$h; \
                  for f in /*; do echo entry=$f; done; echo foo=$FOO; echo home=${HOME-unset}";
    let Some(out) = run_or_skip(&root, &["--hostname", "box7", "--env", "FOO=bar"], script) else {
        return;
    };
    let t = text(&out);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{t}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(t.contains("pid=1\n"), "{t}");
    assert!(t.contains("host=box7\n"), "{t}");
    assert!(t.contains("foo=bar\n") && t.contains("home=unset\n"), "{t}");
    let entries: Vec<&str> = t.lines().filter_map(|l| l.strip_prefix("entry=")).collect();
    assert!(
        entries.contains(&"/bin") && entries.contains(&"/proc"),
        "{entries:?}"
    );
    assert!(
        !entries.contains(&"/.oldroot")
            && !entries.contains(&"/home")
            && !entries.contains(&"/mnt"),
        "{entries:?}"
    );
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    assert_ne!(host.trim(), "box7", "hostname leaked to the host");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn exit_codes_and_signals_propagate() {
    let root = rootfs("exit");
    let Some(out) = run_or_skip(&root, &[], "exit 7") else {
        return;
    };
    assert_eq!(out.status.code(), Some(7));
    // PID 1 of a namespace ignores SIGKILL sent from inside it; a killed child reports 128 + 9 through the shell.
    let out = pandora(
        &root,
        &[],
        "kill -9 $$; echo alive; /bin/sh -c 'kill -9 $$'; exit $?",
    );
    assert_eq!(text(&out), "alive\n");
    assert_eq!(out.status.code(), Some(128 + 9));
    let mut c = Command::new(env!("CARGO_BIN_EXE_pandora"));
    let out = c
        .arg("run")
        .arg("--rootfs")
        .arg(&root)
        .args(["--", "/no/such/program"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&out.stderr).contains("exec failed"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn proc_shows_only_the_container() {
    let root = rootfs("proc");
    let script = "n=0; for d in /proc/[0-9]*; do n=$((n+1)); done; echo procs=$n";
    let Some(out) = run_or_skip(&root, &[], script) else {
        return;
    };
    assert_eq!(
        text(&out).trim(),
        "procs=1",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn bad_input_is_rejected_before_any_namespace() {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pandora"));
    let out = c
        .args(["run", "--rootfs", "relative/dir", "--", "/bin/sh"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(125));
    let mut c = Command::new(env!("CARGO_BIN_EXE_pandora"));
    let out = c
        .args([
            "run", "--rootfs", "/tmp", "--memory", "12X", "--", "/bin/sh",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(125));
    let mut c = Command::new(env!("CARGO_BIN_EXE_pandora"));
    let out = c.args(["nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(125));
}

#[test]
fn limits_without_cgroup_access_fail_cleanly() {
    let root = rootfs("cg");
    let missing = std::env::temp_dir().join(format!("pandora-no-cgroup-{}", std::process::id()));
    let out = pandora(
        &root,
        &["--pids", "8", "--cgroup-base", missing.to_str().unwrap()],
        "echo hi",
    );
    assert_eq!(out.status.code(), Some(125));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cgroup v2"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text(&out).is_empty(),
        "the container must not start without its cgroup"
    );
    let _ = fs::remove_dir_all(&root);
}
