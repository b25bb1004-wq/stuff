//! Integration tests for chronos: scripted history, validation, DirStore equivalence, corruption.

#![allow(clippy::unwrap_used)]

use assay::forall;
use chronos::{to_hex, DirStore, Error, Files, Id, Kind, MemStore, Repo, Signature, Store};

fn sig(name: &str, time: i64) -> Signature {
    Signature {
        name: name.to_string(),
        email: format!("{name}@example.com"),
        time,
        tz_minutes: 0,
    }
}

fn run_script<S: Store>(store: S) -> (Vec<Id>, Files, Files, Files) {
    let mut repo = Repo::init(store);
    assert_eq!(repo.head(), None);
    assert_eq!(repo.current_branch().as_deref(), Some("main"));

    let mut commits = Vec::new();
    // Commit 1 (main): nested paths + exec bit
    repo.add("README.md", b"hello\n", false).unwrap();
    repo.add("bin/run.sh", b"#!/bin/sh\necho hi\n", true)
        .unwrap();
    repo.add("src/lib.rs", b"pub fn a() {}\n", false).unwrap();
    let c1 = repo.commit("c1: init", &sig("alice", 1000)).unwrap();
    commits.push(c1);

    // Commit 2 (main): modify and add
    repo.add("src/mod/x.rs", b"pub fn x() {}\n", false).unwrap();
    let c2 = repo.commit("c2: add mod/x", &sig("alice", 1010)).unwrap();
    commits.push(c2);

    // Commit 3 (main): empty commit (--allow-empty)
    let c3 = repo.commit("c3: empty", &sig("alice", 1020)).unwrap();
    commits.push(c3);

    // Commit 4 (main): rm a file
    repo.rm("src/lib.rs").unwrap();
    let c4 = repo.commit("c4: rm lib.rs", &sig("bob", 1030)).unwrap();
    commits.push(c4);

    // Branch feat-a at c2
    repo.branch("feat-a", &c2).unwrap();
    let f_a_checkout = repo.checkout("feat-a").unwrap();
    assert_eq!(repo.current_branch().as_deref(), Some("feat-a"));

    // Commits 5..8 on feat-a
    repo.add("feat_a/one.txt", b"1", false).unwrap();
    let c5 = repo.commit("c5: feat-a 1", &sig("bob", 1040)).unwrap();
    commits.push(c5);

    repo.add("feat_a/two.txt", b"2", false).unwrap();
    let c6 = repo.commit("c6: feat-a 2", &sig("bob", 1050)).unwrap();
    commits.push(c6);

    repo.add("feat_a/three.txt", b"3", true).unwrap();
    let c7 = repo.commit("c7: feat-a 3", &sig("bob", 1060)).unwrap();
    commits.push(c7);

    repo.rm("feat_a/one.txt").unwrap();
    let c8 = repo.commit("c8: feat-a 4", &sig("bob", 1070)).unwrap();
    commits.push(c8);

    // Branch feat-b at c4
    repo.branch("feat-b", &c4).unwrap();
    repo.checkout("feat-b").unwrap();
    assert_eq!(repo.current_branch().as_deref(), Some("feat-b"));

    // Commits 9..12 on feat-b
    repo.add("b/1.txt", b"b1", false).unwrap();
    let c9 = repo.commit("c9: feat-b 1", &sig("carol", 1080)).unwrap();
    commits.push(c9);

    repo.add("b/2.txt", b"b2", false).unwrap();
    let c10 = repo.commit("c10: feat-b 2", &sig("carol", 1090)).unwrap();
    commits.push(c10);

    repo.add("b/3.txt", b"b3", false).unwrap();
    let c11 = repo.commit("c11: feat-b 3", &sig("carol", 1100)).unwrap();
    commits.push(c11);

    repo.add("b/4.txt", b"b4", false).unwrap();
    let c12 = repo.commit("c12: feat-b 4", &sig("carol", 1110)).unwrap();
    commits.push(c12);

    // Checkout back and forth, including detached HEAD
    let main_files = repo.checkout("main").unwrap();
    assert_eq!(repo.current_branch().as_deref(), Some("main"));
    assert_eq!(repo.head(), Some(c4));

    // Resolve every rev form
    assert_eq!(repo.resolve("HEAD"), Some(c4));
    assert_eq!(repo.resolve("main"), Some(c4));
    assert_eq!(repo.resolve("feat-a"), Some(c8));
    assert_eq!(repo.resolve("feat-b"), Some(c12));
    assert_eq!(repo.resolve(&to_hex(&c7)), Some(c7));
    let prefix = &to_hex(&c7)[..8];
    assert_eq!(repo.resolve(prefix), Some(c7));
    assert_eq!(repo.resolve(&to_hex(&c7)[..3]), None);
    assert_eq!(repo.resolve("HEAD^"), Some(c3));
    assert_eq!(repo.resolve("HEAD~1"), Some(c3));
    assert_eq!(repo.resolve("HEAD~2"), Some(c2));
    assert_eq!(repo.resolve("HEAD~3"), Some(c1));
    assert_eq!(repo.resolve("HEAD~4"), None);
    assert_eq!(repo.resolve("feat-b~3^"), Some(c4));

    // Detached checkout by hex prefix
    let c6_files = repo.checkout(&to_hex(&c6)[..10]).unwrap();
    assert!(c6_files.contains_key("feat_a/two.txt"));
    assert_eq!(repo.current_branch(), None);
    assert_eq!(repo.head(), Some(c6));

    // Dirty refusal
    repo.add("dirty.txt", b"uncommitted", false).unwrap();
    assert_eq!(repo.checkout("main"), Err(Error::Dirty));
    assert_eq!(repo.staged().get("dirty.txt").unwrap().0, b"uncommitted");
    repo.rm("dirty.txt").unwrap();
    repo.checkout("main").unwrap();

    // Log order: newest committer time first
    let log_b = repo.log(&c12).unwrap();
    assert_eq!(log_b, vec![c12, c11, c10, c9, c4, c3, c2, c1]);

    // Branches & delete_branch
    assert_eq!(repo.branches().len(), 3);
    assert_eq!(repo.delete_branch("main"), Err(Error::Dirty));
    repo.delete_branch("feat-a").unwrap();
    assert_eq!(repo.branches().len(), 2);

    let f_b_files = repo.files(&c12).unwrap();
    (commits, f_a_checkout, main_files, f_b_files)
}

#[test]
fn memstore_and_dirstore_identical_script() {
    let mem_res = run_script(MemStore::new());

    let tmp = std::env::temp_dir().join(format!("drillcore_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir_res = run_script(DirStore::new(&tmp));
    let _ = std::fs::remove_dir_all(&tmp);

    assert_eq!(mem_res, dir_res);
    assert_eq!(mem_res.0.len(), 12);
}

#[test]
fn path_and_branch_validation_tables() {
    let mut repo = Repo::init(MemStore::new());
    let c1 = repo.commit("init", &sig("a", 1)).unwrap();

    let path_cases: &[(&str, bool)] = &[
        ("a", true),
        ("a/b", true),
        ("a/b/c.txt", true),
        ("dir.with.dots/file_name-1", true),
        ("a/b/c/d/e/f", true),
        (".hidden", true),
        ("dir/.gitkeep", true),
        ("123", true),
        ("README", true),
        ("a b/c d", true),
        ("", false),
        ("/", false),
        ("/abs", false),
        ("trailing/", false),
        ("a//b", false),
        (".", false),
        ("..", false),
        ("a/./b", false),
        ("a/../b", false),
        ("./a", false),
        ("../a", false),
        ("a/.", false),
        ("a/..", false),
        ("bad\0path", false),
    ];
    for &(p, valid) in path_cases {
        let res = repo.add(p, b"x", false);
        assert_eq!(res.is_ok(), valid, "path case {p:?}");
        if res.is_ok() {
            repo.rm(p).unwrap();
        }
    }

    assert_eq!(repo.branch("main", &c1), Err(Error::Exists));

    let branch_cases: &[(&str, bool)] = &[
        ("other-main", true),
        ("feat/one", true),
        ("v1.0.2", true),
        ("fix_123-abc", true),
        ("A/B/C", true),
        ("release-2026.11", true),
        ("topic/sub_topic/v2", true),
        ("a", true),
        ("a.b", true),
        ("a-b", true),
        ("", false),
        ("-dash", false),
        (".dot", false),
        ("/lead", false),
        ("trail/", false),
        ("foo.lock", false),
        ("a..b", false),
        ("a//b", false),
        ("has space", false),
        ("bad\0name", false),
        ("bad:name", false),
        ("bad~1", false),
        ("bad^", false),
        ("..", false),
    ];
    for &(b, valid) in branch_cases {
        let res = repo.branch(b, &c1);
        assert_eq!(res.is_ok(), valid, "branch case {b:?}");
    }
}

#[test]
fn corruption_detected_forall_and_open() {
    assert_eq!(Repo::open(MemStore::new()).err(), Some(Error::Corrupt));

    let mut bad_head = MemStore::new();
    bad_head.write("HEAD", b"garbage");
    assert_eq!(Repo::open(bad_head).err(), Some(Error::Corrupt));

    forall(100, 0xbad0_0001, |rng| {
        let mut store = MemStore::new();
        let data = rng.bytes(200);
        let id = {
            let mut repo = Repo::init(store);
            let id = repo.put(Kind::Blob, &data);
            store = repo.store;
            id
        };
        let hex = to_hex(&id);
        let obj_path = format!("objects/{}/{}", &hex[..2], &hex[2..]);
        let mut raw = store.read(&obj_path).unwrap();
        let idx = rng.below(raw.len());
        let mask = (rng.range(1, 256)) as u8;
        raw[idx] ^= mask;
        store.write(&obj_path, &raw);

        let repo = Repo::open(store).unwrap();
        assert_eq!(repo.get(&id), Err(Error::Corrupt));
    });
}
