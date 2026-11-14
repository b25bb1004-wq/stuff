//! Integration, table, and property tests for diff, unified, merge3, status, and Repo::merge.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use drillcore::{
    diff, lines, merge3, to_hex, unified, Change, Edit, Error, MemStore, MergeError, Merged, Repo,
    Signature,
};

fn sig(name: &str, time: i64) -> Signature {
    Signature {
        name: name.into(),
        email: format!("{name}@example.com"),
        time,
        tz_minutes: 0,
    }
}

fn lcs_len<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let m = b.len();
    let mut prev = vec![0usize; m + 1];
    let mut curr = vec![0usize; m + 1];
    for ai in a {
        for (j, bj) in b.iter().enumerate() {
            curr[j + 1] = if ai == bj {
                prev[j] + 1
            } else {
                prev[j + 1].max(curr[j])
            };
        }
        core::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }
    prev[m]
}

fn replay<T: Clone + PartialEq>(a: &[T], b: &[T], edits: &[Edit]) -> Vec<T> {
    let mut out = Vec::new();
    let mut ai = 0;
    let mut bi = 0;
    for &e in edits {
        match e {
            Edit::Keep(i, j) => {
                assert_eq!(i, ai);
                assert_eq!(j, bi);
                assert!(a[i] == b[j]);
                out.push(a[i].clone());
                ai += 1;
                bi += 1;
            }
            Edit::Delete(i) => {
                assert_eq!(i, ai);
                ai += 1;
            }
            Edit::Insert(j) => {
                assert_eq!(j, bi);
                out.push(b[j].clone());
                bi += 1;
            }
        }
    }
    assert_eq!(ai, a.len());
    assert_eq!(bi, b.len());
    out
}

#[test]
fn diff_minimality_and_replay_forall_1000() {
    forall(1000, 0x44_49_46_46, |rng: &mut Rng| {
        let len_a = rng.below(35);
        let len_b = rng.below(35);
        let alphabet = rng.range(1, 5) as u8; // small alphabet for many ties
        let a: Vec<u8> = (0..len_a)
            .map(|_| rng.below(alphabet as usize) as u8)
            .collect();
        let b: Vec<u8> = (0..len_b)
            .map(|_| rng.below(alphabet as usize) as u8)
            .collect();

        let edits = diff(&a, &b);
        let replayed = replay(&a, &b, &edits);
        assert_eq!(replayed, b);

        let changes = edits
            .iter()
            .filter(|e| !matches!(e, Edit::Keep(_, _)))
            .count();
        let expected_changes = a.len() + b.len() - 2 * lcs_len(&a, &b);
        assert_eq!(changes, expected_changes);

        // Verify Deletes come before Inserts inside each changed region.
        let mut saw_insert = false;
        for &e in &edits {
            match e {
                Edit::Keep(_, _) => saw_insert = false,
                Edit::Delete(_) => assert!(!saw_insert, "Delete after Insert in changed region"),
                Edit::Insert(_) => saw_insert = true,
            }
        }
    });
}

#[test]
fn unified_table_cases() {
    type Case<'a> = (&'a str, &'a [u8], &'a [u8], usize, &'a str);
    let cases: &[Case<'_>] = &[
        // 1. Equal empty
        ("f", b"", b"", 3, ""),
        // 2. Equal non-empty without trailing newline
        ("f", b"abc", b"abc", 3, ""),
        // 3. Empty to single line with newline
        (
            "f",
            b"",
            b"hello\n",
            3,
            "--- a/f\n+++ b/f\n@@ -0,0 +1 @@\n+hello\n",
        ),
        // 4. Single line to empty
        (
            "f",
            b"hello\n",
            b"",
            3,
            "--- a/f\n+++ b/f\n@@ -1 +0,0 @@\n-hello\n",
        ),
        // 5. Empty to line without newline
        (
            "f",
            b"",
            b"hello",
            3,
            "--- a/f\n+++ b/f\n@@ -0,0 +1 @@\n+hello\n\\ No newline at end of file\n",
        ),
        // 6. Line without newline to empty
        (
            "f",
            b"hello",
            b"",
            3,
            "--- a/f\n+++ b/f\n@@ -1 +0,0 @@\n-hello\n\\ No newline at end of file\n",
        ),
        // 7. Missing newline added
        (
            "f",
            b"a\nb",
            b"a\nb\n",
            3,
            "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+b\n",
        ),
        // 8. Missing newline removed
        (
            "f",
            b"a\nb\n",
            b"a\nb",
            3,
            "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+b\n\\ No newline at end of file\n",
        ),
        // 9. Missing newline in trailing context
        (
            "f",
            b"x\na\nb",
            b"y\na\nb",
            3,
            "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-x\n+y\n a\n b\n\\ No newline at end of file\n",
        ),
        // 10. Context 0 insert at start
        (
            "dir/f.rs",
            b"a\nb\n",
            b"x\na\nb\n",
            0,
            "--- a/dir/f.rs\n+++ b/dir/f.rs\n@@ -0,0 +1 @@\n+x\n",
        ),
        // 11. Context 0 insert in middle
        (
            "f",
            b"a\nb\n",
            b"a\nx\nb\n",
            0,
            "--- a/f\n+++ b/f\n@@ -1,0 +2 @@\n+x\n",
        ),
        // 12. Context 0 insert at end
        (
            "f",
            b"a\nb\n",
            b"a\nb\nx\n",
            0,
            "--- a/f\n+++ b/f\n@@ -2,0 +3 @@\n+x\n",
        ),
        // 13. Context 0 delete at end
        (
            "f",
            b"a\nb\n",
            b"a\n",
            0,
            "--- a/f\n+++ b/f\n@@ -2 +1,0 @@\n-b\n",
        ),
        // 14. Two changes separated by 6 lines merge at context=3
        (
            "f",
            b"1\n2\n3\n4\n5\n6\n7\n8\n",
            b"A\n2\n3\n4\n5\n6\n7\nB\n",
            3,
            "--- a/f\n+++ b/f\n@@ -1,8 +1,8 @@\n-1\n+A\n 2\n 3\n 4\n 5\n 6\n 7\n-8\n+B\n",
        ),
        // 15. Two changes separated by 7 lines stay separate at context=3
        (
            "f",
            b"1\n2\n3\n4\n5\n6\n7\n8\n9\n",
            b"A\n2\n3\n4\n5\n6\n7\n8\nB\n",
            3,
            "--- a/f\n+++ b/f\n@@ -1,4 +1,4 @@\n-1\n+A\n 2\n 3\n 4\n@@ -6,4 +6,4 @@\n 6\n 7\n 8\n-9\n+B\n",
        ),
        // 16. Context 0 separates changes with 1 common line between them
        (
            "f",
            b"1\n2\n3\n",
            b"A\n2\nB\n",
            0,
            "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-1\n+A\n@@ -3 +3 @@\n-3\n+B\n",
        ),
    ];

    for (idx, (path, a, b, ctx, expected)) in cases.iter().enumerate() {
        let got = unified(path, a, b, *ctx);
        assert_eq!(
            String::from_utf8_lossy(&got),
            *expected,
            "unified case {} failed",
            idx + 1
        );
    }
}

#[test]
fn merge3_table_cases() {
    type Case<'a> = (&'a [u8], &'a [u8], &'a [u8], &'a [u8], usize);
    let cases: &[Case<'_>] = &[
        // 1. All equal
        (b"a\nb\n", b"a\nb\n", b"a\nb\n", b"a\nb\n", 0),
        // 2. Empty all
        (b"", b"", b"", b"", 0),
        // 3. Ours only edit
        (b"a\nb\nc\n", b"a\nX\nc\n", b"a\nb\nc\n", b"a\nX\nc\n", 0),
        // 4. Theirs only edit
        (b"a\nb\nc\n", b"a\nb\nc\n", b"a\nY\nc\n", b"a\nY\nc\n", 0),
        // 5. Identical edits
        (b"a\nb\nc\n", b"a\nX\nc\n", b"a\nX\nc\n", b"a\nX\nc\n", 0),
        // 6. Disjoint edits (separated by 1 line)
        (b"a\nb\nc\n", b"A\nb\nc\n", b"a\nb\nC\n", b"A\nb\nC\n", 0),
        // 7. Insert at start (ours) and end (theirs)
        (b"a\n", b"start\na\n", b"a\nend\n", b"start\na\nend\n", 0),
        // 8. Both insert differently at start -> conflict
        (
            b"a\n",
            b"X\na\n",
            b"Y\na\n",
            b"<<<<<<< ours\nX\n=======\nY\n>>>>>>> theirs\na\n",
            1,
        ),
        // 9. Both insert identically at start -> clean
        (b"a\n", b"X\na\n", b"X\na\n", b"X\na\n", 0),
        // 10. Both insert differently at end -> conflict
        (
            b"a\n",
            b"a\nX\n",
            b"a\nY\n",
            b"a\n<<<<<<< ours\nX\n=======\nY\n>>>>>>> theirs\n",
            1,
        ),
        // 11. Adjacent edits (line 1 vs line 2) -> conflict
        (
            b"1\n2\n3\n",
            b"A\n2\n3\n",
            b"1\nB\n3\n",
            b"<<<<<<< ours\nA\n2\n=======\n1\nB\n>>>>>>> theirs\n3\n",
            1,
        ),
        // 12. Overlapping edit with common prefix trimmed (zealous)
        (
            b"1\n2\n3\n",
            b"A\nB\n3\n",
            b"A\nC\n3\n",
            b"A\n<<<<<<< ours\nB\n=======\nC\n>>>>>>> theirs\n3\n",
            1,
        ),
        // 13. Overlapping edit with common suffix trimmed (zealous)
        (
            b"1\n2\n3\n",
            b"1\nB\nZ\n",
            b"1\nC\nZ\n",
            b"1\n<<<<<<< ours\nB\n=======\nC\n>>>>>>> theirs\nZ\n",
            1,
        ),
        // 14. Overlapping edit with both prefix & suffix trimmed
        (
            b"1\n2\n3\n4\n",
            b"1\nP\nX\nS\n4\n",
            b"1\nP\nY\nS\n4\n",
            b"1\nP\n<<<<<<< ours\nX\n=======\nY\n>>>>>>> theirs\nS\n4\n",
            1,
        ),
        // 15. Common alphanumeric line <= 3 lines merges back into 1 conflict (ZEALOUS_ALNUM)
        (
            b"1\n2\n3\n",
            b"A\nmid\nB\n",
            b"X\nmid\nY\n",
            b"<<<<<<< ours\nA\nmid\nB\n=======\nX\nmid\nY\n>>>>>>> theirs\n",
            1,
        ),
        // 15b. Common alphanumeric block >= 4 lines splits conflict
        (
            b"1\n2\n3\n",
            b"A\nm1\nm2\nm3\nm4\nB\n",
            b"X\nm1\nm2\nm3\nm4\nY\n",
            b"<<<<<<< ours\nA\n=======\nX\n>>>>>>> theirs\nm1\nm2\nm3\nm4\n<<<<<<< ours\nB\n=======\nY\n>>>>>>> theirs\n",
            2,
        ),
        // 16. Common blank line in middle does NOT split conflict
        (
            b"1\n2\n3\n",
            b"A\n\nB\n",
            b"X\n\nY\n",
            b"<<<<<<< ours\nA\n\nB\n=======\nX\n\nY\n>>>>>>> theirs\n",
            1,
        ),
        // 17. One side deletes, other side untouched -> deleted
        (b"1\n2\n3\n", b"1\n3\n", b"1\n2\n3\n", b"1\n3\n", 0),
        // 18. Both sides delete same range -> deleted cleanly
        (b"1\n2\n3\n4\n", b"1\n4\n", b"1\n4\n", b"1\n4\n", 0),
        // 19. Overlapping deletions -> conflict
        (
            b"1\n2\n3\n4\n5\n",
            b"1\n4\n5\n",
            b"1\n2\n5\n",
            b"1\n<<<<<<< ours\n4\n=======\n2\n>>>>>>> theirs\n5\n",
            1,
        ),
        // 20. Delete on ours vs modify on theirs -> conflict
        (
            b"1\n2\n3\n",
            b"1\n3\n",
            b"1\nX\n3\n",
            b"1\n<<<<<<< ours\n=======\nX\n>>>>>>> theirs\n3\n",
            1,
        ),
        // 21. Modify on ours vs delete on theirs -> conflict
        (
            b"1\n2\n3\n",
            b"1\nX\n3\n",
            b"1\n3\n",
            b"1\n<<<<<<< ours\nX\n=======\n>>>>>>> theirs\n3\n",
            1,
        ),
        // 22. Empty base, disjoint? Both added differently -> conflict
        (
            b"",
            b"ours",
            b"theirs",
            b"<<<<<<< ours\nours\n=======\ntheirs\n>>>>>>> theirs\n",
            1,
        ),
        // 23. Empty base, both added identically
        (b"", b"same\n", b"same\n", b"same\n", 0),
        // 24. Missing final newline preserved in clean trailing line
        (b"a\nb\nc", b"X\nb\nc", b"a\nb\nc", b"X\nb\nc", 0),
        // 25. Missing final newline on both sides in conflict gets newline before markers
        (
            b"a\nb",
            b"a\nx",
            b"a\ny",
            b"a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n",
            1,
        ),
        // 26. Ours inserts at start, theirs edits without final newline at end
        (b"a\nb\nc\n", b"0\na\nb\nc\n", b"a\nb\nc", b"0\na\nb\nc", 0),
        // 27. Duplicate line disambiguation (both insert at end after "a\n")
        (
            b"a\n",
            b"a\na\n",
            b"a\nb\n",
            b"a\n<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n",
            1,
        ),
    ];

    for (idx, (base, ours, theirs, expected_text, expected_conflicts)) in cases.iter().enumerate() {
        let Merged { text, conflicts } = merge3(base, ours, theirs);
        assert_eq!(
            conflicts,
            *expected_conflicts,
            "merge3 case {} conflict count",
            idx + 1
        );
        assert_eq!(
            String::from_utf8_lossy(&text),
            String::from_utf8_lossy(expected_text),
            "merge3 case {} text",
            idx + 1
        );
        assert_eq!(text, *expected_text, "merge3 case {} raw bytes", idx + 1);
    }

    // Sanity check lines() usage
    assert_eq!(lines(b"x\ny"), vec![b"x\n".as_slice(), b"y".as_slice()]);
}

#[test]
fn repo_status_and_merge_scenarios() {
    // 1. Fast-forward and UpToDate
    let mut repo = Repo::init(MemStore::new());
    repo.add("f.txt", b"1\n2\n3\n", false).unwrap();
    let c1 = repo.commit("c1", &sig("a", 10)).unwrap();

    repo.branch("feat", &c1).unwrap();
    repo.checkout("feat").unwrap();
    repo.add("f.txt", b"1\n2\n3\n4\n", false).unwrap();
    let c2 = repo.commit("c2", &sig("a", 20)).unwrap();

    repo.checkout("main").unwrap();
    assert_eq!(
        repo.merge("main", "noop", &sig("a", 25)),
        Err(MergeError::UpToDate)
    );
    let ff = repo.merge("feat", "ff", &sig("a", 30)).unwrap();
    assert_eq!(ff, c2);
    assert_eq!(repo.head(), Some(c2));
    assert!(repo.status().is_empty());
    assert_eq!(
        repo.merge("feat", "again", &sig("a", 35)),
        Err(MergeError::UpToDate)
    );

    // 2. True 3-way merge (same file non-overlapping + separate files)
    repo.branch("side", &c1).unwrap();
    // Advance main (c2 already has line 4)
    repo.add("main_only.txt", b"m\n", false).unwrap();
    let c3 = repo.commit("c3", &sig("a", 40)).unwrap();

    repo.checkout("side").unwrap();
    repo.add("f.txt", b"0\n1\n2\n3\n", false).unwrap();
    repo.add("side_only.txt", b"s\n", true).unwrap();
    let c4 = repo.commit("c4", &sig("b", 50)).unwrap();

    repo.checkout("main").unwrap();
    let m1 = repo.merge("side", "merge side", &sig("a", 60)).unwrap();
    assert_ne!(m1, c3);
    assert_ne!(m1, c4);
    assert!(repo.status().is_empty());
    let files = repo.files(&m1).unwrap();
    assert_eq!(files.get("f.txt").unwrap().0, b"0\n1\n2\n3\n4\n");
    assert_eq!(files.get("main_only.txt").unwrap().0, b"m\n");
    assert_eq!(
        files.get("side_only.txt").unwrap(),
        &(b"s\n".to_vec(), true)
    );
    let log = repo.log(&m1).unwrap();
    assert!(log.contains(&c3) && log.contains(&c4));
}

#[test]
fn repo_merge_conflicts_leave_state_unchanged_and_pick_newest_base() {
    let mut repo = Repo::init(MemStore::new());
    repo.add("shared.txt", b"a\nb\nc\n", false).unwrap();
    repo.add("del_mod.txt", b"keep\n", false).unwrap();
    let base = repo.commit("base", &sig("a", 10)).unwrap();

    // Branch B1 & B2 creating criss-cross ancestors with different committer times
    repo.branch("b1", &base).unwrap();
    repo.branch("b2", &base).unwrap();

    repo.checkout("b1").unwrap();
    repo.add("x.txt", b"from_b1\n", false).unwrap();
    let _b1 = repo.commit("b1", &sig("a", 20)).unwrap();

    repo.checkout("b2").unwrap();
    repo.add("shared.txt", b"a\nB2\nc\n", false).unwrap();
    let b2 = repo.commit("b2", &sig("a", 30)).unwrap(); // newer common ancestor!

    // Merge b2 into b1 -> creates ca_newer (time 40)
    repo.checkout("b1").unwrap();
    let ca_newer = repo.merge("b2", "merge b2", &sig("a", 40)).unwrap();

    // Now make main point at ca_newer, and create another branch at b2 that modifies x differently
    repo.branch("conflict_br", &b2).unwrap();
    repo.checkout("conflict_br").unwrap();
    repo.add("x.txt", b"from_conflict_br\n", false).unwrap(); // added on both differently!
    repo.add("del_mod.txt", b"modified\n", false).unwrap();
    repo.add("shared.txt", b"a\nCONF\nc\n", false).unwrap();
    let _c_conf = repo.commit("conf", &sig("b", 50)).unwrap();

    repo.checkout("b1").unwrap();
    repo.rm("del_mod.txt").unwrap();
    repo.add("shared.txt", b"a\nB1\nc\n", false).unwrap();
    let pre_head = repo.commit("del and mod on b1", &sig("a", 60)).unwrap();
    let pre_staged = repo.staged();

    // Conflict should list all three conflicting paths sorted, and leave HEAD & index unchanged
    let err = repo
        .merge("conflict_br", "should fail", &sig("a", 70))
        .unwrap_err();
    assert_eq!(
        err,
        MergeError::Conflict(vec![
            "del_mod.txt".into(),
            "shared.txt".into(),
            "x.txt".into(),
        ])
    );
    assert_eq!(repo.head(), Some(pre_head));
    assert_eq!(repo.staged(), pre_staged);
    assert!(repo.status().is_empty());

    // Dirty index blocks merge
    repo.add("dirty.txt", b"d\n", false).unwrap();
    assert_eq!(
        repo.merge("conflict_br", "dirty", &sig("a", 75)),
        Err(MergeError::Repo(Error::Dirty))
    );
    repo.rm("dirty.txt").unwrap();

    // Unknown ref returns NotFound
    assert_eq!(
        repo.merge("no-such-branch", "missing", &sig("a", 80)),
        Err(MergeError::Repo(Error::NotFound))
    );

    // Verify criss-cross newest common ancestor (time 30 vs 10):
    // b2 (time 30) has shared.txt = "a\nB2\nc\n", base (time 10) has "a\nb\nc\n".
    // Make another branch from b2 and merge into ca_newer-based b1 cleanly on shared.txt:
    repo.branch("clean_b2", &b2).unwrap();
    repo.checkout("clean_b2").unwrap();
    repo.add("clean_only.txt", b"ok\n", false).unwrap();
    let _ = repo.commit("clean b2", &sig("c", 90)).unwrap();

    // Detached HEAD merge test using full hex
    let b1_hex = to_hex(&ca_newer);
    repo.checkout(&b1_hex).unwrap();
    assert!(repo.current_branch().is_none());
    let detached_m = repo
        .merge("clean_b2", "detached merge", &sig("c", 100))
        .unwrap();
    assert_eq!(repo.head(), Some(detached_m));
    assert_eq!(
        repo.files(&detached_m)
            .unwrap()
            .get("shared.txt")
            .unwrap()
            .0,
        b"a\nB2\nc\n"
    );

    // Status tests: Added, Modified (exec bit + content), Deleted
    let mut r2 = Repo::init(MemStore::new());
    r2.add("keep.txt", b"v1\n", false).unwrap();
    r2.add("drop.txt", b"bye\n", false).unwrap();
    let root = r2.commit("init", &sig("a", 1)).unwrap();
    r2.add("keep.txt", b"v1\n", true).unwrap(); // exec bit changed
    r2.rm("drop.txt").unwrap();
    r2.add("new.txt", b"hi\n", false).unwrap();
    assert_eq!(
        r2.status(),
        vec![
            Change::Deleted("drop.txt".into()),
            Change::Modified("keep.txt".into()),
            Change::Added("new.txt".into()),
        ]
    );

    // File vs directory conflict ("foo" vs "foo/bar")
    let mut r3 = Repo::init(MemStore::new());
    r3.add("base.txt", b"0\n", false).unwrap();
    let c0 = r3.commit("c0", &sig("a", 1)).unwrap();
    r3.branch("other", &c0).unwrap();
    r3.add("foo", b"file\n", false).unwrap();
    r3.commit("file foo", &sig("a", 2)).unwrap();
    r3.checkout("other").unwrap();
    r3.add("foo/bar", b"nested\n", false).unwrap();
    r3.commit("dir foo", &sig("b", 3)).unwrap();
    assert_eq!(
        r3.merge("main", "path conflict", &sig("b", 4)),
        Err(MergeError::Conflict(vec!["foo".into(), "foo/bar".into()]))
    );
    let _ = root;

    // Criss-cross newest common ancestor by committer time:
    // base(t=10): nca="0\n"
    // c1(t=20, parent=base): nca="1\n"
    // c2(t=30, parent=base): nca="2\n"
    // m_a(t=40, parents=[c1, c2]): nca="2\n" (Wait: c1 vs c2 conflicts, so let's change disjoint lines or use c2 after c1!)
    // Actually: c1(t=20): f="a\n", c2(t=30, parent=c1? No, criss-cross!):
    // In criss-cross: c1(t=20) sets nca="X\n", c2(t=30) keeps nca="0\n" and sets other="Y\n".
    // Branch A merges c2 (has nca="X\n", other="Y\n") at t=40, then sets nca="A\n" at t=60.
    // Branch B (from c2) merges c1 (has nca="X\n", other="Y\n") at t=50, then keeps nca="X\n" at t=70!
    // Common ancestors of A and B are {base(10), c1(20), c2(30)}:
    // - If newest c2(30) is chosen (base nca="0\n"), A changed 0->A and B changed 0->X -> CONFLICT!
    // - Wait: if c2(30) changed nca="C2\n" cleanly (e.g. c1 didn't touch nca) and A kept nca="C2\n" while B changed nca="C2\n"->"B\n":
    //   Wait: if c1(20) didn't touch nca (nca="0\n"), c2(30) changed nca="C2\n", then both m1 and m2 have nca="C2\n".
    //   Then A(from m1) reverts nca to "0\n" and B(from m2) keeps "C2\n":
    //   Against c2(30, nca="C2\n"): A changed C2->0, B kept C2 -> clean result "0\n"!
    //   Against c1(20, nca="0\n"): A kept 0, B changed 0->C2 -> result would have been "C2\n"!
    let mut r4 = Repo::init(MemStore::new());
    r4.add("nca.txt", b"0\n", false).unwrap();
    let r4_base = r4.commit("base", &sig("a", 10)).unwrap();
    r4.branch("br2", &r4_base).unwrap();
    r4.add("only1.txt", b"1\n", false).unwrap();
    let _r4_c1 = r4.commit("c1", &sig("a", 20)).unwrap(); // nca = "0\n"

    r4.checkout("br2").unwrap();
    r4.add("nca.txt", b"C2\n", false).unwrap();
    let r4_c2 = r4.commit("c2", &sig("a", 30)).unwrap(); // newer common ancestor, nca = "C2\n"

    // m1 on main merges br2 (parents [c1, c2]) -> nca = "C2\n"
    r4.checkout("main").unwrap();
    r4.merge("br2", "m1", &sig("a", 40)).unwrap();

    // br_b starts at c2, merges c1 (wait: create a branch at c2 and merge main's parent c1)
    r4.branch("br_b", &r4_c2).unwrap();
    // Advance main (A) to revert nca.txt back to "0\n"
    r4.add("nca.txt", b"0\n", false).unwrap();
    r4.commit("A reverts to 0", &sig("a", 60)).unwrap();

    // On br_b: merge c1 (via r4_base? No, via hex of _r4_c1!) so br_b has both c1 and c2 as ancestors!
    r4.checkout("br_b").unwrap();
    r4.merge(&to_hex(&_r4_c1), "m2", &sig("a", 50)).unwrap();
    // br_b has nca.txt = "C2\n". Now merge main into br_b:
    // Common ancestors of main and br_b are {r4_base(10), _r4_c1(20), r4_c2(30)}.
    // Newest by committer time is r4_c2(30) where nca.txt = "C2\n".
    // Against r4_c2, br_b kept "C2\n" and main changed it to "0\n", so merged nca.txt MUST be "0\n"!
    let final_m = r4.merge("main", "criss-cross", &sig("a", 70)).unwrap();
    assert_eq!(
        r4.files(&final_m).unwrap().get("nca.txt").unwrap().0,
        b"0\n"
    );
}

#[test]
fn git_reference_fixtures_match_exactly() {
    let decode = |field: &str| -> Vec<u8> {
        if field == "-" {
            Vec::new()
        } else {
            assay::hex(field)
        }
    };
    let hunks_from_at = |raw: &[u8]| -> Vec<u8> {
        let pos = raw.windows(3).position(|w| w == b"\n@@");
        match pos {
            Some(i) => raw[i + 1..].to_vec(),
            None => Vec::new(),
        }
    };

    let data = include_str!("fixtures/git_reference.txt");
    let mut m_count = 0usize;
    let mut d_count = 0usize;
    let mut m_mismatches = Vec::new();
    let mut d_mismatches = Vec::new();

    for (line_no, line) in data.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(' ').collect();
        match parts.as_slice() {
            ["M", base_h, ours_h, theirs_h, conflicts_s, merged_h] => {
                m_count += 1;
                let base = decode(base_h);
                let ours = decode(ours_h);
                let theirs = decode(theirs_h);
                let want_conflicts: usize = conflicts_s.parse().unwrap();
                let want_merged = decode(merged_h);
                let got = merge3(&base, &ours, &theirs);
                if got.conflicts != want_conflicts || got.text != want_merged {
                    m_mismatches.push(format!(
                        "line {}: base={:?} ours={:?} theirs={:?}\n  want ({}): {:?}\n  got  ({}): {:?}",
                        line_no + 1,
                        String::from_utf8_lossy(&base),
                        String::from_utf8_lossy(&ours),
                        String::from_utf8_lossy(&theirs),
                        want_conflicts,
                        String::from_utf8_lossy(&want_merged),
                        got.conflicts,
                        String::from_utf8_lossy(&got.text),
                    ));
                }
            }
            ["D", a_h, b_h, hunks_h] => {
                d_count += 1;
                let a = decode(a_h);
                let b = decode(b_h);
                let want_hunks = decode(hunks_h);
                let got_full = unified("f", &a, &b, 3);
                let got_hunks = hunks_from_at(&got_full);
                if got_hunks != want_hunks {
                    d_mismatches.push(format!(
                        "line {}: a={:?} b={:?}\n  want: {:?}\n  got:  {:?}",
                        line_no + 1,
                        String::from_utf8_lossy(&a),
                        String::from_utf8_lossy(&b),
                        String::from_utf8_lossy(&want_hunks),
                        String::from_utf8_lossy(&got_hunks),
                    ));
                }
            }
            _ => panic!("unexpected fixture line {}: {line}", line_no + 1),
        }
    }

    assert_eq!(m_count, 800);
    assert_eq!(d_count, 300);
    assert!(
        d_mismatches.is_empty(),
        "{} D mismatches:\n{}",
        d_mismatches.len(),
        d_mismatches[..d_mismatches.len().min(6)].join("\n")
    );
    assert!(
        m_mismatches.is_empty(),
        "{} M mismatches:\n{}",
        m_mismatches.len(),
        m_mismatches[..m_mismatches.len().min(6)].join("\n")
    );
}
