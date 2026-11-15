//! Leader check for T40 (B+tree). Copied to crates/iridium/tests/tree_leader.rs.
//! The page parser below is the leader's own reading of docs/IRIDIUM.md section 2 and briefs/T40.md.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use iridium::{Error, MemStorage, Pager, Tree, USABLE};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

type P = Pager<MemStorage>;
type Model = BTreeMap<Vec<u8>, Vec<u8>>;
const CAP: usize = USABLE - 16;
const OVF: usize = USABLE - 24;

fn new_pager() -> P {
    Pager::open(MemStorage::new(), MemStorage::new()).unwrap()
}
fn reopen(p: P) -> P {
    let (a, b) = p.close().unwrap();
    Pager::open(a, b).unwrap()
}
fn u16_at(b: &[u8], o: usize) -> usize {
    usize::from(u16::from_le_bytes([b[o], b[o + 1]]))
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn varint(b: &[u8], o: &mut usize) -> usize {
    let mut v = 0u64;
    for i in 0..10 {
        let byte = b[*o];
        *o += 1;
        v |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            assert!(i == 0 || byte != 0, "non-minimal varint");
            return usize::try_from(v).unwrap();
        }
    }
    panic!("varint longer than 10 bytes")
}

#[derive(Default)]
struct Shape {
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    pages: Vec<u64>,
    /// (page, used bytes, link)
    leaves: Vec<(u64, usize, u64)>,
    interiors: usize,
    leaf_depth: Option<usize>,
}

fn walk(p: &mut P, root: u64) -> Shape {
    let mut s = Shape::default();
    node(p, root, (None, None), 1, true, &mut s);
    for w in s.leaves.windows(2) {
        assert_eq!(
            w[0].2, w[1].0,
            "leaf {} links to {}, next leaf is {}",
            w[0].0, w[0].2, w[1].0
        );
    }
    assert_eq!(s.leaves.last().unwrap().2, 0, "last leaf link");
    for w in s.entries.windows(2) {
        assert!(w[0].0 < w[1].0, "keys out of order");
    }
    let set: BTreeSet<u64> = s.pages.iter().copied().collect();
    assert_eq!(set.len(), s.pages.len(), "a page is reachable twice");
    s
}

fn node(
    p: &mut P,
    n: u64,
    (lo, hi): (Option<Vec<u8>>, Option<Vec<u8>>),
    depth: usize,
    root: bool,
    s: &mut Shape,
) {
    assert!(depth <= 32, "tree too deep");
    let pg = p.read(n).unwrap();
    s.pages.push(n);
    let (kind, count, start, link) = (pg[0], u16_at(&pg, 2), u16_at(&pg, 4), u64_at(&pg, 8));
    assert_eq!(pg[1], 0, "page {n} flags");
    assert!(
        16 + 2 * count <= start && start <= USABLE,
        "page {n}: count {count}, start {start}"
    );
    let mut spans = Vec::new();
    let mut used = 2 * count;
    let in_range =
        |k: &[u8]| lo.as_deref().is_none_or(|l| l <= k) && hi.as_deref().is_none_or(|h| k < h);
    match kind {
        3 => {
            assert!(root || count >= 1, "empty non-root leaf {n}");
            for i in 0..count {
                let off = u16_at(&pg, 16 + 2 * i);
                assert!(off >= start && off < USABLE, "page {n} offset {off}");
                let mut o = off;
                let kl = varint(&pg, &mut o);
                let vl = varint(&pg, &mut o);
                assert!(kl <= 1024 && vl <= 1 << 20, "lengths");
                let key = pg[o..o + kl].to_vec();
                o += kl;
                let value = if kl + vl <= 1200 {
                    o += vl;
                    pg[o - vl..o].to_vec()
                } else {
                    let inl = 1200 - kl;
                    let mut v = pg[o..o + inl].to_vec();
                    o += inl;
                    let mut ov = u64_at(&pg, o);
                    o += 8;
                    while v.len() < vl {
                        assert_ne!(ov, 0, "overflow chain too short");
                        let op = p.read(ov).unwrap();
                        assert_eq!(op[0], 4, "overflow page kind");
                        s.pages.push(ov);
                        let take = (vl - v.len()).min(OVF);
                        v.extend_from_slice(&op[24..24 + take]);
                        ov = u64_at(&op, 16);
                    }
                    assert_eq!(ov, 0, "overflow chain longer than the value");
                    v
                };
                assert!(o <= USABLE);
                assert!(in_range(&key), "leaf {n}: key outside its separator bounds");
                spans.push((off, o));
                used += o - off;
                s.entries.push((key, value));
            }
            match s.leaf_depth {
                None => s.leaf_depth = Some(depth),
                Some(d) => assert_eq!(d, depth, "leaves at different depths"),
            }
            s.leaves.push((n, used, link));
        }
        2 => {
            assert!(count >= 1, "interior page {n} without cells");
            s.interiors += 1;
            let mut cells = Vec::new();
            for i in 0..count {
                let off = u16_at(&pg, 16 + 2 * i);
                assert!(off >= start && off < USABLE, "page {n} offset {off}");
                let child = u64_at(&pg, off);
                let mut o = off + 8;
                let kl = varint(&pg, &mut o);
                assert!(kl <= 1024);
                let key = pg[o..o + kl].to_vec();
                o += kl;
                assert!(o <= USABLE);
                spans.push((off, o));
                cells.push((child, key));
            }
            for w in cells.windows(2) {
                assert!(w[0].1 < w[1].1, "page {n}: separators out of order");
            }
            let mut prev = lo.clone();
            for (child, key) in cells {
                node(p, child, (prev, Some(key.clone())), depth + 1, false, s);
                prev = Some(key);
            }
            node(p, link, (prev, hi.clone()), depth + 1, false, s);
        }
        k => panic!("page {n}: kind {k} in a tree"),
    }
    spans.sort_unstable();
    for w in spans.windows(2) {
        assert!(w[0].1 <= w[1].0, "page {n}: cells overlap");
    }
}

fn free_list(p: &mut P) -> Vec<u64> {
    let mut out = Vec::new();
    let mut f = u64_at(&p.read(0).unwrap(), 20);
    while f != 0 {
        assert!((out.len() as u64) < p.page_count(), "free list cycle");
        let pg = p.read(f).unwrap();
        assert_eq!(pg[0], 5, "free page kind");
        out.push(f);
        f = u64_at(&pg, 16);
    }
    out
}

/// page 0 + tree pages + free list = page_count, with no page twice.
fn account(p: &mut P, roots: &[u64]) {
    let mut all = BTreeSet::from([0u64]);
    for &r in roots {
        for pg in walk(p, r).pages {
            assert!(all.insert(pg), "page {pg} in two places");
        }
    }
    for pg in free_list(p) {
        assert!(all.insert(pg), "free page {pg} also in use");
    }
    assert_eq!(all.len() as u64, p.page_count(), "leaked pages");
}

fn in_bounds(k: &[u8], lo: &Bound<Vec<u8>>, hi: &Bound<Vec<u8>>) -> bool {
    let a = match lo {
        Bound::Included(l) => k >= &l[..],
        Bound::Excluded(l) => k > &l[..],
        Bound::Unbounded => true,
    };
    let b = match hi {
        Bound::Included(h) => k <= &h[..],
        Bound::Excluded(h) => k < &h[..],
        Bound::Unbounded => true,
    };
    a && b
}
fn as_ref(b: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match b {
        Bound::Included(v) => Bound::Included(&v[..]),
        Bound::Excluded(v) => Bound::Excluded(&v[..]),
        Bound::Unbounded => Bound::Unbounded,
    }
}

fn key(r: &mut Rng, live: &Model) -> Vec<u8> {
    if !live.is_empty() && r.below(3) == 0 {
        return live.keys().nth(r.below(live.len())).unwrap().clone();
    }
    match r.below(10) {
        0 => {
            let mut k = vec![0u8; if r.below(2) == 0 { 1024 } else { r.below(1025) }];
            r.fill(&mut k);
            k
        }
        1..=6 => (0..r.below(13)).map(|_| b"ab\0\xff"[r.below(4)]).collect(),
        _ => r.u64().to_be_bytes().to_vec(),
    }
}
fn value(r: &mut Rng, kl: usize) -> Vec<u8> {
    let n = match r.below(20) {
        0..=2 => (1200 - kl as i64 + r.below(3) as i64 - 1).max(0) as usize,
        3 => 4000 + r.below(16000),
        4 if r.below(20) == 0 => 200_000 + r.below(100_000),
        _ => r.below(120),
    };
    let mut v = vec![0u8; n];
    r.fill(&mut v);
    v
}
fn bound(r: &mut Rng, m: &Model) -> Bound<Vec<u8>> {
    let k = key(r, m);
    match r.below(3) {
        0 => Bound::Included(k),
        1 => Bound::Excluded(k),
        _ => Bound::Unbounded,
    }
}

fn verify(p: &mut P, t: Tree, m: &Model, r: &mut Rng) {
    let s = walk(p, t.root());
    let want: Vec<_> = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    assert!(
        s.entries == want,
        "tree holds {} entries, model {}",
        s.entries.len(),
        want.len()
    );
    assert_eq!(
        t.range(p, None, Bound::Unbounded, Bound::Unbounded)
            .unwrap(),
        want
    );
    for _ in 0..6 {
        let (lo, hi) = (bound(r, m), bound(r, m));
        let got = t.range(p, None, as_ref(&lo), as_ref(&hi)).unwrap();
        let exp: Vec<_> = want
            .iter()
            .filter(|(k, _)| in_bounds(k, &lo, &hi))
            .cloned()
            .collect();
        assert_eq!(got, exp, "range {lo:?}..{hi:?}");
    }
    for _ in 0..3 {
        let from = bound(r, m);
        let stop = r.below(8);
        let mut seen = Vec::new();
        t.scan(p, None, as_ref(&from), &mut |k, v| {
            seen.push((k.to_vec(), v.to_vec()));
            seen.len() < stop
        })
        .unwrap();
        let exp: Vec<_> = want
            .iter()
            .filter(|(k, _)| in_bounds(k, &from, &Bound::Unbounded))
            .take(stop.max(1))
            .cloned()
            .collect();
        assert_eq!(seen, exp, "scan from {from:?} stopping after {stop}");
    }
    for _ in 0..20 {
        let k = key(r, m);
        assert_eq!(t.get(p, None, &k).unwrap(), m.get(&k).cloned(), "get");
    }
}

fn random_ops(p: &mut P, t: Tree, m: &mut Model, r: &mut Rng, n: usize) {
    for _ in 0..n {
        let k = key(r, m);
        match r.below(20) {
            0..=11 => {
                let v = value(r, k.len());
                assert_eq!(
                    t.insert(p, &k, &v).unwrap(),
                    m.insert(k, v),
                    "insert old value"
                );
            }
            12..=16 => assert_eq!(t.remove(p, &k).unwrap(), m.remove(&k), "remove"),
            _ => assert_eq!(
                t.get(p, None, &k).unwrap(),
                m.get(&k).cloned(),
                "get in txn"
            ),
        }
    }
}

#[test]
fn leader_model_commit_rollback_reopen() {
    for (seed, txns, ops) in [(0x40_7ee0_0001u64, 60, 60), (0x40_7ee0_0002, 25, 300)] {
        let mut r = Rng::new(seed);
        let mut p = new_pager();
        p.begin().unwrap();
        let ts = [Tree::create(&mut p).unwrap(), Tree::create(&mut p).unwrap()];
        p.commit().unwrap();
        let roots = [ts[0].root(), ts[1].root()];
        let mut ms = [Model::new(), Model::new()];
        for i in 0..txns {
            p.begin().unwrap();
            let mut next = ms.clone();
            for _ in 0..4 {
                let j = r.below(2);
                random_ops(&mut p, ts[j], &mut next[j], &mut r, ops / 4);
            }
            if i % 7 == 3 {
                verify(&mut p, ts[0], &next[0], &mut r);
                account(&mut p, &roots);
            }
            if r.below(5) == 0 {
                p.rollback();
            } else {
                p.commit().unwrap();
                ms = next;
            }
            if i % 10 == 9 {
                p = reopen(p);
            }
            for j in 0..2 {
                let t = Tree::open(roots[j]);
                assert_eq!(t, ts[j]);
                verify(&mut p, t, &ms[j], &mut r);
            }
            account(&mut p, &roots);
        }
    }
}

#[test]
fn leader_layout_byte_exact() {
    let mut p = new_pager();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let pg = p.read(t.root()).unwrap();
    assert_eq!(
        (pg[0], pg[1], u16_at(&pg, 2), u16_at(&pg, 4), u64_at(&pg, 8)),
        (3, 0, 0, USABLE, 0),
        "empty leaf"
    );
    t.insert(&mut p, b"key", b"value!").unwrap();
    let pg = p.read(t.root()).unwrap();
    let cell = [3u8, 6, b'k', b'e', b'y', b'v', b'a', b'l', b'u', b'e', b'!'];
    assert_eq!(
        (u16_at(&pg, 2), u16_at(&pg, 4), u16_at(&pg, 16)),
        (1, USABLE - 11, USABLE - 11)
    );
    assert_eq!(&pg[USABLE - 11..USABLE], &cell);
    // Exactly 1200 inline, 1201 overflows.
    let k = vec![7u8; 200];
    t.insert(&mut p, &k, &vec![1u8; 1000]).unwrap();
    t.insert(&mut p, &[8u8; 200], &vec![2u8; 1001]).unwrap();
    let s = walk(&mut p, t.root());
    assert_eq!(
        s.pages.len(),
        2,
        "one overflow page for the 1201-byte entry"
    );
    assert_eq!(s.entries[0].1, vec![1u8; 1000]);
    assert_eq!(s.entries[1].1, vec![2u8; 1001]);
    p.commit().unwrap();
}

#[test]
fn leader_deep_trees_merges_and_remove_all() {
    let mut r = Rng::new(0x40_dee9_0001);
    let mut p = new_pager();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let root = t.root();
    let mut m = Model::new();
    // Long keys sharing a 1000-byte prefix (so separators stay long): deep tree.
    for _ in 0..400 {
        let mut k = vec![0xaa; 1024];
        r.fill(&mut k[1000..]);
        let v = value(&mut r, 1024);
        t.insert(&mut p, &k, &v).unwrap();
        m.insert(k, v);
    }
    p.commit().unwrap();
    let s = walk(&mut p, root);
    assert!(
        s.leaf_depth.unwrap() >= 4,
        "1024-byte keys should give depth >= 4, got {:?}",
        s.leaf_depth
    );
    verify(&mut p, t, &m, &mut r);
    account(&mut p, &[root]);
    let mut keys: Vec<_> = m.keys().cloned().collect();
    p.begin().unwrap();
    for i in 0..keys.len() {
        let k = keys.swap_remove(r.below(keys.len()));
        assert!(t.remove(&mut p, &k).unwrap().is_some());
        m.remove(&k);
        if i % 40 == 0 {
            verify(&mut p, t, &m, &mut r);
            account(&mut p, &[root]);
        }
    }
    p.commit().unwrap();
    let s = walk(&mut p, root);
    assert_eq!(
        (s.pages.len(), s.entries.len()),
        (1, 0),
        "remove-all leaves one empty root leaf"
    );
    account(&mut p, &[root]);
    assert_eq!(free_list(&mut p).len() as u64, p.page_count() - 2);

    // Many small entries, then a 90% delete: merges must keep leaves reasonably full.
    p.begin().unwrap();
    for _ in 0..6000 {
        let k: Vec<u8> = (0..8 + r.below(16)).map(|_| r.below(256) as u8).collect();
        let mut v = vec![0u8; r.below(60)];
        r.fill(&mut v);
        t.insert(&mut p, &k, &v).unwrap();
        m.insert(k, v);
    }
    p.commit().unwrap();
    let before = walk(&mut p, root).leaves.len();
    let mut keys: Vec<_> = m.keys().cloned().collect();
    let tenth = m.len() / 10;
    for _ in 0..9 {
        p.begin().unwrap();
        for _ in 0..tenth {
            let k = keys.swap_remove(r.below(keys.len()));
            t.remove(&mut p, &k).unwrap();
            m.remove(&k);
        }
        p.commit().unwrap();
    }
    verify(&mut p, t, &m, &mut r);
    account(&mut p, &[root]);
    let s = walk(&mut p, root);
    let total: usize = s.leaves.iter().map(|l| l.1).sum();
    let bound = 3 * total.div_ceil(CAP) + s.interiors + 2;
    assert!(
        s.leaves.len() <= bound,
        "after a 90% delete: {} leaves for {total} bytes (bound {bound}, {before} before)",
        s.leaves.len()
    );

    // destroy frees everything, a second tree is untouched.
    p.begin().unwrap();
    let t2 = Tree::create(&mut p).unwrap();
    for i in 0..2000u32 {
        t2.insert(&mut p, &i.to_be_bytes(), &vec![3u8; (i % 300) as usize])
            .unwrap();
    }
    t2.insert(&mut p, b"big", &vec![4u8; 50_000]).unwrap();
    p.commit().unwrap();
    account(&mut p, &[root, t2.root()]);
    p.begin().unwrap();
    t2.destroy(&mut p).unwrap();
    p.commit().unwrap();
    account(&mut p, &[root]);
    verify(&mut p, t, &m, &mut r);
}

#[test]
fn leader_snapshots_and_errors() {
    let mut r = Rng::new(0x40_5a95_0001);
    let mut p = new_pager();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let mut m = Model::new();
    random_ops(&mut p, t, &mut m, &mut r, 300);
    p.commit().unwrap();
    let snap = p.snapshot();
    let old = m.clone();
    p.begin().unwrap();
    random_ops(&mut p, t, &mut m, &mut r, 300);
    p.commit().unwrap();
    let want: Vec<_> = old.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(
        t.range(&mut p, Some(snap), Bound::Unbounded, Bound::Unbounded)
            .unwrap(),
        want,
        "snapshot range"
    );
    for _ in 0..30 {
        let k = key(&mut r, &old);
        assert_eq!(
            t.get(&mut p, Some(snap), &k).unwrap(),
            old.get(&k).cloned(),
            "snapshot get"
        );
    }
    let mut n = 0;
    t.scan(&mut p, Some(snap), Bound::Unbounded, &mut |_, _| {
        n += 1;
        true
    })
    .unwrap();
    assert_eq!(n, old.len());
    p.release(snap);
    verify(&mut p, t, &m, &mut r);

    let long = vec![1u8; 1025];
    assert_eq!(t.get(&mut p, None, &long), Err(Error::TooLarge));
    assert_eq!(
        t.insert(&mut p, b"k", b"v"),
        Err(Error::Misuse),
        "insert outside a transaction"
    );
    assert_eq!(
        t.remove(&mut p, b"k"),
        Err(Error::Misuse),
        "remove outside a transaction"
    );
    assert_eq!(Tree::create(&mut p), Err(Error::Misuse));
    p.begin().unwrap();
    assert_eq!(t.insert(&mut p, &long, b"v"), Err(Error::TooLarge));
    assert_eq!(t.remove(&mut p, &long), Err(Error::TooLarge));
    assert_eq!(
        t.insert(&mut p, b"k", &vec![0u8; (1 << 20) + 1]),
        Err(Error::TooLarge)
    );
    assert_eq!(
        t.insert(&mut p, &[9u8; 1024], &vec![5u8; 1 << 20]).unwrap(),
        None,
        "limits are inclusive"
    );
    assert_eq!(
        t.get(&mut p, None, &[9u8; 1024]).unwrap().map(|v| v.len()),
        Some(1 << 20)
    );
    p.rollback();
    verify(&mut p, t, &m, &mut r);
}

#[test]
fn leader_hostile_pages_never_panic() {
    let mut r = Rng::new(0x40_ba0d_0001);
    let mut p = new_pager();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let mut m = Model::new();
    random_ops(&mut p, t, &mut m, &mut r, 500);
    p.commit().unwrap();
    let pages = walk(&mut p, t.root()).pages;
    assert!(pages.len() > 5);
    for case in 0..300 {
        p.begin().unwrap();
        let n = pages[r.below(pages.len())];
        let mut pg = p.read(n).unwrap();
        match case % 4 {
            0 => {
                for _ in 0..1 + r.below(8) {
                    pg[r.below(USABLE)] = r.u64() as u8;
                }
            }
            1 => pg[8..16].copy_from_slice(&n.to_le_bytes()), // link to itself
            2 => {
                let at = 16 + 2 * r.below(4);
                pg[at..at + 2].copy_from_slice(&(r.below(1 << 16) as u16).to_le_bytes());
            }
            _ => pg[0] = r.below(8) as u8,
        }
        p.write(n, &pg).unwrap();
        let k = key(&mut r, &m);
        let _ = t.get(&mut p, None, &k);
        let _ = t.range(&mut p, None, Bound::Unbounded, Bound::Unbounded);
        let _ = t.scan(&mut p, None, Bound::Included(&k), &mut |_, _| true);
        let v = value(&mut r, k.len());
        let _ = t.insert(&mut p, &k, &v);
        let _ = t.remove(&mut p, &k);
        p.rollback();
    }
    verify(&mut p, t, &m, &mut r);
}
