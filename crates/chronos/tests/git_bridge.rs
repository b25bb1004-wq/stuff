//! Import/export through the git binary: trees, commit metadata, refs, and export round trips.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chronos::git;
use chronos::{decode_commit, from_hex, Id, MemStore, Repo, Signature, Store};

static SEQ: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "chronos_git_{label}_{}_{nanos}_{seq}",
            std::process::id()
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

/// True when a git binary runs; otherwise prints why the tests are skipped and returns false.
fn git_available() -> bool {
    let ok = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("SKIPPED git bridge tests: no git binary on PATH");
    }
    ok
}

fn git_bytes(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn git_text(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(git_bytes(dir, args))
        .expect("utf-8 output")
        .trim_end()
        .to_string()
}

fn git_run(dir: &Path, args: &[&str], date: Option<&str>) {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Zoe Tester")
        .env("GIT_AUTHOR_EMAIL", "zoe@example.test")
        .env("GIT_COMMITTER_NAME", "Zoe Tester")
        .env("GIT_COMMITTER_EMAIL", "zoe@example.test");
    if let Some(d) = date {
        cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
    }
    let out = cmd.output().expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(dir: &Path, rel: &str, bytes: &[u8]) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, bytes).unwrap();
}

/// A git repo with branches, a merge, a rename, a binary file, an exec bit, tags and a
/// non-ASCII message.
fn build_source(dir: &Path) {
    git_run(dir, &["init", "-q", "-b", "main"], None);
    write(dir, "a.txt", b"alpha\n");
    git_run(dir, &["add", "a.txt"], None);
    git_run(
        dir,
        &["commit", "-q", "-m", "first"],
        Some("2024-05-01T10:00:00+0530"),
    );

    let binary: Vec<u8> = (0..=255u8).chain([0, 255, 0]).collect();
    write(dir, "bin/data.dat", &binary);
    write(dir, "run.sh", b"#!/bin/sh\necho hi\n");
    git_run(dir, &["add", "bin/data.dat", "run.sh"], None);
    git_run(dir, &["update-index", "--chmod=+x", "run.sh"], None);
    git_run(
        dir,
        &["commit", "-q", "-m", "binary and exec"],
        Some("2024-05-02T11:30:00-0700"),
    );

    git_run(dir, &["checkout", "-q", "-b", "feature"], None);
    write(dir, "feature.txt", b"feature line\n");
    git_run(dir, &["add", "feature.txt"], None);
    git_run(
        dir,
        &[
            "commit",
            "-q",
            "-m",
            "h\u{e9}llo \u{65e5}\u{672c}\u{8a9e} \u{2713} feature",
        ],
        Some("2024-05-03T09:15:00+0000"),
    );

    git_run(dir, &["checkout", "-q", "main"], None);
    write(dir, "main.txt", b"main line\n");
    git_run(dir, &["add", "main.txt"], None);
    git_run(
        dir,
        &["commit", "-q", "-m", "main work"],
        Some("2024-05-04T08:00:00+0200"),
    );

    git_run(
        dir,
        &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"],
        Some("2024-05-05T12:00:00+0000"),
    );

    fs::create_dir_all(dir.join("renamed")).unwrap();
    git_run(dir, &["mv", "a.txt", "renamed/a.txt"], None);
    git_run(
        dir,
        &["commit", "-q", "-m", "rename a"],
        Some("2024-05-06T07:45:00+0000"),
    );

    write(dir, "bin/data.dat", b"changed binary\0\x01");
    git_run(dir, &["add", "bin/data.dat"], None);
    git_run(
        dir,
        &["commit", "-q", "-m", "edit binary"],
        Some("2024-05-07T06:00:00+0000"),
    );

    git_run(dir, &["tag", "v1"], None);
    git_run(dir, &["tag", "-a", "v2", "-m", "annotated release"], None);
}

fn fmt_tz(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.abs();
    format!("{sign}{:02}{:02}", abs / 60, abs % 60)
}

/// Files of a git commit as the git binary sees them: path -> (bytes, executable).
fn git_files(dir: &Path, sha: &str) -> BTreeMap<String, (Vec<u8>, bool)> {
    let listing = git_bytes(dir, &["ls-tree", "-r", "-z", sha]);
    let mut out = BTreeMap::new();
    for record in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record.iter().position(|b| *b == b'\t').expect("tab");
        let meta = String::from_utf8(record[..tab].to_vec()).unwrap();
        let path = String::from_utf8(record[tab + 1..].to_vec()).unwrap();
        let fields: Vec<&str> = meta.split(' ').collect();
        assert_eq!(fields[1], "blob", "only blobs expected in {path}");
        let content = git_bytes(dir, &["cat-file", "blob", fields[2]]);
        out.insert(path, (content, fields[0] == "100755"));
    }
    out
}

/// Parse `commit <sha> <chronos-hex>` lines of the store's git-map file.
fn git_map(store: &impl Store) -> BTreeMap<String, Id> {
    let raw = store.read("git-map").expect("git-map written");
    let mut out = BTreeMap::new();
    for line in String::from_utf8(raw).unwrap().lines() {
        let parts: Vec<&str> = line.split(' ').collect();
        if let ["commit", sha, hex] = parts.as_slice() {
            out.insert(sha.to_string(), from_hex(hex).expect("hex id"));
        }
    }
    out
}

#[test]
fn import_matches_git_commit_by_commit() {
    if !git_available() {
        return;
    }
    let src = TempDir::new("src");
    build_source(src.path());

    let mut repo = Repo::init(MemStore::new());
    let summary = git::import(&mut repo, src.path()).expect("import");
    assert!(summary.warnings.iter().any(|w| w.contains("v2")));

    let map = git_map(&repo.store);
    let shas: Vec<String> = git_text(src.path(), &["rev-list", "--all"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(map.len(), shas.len(), "every git commit is mapped");
    assert_eq!(summary.commits, shas.len());

    for sha in &shas {
        let id = map[sha];
        let (_, raw) = repo.get(&id).expect("chronos commit");
        let commit = decode_commit(&raw).expect("decode");

        assert_eq!(
            repo.files(&id).expect("files"),
            git_files(src.path(), sha),
            "tree of {sha}"
        );

        let parents: Vec<Id> = git_text(src.path(), &["rev-list", "--parents", "-n", "1", sha])
            .split_whitespace()
            .skip(1)
            .map(|p| map[p])
            .collect();
        assert_eq!(commit.parents, parents, "parents of {sha}");

        let raw_git = git_bytes(src.path(), &["cat-file", "commit", sha]);
        let split = raw_git
            .windows(2)
            .position(|w| w == b"\n\n")
            .expect("header end");
        let header = String::from_utf8(raw_git[..split].to_vec()).unwrap();
        let message = &raw_git[split + 2..];
        assert_eq!(commit.message.as_bytes(), message, "message of {sha}");
        for (key, sig) in [("author", &commit.author), ("committer", &commit.committer)] {
            let line = format!(
                "{key} {} <{}> {} {}",
                sig.name,
                sig.email,
                sig.time,
                fmt_tz(sig.tz_minutes)
            );
            assert!(header.contains(&line), "{key} of {sha}: want {line}");
        }
    }

    let branches: BTreeMap<String, Id> = repo.branches().into_iter().collect();
    assert_eq!(branches.len(), 2, "main and feature");
    let main_tip = git_text(src.path(), &["rev-parse", "main"]);
    assert_eq!(branches["main"], map[&main_tip]);
    let feature_tip = git_text(src.path(), &["rev-parse", "feature"]);
    assert_eq!(branches["feature"], map[&feature_tip]);

    let v1 = git_text(src.path(), &["rev-parse", "v1^{commit}"]);
    assert_eq!(repo.store.read("refs/tags/v1").map(|r| r.len()), Some(65));
    let v1_raw = repo.store.read("refs/tags/v1").unwrap();
    assert_eq!(
        from_hex(String::from_utf8(v1_raw).unwrap().trim()),
        Some(map[&v1])
    );

    assert!(repo.status().is_empty(), "index matches HEAD after import");
}

#[test]
fn import_is_idempotent_and_export_round_trips() {
    if !git_available() {
        return;
    }
    let src = TempDir::new("rt");
    build_source(src.path());
    let mut repo = Repo::init(MemStore::new());
    git::import(&mut repo, src.path()).expect("import");
    let map_before = repo.store.read("git-map");
    let again = git::import(&mut repo, src.path()).expect("second import");
    assert_eq!(again.commits, 0, "second import creates nothing");
    assert_eq!(repo.store.read("git-map"), map_before);

    // Nothing new: export is a no-op and does not touch git.
    let head_before = git_text(src.path(), &["rev-parse", "HEAD"]);
    let quiet = git::export(&mut repo, src.path()).expect("export nothing");
    assert_eq!(quiet, git::Summary::default());
    assert_eq!(git_text(src.path(), &["rev-parse", "HEAD"]), head_before);

    // A chronos commit on top of main, with a new file and a changed binary.
    repo.add("notes/new.txt", b"from chronos\n", false).unwrap();
    let who = Signature {
        name: "Chronos Tester".to_string(),
        email: "chronos@example.test".to_string(),
        time: 1_720_000_000,
        tz_minutes: 330,
    };
    let id = repo
        .commit("chronos message\n\nsecond line", &who)
        .expect("commit");

    let summary = git::export(&mut repo, src.path()).expect("export");
    assert_eq!(summary.commits, 1);
    assert_eq!(summary.refs, 1);

    assert_eq!(
        git_text(src.path(), &["log", "-1", "--format=%an <%ae>|%at|%s"]),
        "Chronos Tester <chronos@example.test>|1720000000|chronos message"
    );
    assert!(git_text(src.path(), &["log", "-1", "--format=%ai"]).ends_with(" +0530"));
    let raw_commit = git_bytes(src.path(), &["cat-file", "commit", "HEAD"]);
    assert!(
        raw_commit.ends_with(b"\n\nchronos message\n\nsecond line"),
        "message kept verbatim"
    );
    assert_eq!(
        git_bytes(src.path(), &["show", "HEAD:notes/new.txt"]),
        b"from chronos\n".to_vec()
    );
    assert_eq!(
        git_text(src.path(), &["status", "--porcelain"]),
        "",
        "working tree and index match the new commit"
    );
    let git_head = git_text(src.path(), &["rev-parse", "HEAD"]);
    let map = git_map(&repo.store);
    assert_eq!(map.get(&git_head), Some(&id));

    // Second export is a no-op.
    let again = git::export(&mut repo, src.path()).expect("second export");
    assert_eq!(again, git::Summary::default());
    assert_eq!(git_text(src.path(), &["rev-parse", "HEAD"]), git_head);
    assert_eq!(git_text(src.path(), &["rev-list", "--count", "--all"]), "8");
}

#[test]
fn export_refuses_dirty_checked_out_branch() {
    if !git_available() {
        return;
    }
    let src = TempDir::new("dirty");
    build_source(src.path());
    let mut repo = Repo::init(MemStore::new());
    git::import(&mut repo, src.path()).expect("import");
    let who = Signature {
        name: "Chronos Tester".to_string(),
        email: "chronos@example.test".to_string(),
        time: 1_720_000_000,
        tz_minutes: 0,
    };
    repo.add("extra.txt", b"x\n", false).unwrap();
    repo.commit("extra", &who).unwrap();

    write(src.path(), "main.txt", b"local edit\n");
    let head_before = git_text(src.path(), &["rev-parse", "HEAD"]);
    let err = git::export(&mut repo, src.path()).expect_err("dirty tree is refused");
    assert!(err.to_string().contains("uncommitted"), "{err}");
    assert_eq!(git_text(src.path(), &["rev-parse", "HEAD"]), head_before);
}
