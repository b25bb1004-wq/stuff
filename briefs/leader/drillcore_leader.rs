//! Leader check for T43: blob, tree and commit ids equal real git's (SHA-256 object format) for
//! the same history, including git's tree order and exec bits; history, checkout and corruption.
#![allow(clippy::unwrap_used)]
use drillcore::{from_hex, to_hex, Commit, Error, Kind, MemStore, Repo, Signature, Store};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str], date: &str) -> String {
    let o = Command::new("git")
        .args(["-c", "core.autocrlf=false", "-c", "core.filemode=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Ada Quarry")
        .env("GIT_AUTHOR_EMAIL", "ada@strata.test")
        .env("GIT_COMMITTER_NAME", "Ada Quarry")
        .env("GIT_COMMITTER_EMAIL", "ada@strata.test")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn who(time: i64, tz: i32) -> Signature {
    Signature {
        name: "Ada Quarry".into(),
        email: "ada@strata.test".into(),
        time,
        tz_minutes: tz,
    }
}

#[test]
fn leader_ids_match_git_sha256() {
    let dir = std::env::temp_dir().join(format!("drill-leader-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(
        &dir,
        &["init", "-q", "--object-format=sha256", "-b", "main", "."],
        "",
    );
    let mut repo = Repo::init(MemStore::default());

    // Names chosen to exercise git's "directory sorts as name/" rule.
    let steps: &[(&[(&str, &str, bool)], &[&str], &str, &str, i64, i32)] = &[
        (
            &[
                ("a.b", "dot\n", false),
                ("a-b", "dash\n", false),
                ("a/x", "in dir\n", false),
                ("a0", "zero\n", true),
                ("z/y/deep.txt", "deep\n", false),
                ("empty", "", false),
            ],
            &[],
            "first\n",
            "1700000000 +0530",
            1_700_000_000,
            330,
        ),
        (
            &[
                ("a/x", "changed\n", false),
                ("bin/run", "#!/bin/sh\necho hi\n", true),
                ("é.txt", "unicode name\n", false),
            ],
            &["a-b"],
            "second: change, add, remove\n\nwith a body line\n",
            "1700003600 -0800",
            1_700_003_600,
            -480,
        ),
        (
            &[("z/y/deep.txt", "deeper\n", false)],
            &["a.b", "empty"],
            "third\n",
            "1700007200 +0000",
            1_700_007_200,
            0,
        ),
    ];
    for (i, (add, rm, msg, date, t, tz)) in steps.iter().enumerate() {
        for (p, body, exec) in add.iter() {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, body).unwrap();
            git(&dir, &["add", "--", p], "");
            if *exec {
                git(&dir, &["update-index", "--chmod=+x", "--", p], "");
            }
            repo.add(p, body.as_bytes(), *exec).unwrap();
            let blob = git(&dir, &["hash-object", "--", p], "");
            assert_eq!(
                to_hex(&drillcore::hash_object(Kind::Blob, body.as_bytes())),
                blob,
                "blob {p}"
            );
        }
        for p in rm.iter() {
            git(&dir, &["rm", "-q", "--", p], "");
            repo.rm(p).unwrap();
        }
        git(
            &dir,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                msg.trim_end_matches('\n'),
            ],
            date,
        );
        let ours = repo.commit(msg, &who(*t, *tz)).unwrap();
        let theirs_tree = git(&dir, &["rev-parse", "HEAD^{tree}"], "");
        let (_, cdata) = repo.get(&ours).unwrap();
        let c = drillcore::decode_commit(&cdata).unwrap();
        assert_eq!(to_hex(&c.tree), theirs_tree, "step {i}: tree id");
        assert_eq!(
            to_hex(&ours),
            git(&dir, &["rev-parse", "HEAD"], ""),
            "step {i}: commit id"
        );
    }
    // A merge commit with two parents through commit-tree.
    let head = repo.head().unwrap();
    let first = repo.resolve("HEAD~2").unwrap();
    let tree = git(&dir, &["rev-parse", "HEAD^{tree}"], "");
    let merge = git(
        &dir,
        &[
            "commit-tree",
            &tree,
            "-p",
            &to_hex(&head),
            "-p",
            &to_hex(&first),
            "-m",
            "merge",
        ],
        "1700010000 +0100",
    );
    let c = Commit {
        tree: from_hex(&tree).unwrap(),
        parents: vec![head, first],
        author: who(1_700_010_000, 60),
        committer: who(1_700_010_000, 60),
        message: "merge\n".into(),
    };
    assert_eq!(
        to_hex(&repo.put(Kind::Commit, &drillcore::encode_commit(&c))),
        merge
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn leader_history_checkout_and_corruption() {
    let mut r = Repo::init(MemStore::default());
    assert_eq!(r.current_branch().as_deref(), Some("main"));
    assert!(r.head().is_none());
    assert!(r.add("../x", b"", false).is_err() && r.add("a//b", b"", false).is_err());
    r.add("f", b"1", false).unwrap();
    let c1 = r.commit("c1\n", &who(10, 0)).unwrap();
    r.branch("dev", &c1).unwrap();
    assert_eq!(r.branch("dev", &c1), Err(Error::Exists));
    for bad in ["-x", ".x", "a..b", "a/", "x.lock", "sp ace"] {
        assert_eq!(r.branch(bad, &c1), Err(Error::BadName), "{bad}");
    }
    r.add("f", b"2", false).unwrap();
    let c2 = r.commit("c2\n", &who(20, 0)).unwrap();
    r.add("g", b"x", false).unwrap();
    assert_eq!(r.checkout("dev"), Err(Error::Dirty));
    r.rm("g").unwrap();
    let files = r.checkout("dev").unwrap();
    assert_eq!(files.get("f").unwrap().0, b"1");
    assert_eq!(r.current_branch().as_deref(), Some("dev"));
    r.add("h", b"dev", true).unwrap();
    let c3 = r.commit("c3\n", &who(30, 0)).unwrap();
    assert_eq!(r.log(&c3).unwrap(), vec![c3, c1]);
    assert_eq!(r.resolve("main"), Some(c2));
    assert_eq!(r.resolve("HEAD^"), Some(c1));
    assert_eq!(r.resolve(&to_hex(&c2)[..8]), Some(c2));
    assert_eq!(r.resolve("nope"), None);
    r.checkout(&to_hex(&c2)).unwrap();
    assert_eq!(r.current_branch(), None, "detached");
    assert!(r.files(&c3).unwrap().get("h").unwrap().1);
    assert!(r.delete_branch("main").is_ok());

    // Corrupt every byte of one object in turn: `get` is Corrupt, never a panic.
    let path = format!("objects/{}/{}", &to_hex(&c3)[..2], &to_hex(&c3)[2..]);
    let good = r.store.read(&path).unwrap();
    for i in 0..good.len() {
        let mut b = good.clone();
        b[i] ^= 0x20;
        r.store.write(&path, &b);
        assert!(r.get(&c3).is_err(), "byte {i}");
        let _ = r.log(&c3);
    }
    r.store.write(&path, &good);
    assert!(r.get(&c3).is_ok());
    r.store.remove("HEAD");
    let s = std::mem::take(&mut r.store);
    assert!(Repo::open(s).is_err());
}
