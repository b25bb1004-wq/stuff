//! Leader check for T44: merge3 against real `git merge-file -p`, unified hunks against
//! `git diff --no-index -U3`, diff minimality with a fresh seed, repository merge scenarios.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use chronos::{diff, merge3, unified, Edit, MemStore, MergeError, Repo, Signature};
use std::path::Path;
use std::process::Command;

fn text(r: &mut Rng, base: Option<&[u8]>) -> Vec<u8> {
    let alpha: &[&str] = &["a\n", "b\n", "c\n", "d\n", "e\n", "fff\n", "\n"];
    let mut lines: Vec<String> = match base {
        None => (0..r.below(10))
            .map(|_| r.pick(alpha).to_string())
            .collect(),
        Some(b) => String::from_utf8(b.to_vec())
            .unwrap()
            .split_inclusive('\n')
            .map(String::from)
            .collect(),
    };
    if base.is_some() {
        for _ in 0..r.below(4) {
            let i = r.below(lines.len() + 1);
            match r.below(3) {
                0 => lines.insert(i, r.pick(alpha).to_string()),
                1 if i < lines.len() => {
                    lines.remove(i);
                }
                _ if i < lines.len() => lines[i] = r.pick(alpha).to_string(),
                _ => {}
            }
        }
    }
    let mut t = lines.concat().into_bytes();
    if r.chance(1, 6) && t.last() == Some(&b'\n') {
        t.pop(); // missing final newline
    }
    t
}

fn git_merge(dir: &Path, base: &[u8], ours: &[u8], theirs: &[u8]) -> (Vec<u8>, usize) {
    for (n, d) in [("ours", ours), ("base", base), ("theirs", theirs)] {
        std::fs::write(dir.join(n), d).unwrap();
    }
    let o = Command::new("git")
        .args([
            "merge-file",
            "-p",
            "-L",
            "ours",
            "-L",
            "base",
            "-L",
            "theirs",
            "ours",
            "base",
            "theirs",
        ])
        .current_dir(dir)
        .output()
        .unwrap();
    let code = o.status.code().unwrap();
    assert!((0..127).contains(&code), "git merge-file failed: {code}");
    (o.stdout, code as usize)
}

#[test]
fn leader_merge3_matches_git_merge_file() {
    let dir = std::env::temp_dir().join(format!("drill-merge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Rng::new(0x44_2026_1115);
    let mut bad = Vec::new();
    let (mut clean, mut conflicted) = (0, 0);
    for case in 0..1500 {
        let base = text(&mut r, None);
        let ours = text(&mut r, Some(&base));
        let theirs = text(&mut r, Some(&base));
        let (want, n) = git_merge(&dir, &base, &ours, &theirs);
        let got = merge3(&base, &ours, &theirs);
        if n == 0 {
            clean += 1
        } else {
            conflicted += 1
        }
        if got.text != want || got.conflicts != n {
            bad.push(format!(
                "case {case}: base {:?} ours {:?} theirs {:?}\n  git ({n}) {:?}\n  ours ({}) {:?}",
                String::from_utf8_lossy(&base),
                String::from_utf8_lossy(&ours),
                String::from_utf8_lossy(&theirs),
                String::from_utf8_lossy(&want),
                got.conflicts,
                String::from_utf8_lossy(&got.text)
            ));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    eprintln!("merge3: {clean} clean, {conflicted} conflicting");
    assert!(clean > 200 && conflicted > 200, "generator too one-sided");
    assert!(
        bad.is_empty(),
        "{} of 1500 differ:\n{}",
        bad.len(),
        bad[..bad.len().min(6)].join("\n")
    );
}

#[test]
fn leader_unified_hunks_match_git_diff() {
    let dir = std::env::temp_dir().join(format!("drill-diff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Rng::new(0x44_d1ff);
    let hunks = |t: &[u8]| -> String {
        let s = String::from_utf8_lossy(t).to_string();
        // Hunks from the first "@@"; git's funcname text after the header is dropped.
        s.find("\n@@")
            .map(|i| {
                s[i + 1..]
                    .lines()
                    .map(|l| match l.strip_prefix("@@") {
                        Some(r) => format!("@@{}@@", &r[..r.find("@@").unwrap()]),
                        None => l.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    for case in 0..300 {
        let mut a = text(&mut r, None);
        a.extend(text(&mut r, None));
        let b = text(&mut r, Some(&a));
        std::fs::write(dir.join("a"), &a).unwrap();
        std::fs::write(dir.join("b"), &b).unwrap();
        let o = Command::new("git")
            .args([
                "-c",
                "core.autocrlf=false",
                "diff",
                "--no-index",
                "--no-color",
                "--no-indent-heuristic",
                "-U3",
                "a",
                "b",
            ])
            .current_dir(&dir)
            .output()
            .unwrap();
        let ours = unified("f", &a, &b, 3);
        assert_eq!(ours.is_empty(), a == b, "case {case}");
        assert_eq!(
            hunks(&ours),
            hunks(&o.stdout),
            "case {case}: a {:?} b {:?}",
            String::from_utf8_lossy(&a),
            String::from_utf8_lossy(&b)
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn lcs(a: &[u8], b: &[u8]) -> usize {
    let mut t = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    t[0][0]
}

#[test]
fn leader_diff_is_minimal_and_replays() {
    let mut r = Rng::new(0x44_1115);
    for _ in 0..3000 {
        let a: Vec<u8> = (0..r.below(40)).map(|_| b"abc"[r.below(3)]).collect();
        let b: Vec<u8> = (0..r.below(40)).map(|_| b"abc"[r.below(3)]).collect();
        let e = diff(&a, &b);
        let changes = e.iter().filter(|x| !matches!(x, Edit::Keep(..))).count();
        assert_eq!(changes, a.len() + b.len() - 2 * lcs(&a, &b), "{a:?} {b:?}");
        let mut out = Vec::new();
        let (mut ia, mut ib) = (0, 0);
        for x in &e {
            match *x {
                Edit::Keep(i, j) => {
                    assert_eq!((i, j), (ia, ib));
                    assert_eq!(a[i], b[j]);
                    out.push(a[i]);
                    ia += 1;
                    ib += 1;
                }
                Edit::Delete(i) => {
                    assert_eq!(i, ia);
                    ia += 1;
                }
                Edit::Insert(j) => {
                    assert_eq!(j, ib);
                    out.push(b[j]);
                    ib += 1;
                }
            }
        }
        assert_eq!((out, ia, ib), (b.clone(), a.len(), b.len()));
    }
}

#[test]
fn leader_repo_merge_scenarios() {
    let who = |t| Signature {
        name: "A".into(),
        email: "a@x".into(),
        time: t,
        tz_minutes: 0,
    };
    let mut r = Repo::init(MemStore::default());
    r.add("f", b"1\n2\n3\n4\n5\n", false).unwrap();
    let c1 = r.commit("base\n", &who(1)).unwrap();
    r.branch("dev", &c1).unwrap();
    r.add("f", b"1\nTWO\n3\n4\n5\n", false).unwrap();
    let c2 = r.commit("main edit\n", &who(2)).unwrap();
    assert!(matches!(
        r.merge("dev", "m\n", &who(3)),
        Err(MergeError::UpToDate)
    ));
    r.checkout("dev").unwrap();
    r.add("f", b"1\n2\n3\n4\nFIVE\n", false).unwrap();
    r.add("g", b"new\n", false).unwrap();
    let c3 = r.commit("dev edit\n", &who(4)).unwrap();
    r.checkout("main").unwrap();
    let m = r.merge("dev", "merge dev\n", &who(5)).unwrap();
    let files = r.files(&m).unwrap();
    assert_eq!(files["f"].0, b"1\nTWO\n3\n4\nFIVE\n");
    assert_eq!(files["g"].0, b"new\n");
    let (_, data) = r.get(&m).unwrap();
    let c = chronos::decode_commit(&data).unwrap();
    assert_eq!(c.parents, vec![c2, c3]);
    // Conflict: both sides change line 3; HEAD and index stay as they were.
    r.add("f", b"1\nTWO\nmain3\n4\nFIVE\n", false).unwrap();
    let c4 = r.commit("main 3\n", &who(6)).unwrap();
    r.checkout("dev").unwrap();
    r.add("f", b"1\n2\ndev3\n4\nFIVE\n", false).unwrap();
    r.commit("dev 3\n", &who(7)).unwrap();
    r.checkout("main").unwrap();
    let staged = r.staged();
    match r.merge("dev", "x\n", &who(8)) {
        Err(MergeError::Conflict(p)) => assert_eq!(p, vec!["f".to_string()]),
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!((r.head(), r.staged()), (Some(c4), staged));
    // Fast-forward: a branch strictly behind moves without a merge commit.
    r.branch("ff", &c1).unwrap();
    r.checkout("ff").unwrap();
    assert_eq!(r.merge("main", "ff\n", &who(9)).unwrap(), c4);
    assert!(r.status().is_empty());
}
