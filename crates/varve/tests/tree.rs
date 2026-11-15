//! Integration and property tests for `varve::Tree`.
#![allow(clippy::unwrap_used, clippy::type_complexity)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use assay::{forall, Rng};
use varve::{Error, MemStorage, Page, Pager, Storage, Tree, PAGE, USABLE};

fn u16_le(p: &Page, off: usize) -> u16 {
    u16::from_le_bytes(p[off..off + 2].try_into().unwrap())
}

fn u64_le(p: &Page, off: usize) -> u64 {
    u64::from_le_bytes(p[off..off + 8].try_into().unwrap())
}

fn read_varint(slice: &[u8]) -> (u64, usize) {
    let mut val = 0u64;
    for (i, &b) in slice.iter().enumerate().take(10) {
        if i == 9 {
            assert_eq!(b, 1);
            val |= 1u64 << 63;
            return (val, 10);
        }
        if b & 0x80 == 0 {
            if i > 0 {
                assert_ne!(b, 0);
            }
            val |= u64::from(b) << (i * 7);
            return (val, i + 1);
        }
        val |= u64::from(b & 0x7f) << (i * 7);
    }
    unreachable!()
}

fn verify_tree_and_accounting<S: Storage>(
    p: &mut Pager<S>,
    trees: &[(Tree, &BTreeMap<Vec<u8>, Vec<u8>>)],
) {
    let page_count = p.page_count();
    let page0 = p.read(0).unwrap();
    let mut free_head = u64_le(&page0, 20);
    let mut free_pages = BTreeSet::new();
    while free_head != 0 {
        assert!(free_head < page_count);
        assert!(free_pages.insert(free_head), "cycle in free list");
        let fp = p.read(free_head).unwrap();
        assert_eq!(fp[0], 5, "free page must have kind 5");
        free_head = u64_le(&fp, 16);
    }

    let mut all_tree_pages = BTreeSet::new();
    for &(tree, model) in trees {
        let mut actual = BTreeMap::new();
        let mut leaves = Vec::new();
        let mut leaf_depth = None;
        verify_subtree(
            p,
            tree.root(),
            1,
            None,
            None,
            &mut leaf_depth,
            &mut all_tree_pages,
            &mut leaves,
            &mut actual,
        );
        assert_eq!(&actual, model, "tree contents mismatch model");

        for i in 0..leaves.len() {
            let (pg_no, link) = leaves[i];
            let expected_link = if i + 1 < leaves.len() {
                leaves[i + 1].0
            } else {
                0
            };
            assert_eq!(link, expected_link, "leaf {pg_no} link mismatch");
        }
    }

    for pg in &all_tree_pages {
        assert!(
            !free_pages.contains(pg),
            "page {pg} in both tree and free list"
        );
    }
    assert_eq!(
        1 + all_tree_pages.len() + free_pages.len(),
        page_count as usize,
        "page accounting mismatch: page_count={page_count}, tree={}, free={}",
        all_tree_pages.len(),
        free_pages.len()
    );
}

#[allow(clippy::too_many_arguments)]
fn verify_subtree<S: Storage>(
    p: &mut Pager<S>,
    page_no: u64,
    depth: usize,
    lo: Option<&[u8]>,
    hi: Option<&[u8]>,
    leaf_depth: &mut Option<usize>,
    seen_pages: &mut BTreeSet<u64>,
    leaves: &mut Vec<(u64, u64)>,
    model: &mut BTreeMap<Vec<u8>, Vec<u8>>,
) {
    assert!(depth <= 32, "depth exceeded 32");
    assert!(page_no > 0 && page_no < p.page_count());
    assert!(seen_pages.insert(page_no), "duplicate tree page {page_no}");

    let page = p.read(page_no).unwrap();
    let kind = page[0];
    assert!(kind == 2 || kind == 3, "bad page kind {kind}");
    assert_eq!(page[1], 0);
    let count = usize::from(u16_le(&page, 2));
    let cs = usize::from(u16_le(&page, 4));
    let frag = usize::from(u16_le(&page, 6));
    let link = u64_le(&page, 8);
    assert!(cs >= 16 + 2 * count && cs <= USABLE);
    assert!(cs + frag <= USABLE);

    if kind == 3 {
        if depth > 1 {
            assert!(count >= 1, "non-root leaf must have >= 1 cell");
        }
        match *leaf_depth {
            Some(d) => assert_eq!(d, depth, "unequal leaf depth"),
            None => *leaf_depth = Some(depth),
        }
        leaves.push((page_no, link));

        let mut prev_key: Option<Vec<u8>> = None;
        for i in 0..count {
            let off = usize::from(u16_le(&page, 16 + 2 * i));
            assert!(off >= cs && off < USABLE);
            let (klen_u, kl_n) = read_varint(&page[off..USABLE]);
            let (vlen_u, vl_n) = read_varint(&page[off + kl_n..USABLE]);
            let klen = klen_u as usize;
            let vlen = vlen_u as usize;
            assert!(klen <= 1024);
            assert!(vlen <= (1 << 20));
            let ks = off + kl_n + vl_n;
            let key = page[ks..ks + klen].to_vec();
            if let Some(pk) = &prev_key {
                assert!(pk < &key, "unsorted leaf keys");
            }
            if let Some(l) = lo {
                assert!(key.as_slice() >= l, "leaf key below lo separator");
            }
            if let Some(h) = hi {
                assert!(key.as_slice() < h, "leaf key >= hi separator");
            }
            prev_key = Some(key.clone());

            let inline_len = if klen + vlen <= 1200 {
                vlen
            } else {
                1200 - klen
            };
            let mut val = page[ks + klen..ks + klen + inline_len].to_vec();
            if klen + vlen > 1200 {
                let mut ov = u64_le(&page, ks + klen + inline_len);
                let mut rem = vlen - inline_len;
                while rem > 0 {
                    assert!(ov > 0 && ov < p.page_count());
                    assert!(seen_pages.insert(ov), "duplicate overflow page {ov}");
                    let ov_page = p.read(ov).unwrap();
                    assert_eq!(ov_page[0], 4);
                    let take = rem.min(USABLE - 24);
                    val.extend_from_slice(&ov_page[24..24 + take]);
                    rem -= take;
                    ov = u64_le(&ov_page, 16);
                }
                assert_eq!(ov, 0, "overflow chain longer than needed");
            }
            assert!(model.insert(key, val).is_none());
        }
    } else {
        assert!(count >= 1, "interior page must have >= 1 cell");
        let mut keys = Vec::with_capacity(count);
        let mut children = Vec::with_capacity(count + 1);
        for i in 0..count {
            let off = usize::from(u16_le(&page, 16 + 2 * i));
            assert!(off >= cs && off < USABLE);
            let child = u64_le(&page, off);
            let (klen_u, kl_n) = read_varint(&page[off + 8..USABLE]);
            let klen = klen_u as usize;
            assert!(klen <= 1024);
            let key = page[off + 8 + kl_n..off + 8 + kl_n + klen].to_vec();
            if i > 0 {
                assert!(keys[i - 1] < key, "unsorted interior keys");
            }
            if let Some(l) = lo {
                assert!(key.as_slice() > l, "interior key <= lo");
            }
            if let Some(h) = hi {
                assert!(key.as_slice() < h, "interior key >= hi");
            }
            children.push(child);
            keys.push(key);
        }
        children.push(link);

        for (i, &ch) in children.iter().enumerate() {
            let c_lo = if i == 0 {
                lo
            } else {
                Some(keys[i - 1].as_slice())
            };
            let c_hi = if i < count {
                Some(keys[i].as_slice())
            } else {
                hi
            };
            verify_subtree(
                p,
                ch,
                depth + 1,
                c_lo,
                c_hi,
                leaf_depth,
                seen_pages,
                leaves,
                model,
            );
        }
    }
}

#[test]
fn byte_exact_page_layout() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();

    // Empty root leaf: kind 3, flags 0, count 0, cs 4092, frag 0, link 0.
    let pg = p.read(t.root()).unwrap();
    assert_eq!(pg[0], 3);
    assert_eq!(pg[1], 0);
    assert_eq!(u16_le(&pg, 2), 0);
    assert_eq!(u16_le(&pg, 4), 4092);
    assert_eq!(u16_le(&pg, 6), 0);
    assert_eq!(u64_le(&pg, 8), 0);

    // First cell ends at 4092: key="a"(1), val="bb"(2) -> 5 bytes at 4087..4092.
    t.insert(&mut p, b"a", b"bb").unwrap();
    let pg = p.read(t.root()).unwrap();
    assert_eq!(u16_le(&pg, 2), 1);
    assert_eq!(u16_le(&pg, 4), 4087);
    assert_eq!(u16_le(&pg, 6), 0);
    assert_eq!(u16_le(&pg, 16), 4087);
    assert_eq!(&pg[4087..4092], &[1, 2, b'a', b'b', b'b']);

    // Inline boundary: klen=100, vlen=1100 => klen+vlen=1200 => inline, no overflow.
    let k100 = vec![b'k'; 100];
    let v1100 = vec![b'v'; 1100];
    t.insert(&mut p, &k100, &v1100).unwrap();
    assert_eq!(p.page_count(), 2);

    // klen=100, vlen=1101 => klen+vlen=1201 => 1100 inline + 1 byte in overflow page.
    let mut k100b = vec![b'k'; 100];
    k100b[99] = b'l';
    let mut v1101 = vec![b'w'; 1101];
    v1101[1100] = 0xAB;
    t.insert(&mut p, &k100b, &v1101).unwrap();
    assert_eq!(p.page_count(), 3);
    let ov_pg = p.read(2).unwrap();
    assert_eq!(ov_pg[0], 4);
    assert_eq!(u64_le(&ov_pg, 16), 0);
    assert_eq!(ov_pg[24], 0xAB);

    // Defragment trigger: fill leaf with 3 cells of 1100 bytes, delete middle, insert 1100-byte cell.
    let t2 = Tree::create(&mut p).unwrap();
    let big = vec![b'x'; 1100];
    t2.insert(&mut p, b"01", &big).unwrap();
    t2.insert(&mut p, b"02", &big).unwrap();
    t2.insert(&mut p, b"03", &big).unwrap();
    t2.remove(&mut p, b"02").unwrap();
    let mid_pg = p.read(t2.root()).unwrap();
    assert!(
        u16_le(&mid_pg, 6) > 1100,
        "fragmented bytes should be recorded"
    );
    t2.insert(&mut p, b"04", &big).unwrap();
    let after_pg = p.read(t2.root()).unwrap();
    assert_eq!(
        u16_le(&after_pg, 6),
        0,
        "defragmentation should reset fragmented bytes"
    );
    assert_eq!(u16_le(&after_pg, 2), 3);

    p.commit().unwrap();
}

#[test]
fn sub_ranges_every_bound_and_scan_early_stop() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let mut model = BTreeMap::new();
    for i in 0..80u16 {
        let k = format!("k{:04}", i * 2).into_bytes();
        let v = vec![(i % 251) as u8; 80];
        t.insert(&mut p, &k, &v).unwrap();
        model.insert(k, v);
    }
    p.commit().unwrap();

    let probes: [&[u8]; 5] = [b"", b"k0000", b"k0021", b"k0080", b"zzzz"];
    for &lo_k in &probes {
        for &hi_k in &probes {
            let lo_bounds = [
                Bound::Unbounded,
                Bound::Included(lo_k),
                Bound::Excluded(lo_k),
            ];
            let hi_bounds = [
                Bound::Unbounded,
                Bound::Included(hi_k),
                Bound::Excluded(hi_k),
            ];
            for lo in lo_bounds {
                for hi in hi_bounds {
                    let got = t.range(&mut p, None, lo, hi).unwrap();
                    let empty_expected = match (lo, hi) {
                        (Bound::Included(a), Bound::Included(b)) => a > b,
                        (Bound::Included(a), Bound::Excluded(b))
                        | (Bound::Excluded(a), Bound::Included(b))
                        | (Bound::Excluded(a), Bound::Excluded(b)) => a >= b,
                        _ => false,
                    };
                    let expected: Vec<(Vec<u8>, Vec<u8>)> = if empty_expected {
                        Vec::new()
                    } else {
                        model
                            .range::<[u8], _>((lo, hi))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    };
                    assert_eq!(got, expected);
                }
            }
        }
    }

    // Scan early stop at 0, 1, 17 items across multiple leaves.
    for limit in [1usize, 5, 25] {
        let mut collected = Vec::new();
        t.scan(&mut p, None, Bound::Included(b"k0010"), &mut |k, v| {
            collected.push((k.to_vec(), v.to_vec()));
            collected.len() < limit
        })
        .unwrap();
        let expected: Vec<(Vec<u8>, Vec<u8>)> = model
            .range::<[u8], _>((Bound::Included(b"k0010".as_slice()), Bound::Unbounded))
            .take(limit)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(collected, expected);
    }
}

#[test]
fn snapshots_and_isolation() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    for i in 0..30u8 {
        t.insert(&mut p, &[i], &vec![i; 1300]).unwrap();
    }
    p.commit().unwrap();
    let s1 = p.snapshot();

    p.begin().unwrap();
    for i in 0..15u8 {
        t.remove(&mut p, &[i]).unwrap();
    }
    for i in 30..60u8 {
        t.insert(&mut p, &[i], &vec![i; 500]).unwrap();
    }
    // Uncommitted view sees changes, s1 sees old committed state.
    assert_eq!(t.get(&mut p, None, &[0]).unwrap(), None);
    assert_eq!(t.get(&mut p, Some(s1), &[0]).unwrap(), Some(vec![0; 1300]));
    assert_eq!(t.get(&mut p, None, &[50]).unwrap(), Some(vec![50; 500]));
    assert_eq!(t.get(&mut p, Some(s1), &[50]).unwrap(), None);

    p.commit().unwrap();
    let s2 = p.snapshot();

    p.begin().unwrap();
    t.insert(&mut p, &[0], b"new").unwrap();
    p.rollback();

    let r1 = t
        .range(&mut p, Some(s1), Bound::Unbounded, Bound::Unbounded)
        .unwrap();
    assert_eq!(r1.len(), 30);
    let r2 = t
        .range(&mut p, Some(s2), Bound::Unbounded, Bound::Unbounded)
        .unwrap();
    assert_eq!(r2.len(), 45);
    assert_eq!(t.get(&mut p, None, &[0]).unwrap(), None);

    p.release(s1);
    assert_eq!(t.get(&mut p, Some(s1), &[0]), Err(Error::Misuse));
    p.release(s2);
}

#[test]
fn deep_tree_1024_byte_keys_and_root_invariance() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let fixed_root = t.root();
    let mut model = BTreeMap::new();

    for i in 0..160u16 {
        let mut k = vec![0x55u8; 1024];
        k[0..2].copy_from_slice(&i.to_be_bytes());
        k[1022..1024].copy_from_slice(&i.to_le_bytes());
        let v = vec![(i % 199) as u8; 300 + (i as usize % 300)];
        assert_eq!(t.insert(&mut p, &k, &v).unwrap(), None);
        model.insert(k, v);
    }
    assert_eq!(t.root(), fixed_root);
    p.commit().unwrap();

    verify_tree_and_accounting(&mut p, &[(t, &model)]);
    for (k, v) in &model {
        assert_eq!(t.get(&mut p, None, k).unwrap().as_ref(), Some(v));
    }
}

#[test]
fn mass_deletes_merges_and_remove_all() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    let mut model = BTreeMap::new();

    for i in 0..250u32 {
        let mut k = vec![0xAAu8; 200];
        k[0..4].copy_from_slice(&i.to_be_bytes());
        let v = if i % 7 == 0 {
            vec![(i & 0xff) as u8; 5000]
        } else {
            vec![(i & 0xff) as u8; 120]
        };
        t.insert(&mut p, &k, &v).unwrap();
        model.insert(k, v);
    }
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[(t, &model)]);

    // Mass delete 90% in strided order.
    p.begin().unwrap();
    let keys: Vec<Vec<u8>> = model.keys().cloned().collect();
    for (idx, k) in keys.iter().enumerate() {
        if idx % 10 != 0 {
            let old = t.remove(&mut p, k).unwrap();
            assert_eq!(old, model.remove(k));
        }
    }
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[(t, &model)]);

    // Check that merges reclaimed most pages.
    let pg0 = p.read(0).unwrap();
    let mut fh = u64_le(&pg0, 20);
    let mut freed = 0usize;
    while fh != 0 {
        freed += 1;
        fh = u64_le(&p.read(fh).unwrap(), 16);
    }
    let active_pages = (p.page_count() as usize) - 1 - freed;
    assert!(
        active_pages <= 12,
        "expected merges to shrink tree, active={active_pages}"
    );

    // Delete remaining keys -> single empty root leaf, all other pages freed.
    p.begin().unwrap();
    let rem: Vec<Vec<u8>> = model.keys().cloned().collect();
    for k in rem {
        let old = t.remove(&mut p, &k).unwrap();
        assert_eq!(old, model.remove(&k));
    }
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[(t, &model)]);

    let root_pg = p.read(t.root()).unwrap();
    assert_eq!(root_pg[0], 3);
    assert_eq!(u16_le(&root_pg, 2), 0);
    assert_eq!(u64_le(&root_pg, 8), 0);

    // Re-insert after remove-all.
    p.begin().unwrap();
    t.insert(&mut p, b"again", b"ok").unwrap();
    model.insert(b"again".to_vec(), b"ok".to_vec());
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[(t, &model)]);
}

#[test]
fn two_trees_and_destroy_page_accounting() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let t1 = Tree::create(&mut p).unwrap();
    let t2 = Tree::create(&mut p).unwrap();
    let mut m1 = BTreeMap::new();
    let mut m2 = BTreeMap::new();

    for i in 0..80u16 {
        let k = i.to_be_bytes().to_vec();
        let v1 = vec![1u8; if i % 5 == 0 { 5000 } else { 150 }];
        let v2 = vec![2u8; if i % 4 == 0 { 4500 } else { 200 }];
        t1.insert(&mut p, &k, &v1).unwrap();
        t2.insert(&mut p, &k, &v2).unwrap();
        m1.insert(k.clone(), v1);
        m2.insert(k, v2);
    }
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[(t1, &m1), (t2, &m2)]);

    p.begin().unwrap();
    t1.destroy(&mut p).unwrap();
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[(t2, &m2)]);

    p.begin().unwrap();
    t2.destroy(&mut p).unwrap();
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[]);

    // Empty tree destroy also frees root.
    p.begin().unwrap();
    let t3 = Tree::create(&mut p).unwrap();
    t3.destroy(&mut p).unwrap();
    p.commit().unwrap();
    verify_tree_and_accounting(&mut p, &[]);
}

#[test]
fn error_cases_limits_and_misuse() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    assert_eq!(Tree::create(&mut p), Err(Error::Misuse));

    p.begin().unwrap();
    let t = Tree::create(&mut p).unwrap();
    p.commit().unwrap();

    // Outside tx -> Misuse even on no-op or oversized.
    assert_eq!(t.insert(&mut p, b"k", b"v"), Err(Error::Misuse));
    assert_eq!(t.remove(&mut p, b"missing"), Err(Error::Misuse));
    assert_eq!(t.destroy(&mut p), Err(Error::Misuse));

    let k1024 = vec![b'a'; 1024];
    let k1025 = vec![b'a'; 1025];
    let v_1mib = vec![b'b'; 1 << 20];
    let v_over = vec![b'b'; (1 << 20) + 1];

    p.begin().unwrap();
    assert_eq!(t.insert(&mut p, &k1025, b"x"), Err(Error::TooLarge));
    assert_eq!(t.insert(&mut p, b"x", &v_over), Err(Error::TooLarge));
    assert_eq!(t.get(&mut p, None, &k1025), Err(Error::TooLarge));
    assert_eq!(t.remove(&mut p, &k1025), Err(Error::TooLarge));
    assert_eq!(
        t.scan(&mut p, None, Bound::Included(&k1025), &mut |_, _| true),
        Err(Error::TooLarge)
    );
    assert_eq!(
        t.range(&mut p, None, Bound::Unbounded, Bound::Excluded(&k1025)),
        Err(Error::TooLarge)
    );

    // Exact inclusive limits succeed!
    assert_eq!(t.insert(&mut p, &k1024, &v_1mib).unwrap(), None);
    assert_eq!(t.get(&mut p, None, &k1024).unwrap().as_ref(), Some(&v_1mib));
    assert_eq!(t.remove(&mut p, &k1024).unwrap().as_ref(), Some(&v_1mib));
    p.commit().unwrap();
}

#[test]
fn hostile_pages_corrupt_no_panic() {
    // 1. Bad kind & non-zero flags.
    {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let t = Tree::create(&mut p).unwrap();
        let mut pg = p.read(t.root()).unwrap();
        pg[0] = 9;
        p.write(t.root(), &pg).unwrap();
        assert_eq!(t.get(&mut p, None, b"a"), Err(Error::Corrupt));

        pg[0] = 3;
        pg[1] = 1;
        p.write(t.root(), &pg).unwrap();
        assert_eq!(t.get(&mut p, None, b"a"), Err(Error::Corrupt));
        p.rollback();
    }

    // 2. Unsorted keys & overlapping/out-of-range offsets.
    {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let t = Tree::create(&mut p).unwrap();
        t.insert(&mut p, b"a", b"1").unwrap();
        t.insert(&mut p, b"b", b"2").unwrap();
        let mut pg = p.read(t.root()).unwrap();
        // Swap offsets so keys are "b" then "a".
        let o0 = pg[16..18].to_vec();
        let o1 = pg[18..20].to_vec();
        pg[16..18].copy_from_slice(&o1);
        pg[18..20].copy_from_slice(&o0);
        p.write(t.root(), &pg).unwrap();
        assert_eq!(t.get(&mut p, None, b"a"), Err(Error::Corrupt));

        // Offset past 4092.
        pg[16..18].copy_from_slice(&4093u16.to_le_bytes());
        p.write(t.root(), &pg).unwrap();
        assert_eq!(t.get(&mut p, None, b"a"), Err(Error::Corrupt));

        // Overlapping offsets (both point to same cell).
        pg[16..18].copy_from_slice(&o0);
        pg[18..20].copy_from_slice(&o0);
        p.write(t.root(), &pg).unwrap();
        assert_eq!(t.get(&mut p, None, b"a"), Err(Error::Corrupt));
        p.rollback();
    }

    // 3. Deeper than 32 levels (self-loop interior page).
    {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let r = p.alloc().unwrap();
        let mut pg = [0u8; PAGE];
        pg[0] = 2;
        pg[2..4].copy_from_slice(&1u16.to_le_bytes());
        let cell_off = USABLE - 10;
        pg[4..6].copy_from_slice(&(cell_off as u16).to_le_bytes());
        pg[8..16].copy_from_slice(&r.to_le_bytes());
        pg[16..18].copy_from_slice(&(cell_off as u16).to_le_bytes());
        pg[cell_off..cell_off + 8].copy_from_slice(&r.to_le_bytes());
        pg[cell_off + 8] = 1;
        pg[cell_off + 9] = b'm';
        p.write(r, &pg).unwrap();
        let t = Tree::open(r);
        assert_eq!(t.get(&mut p, None, b"a"), Err(Error::Corrupt));
        assert_eq!(t.destroy(&mut p), Err(Error::Corrupt));
        p.rollback();
    }

    // 4. Overflow chain longer than needed (next != 0 on tail) and leaf chain cycle.
    {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let t = Tree::create(&mut p).unwrap();
        let big = vec![b'z'; 1500];
        t.insert(&mut p, b"k", &big).unwrap();
        let extra = p.alloc().unwrap();
        let mut extra_pg = [0u8; PAGE];
        extra_pg[0] = 4;
        p.write(extra, &extra_pg).unwrap();

        let mut ov_pg = p.read(2).unwrap();
        ov_pg[16..24].copy_from_slice(&extra.to_le_bytes());
        p.write(2, &ov_pg).unwrap();
        assert_eq!(t.get(&mut p, None, b"k"), Err(Error::Corrupt));
        p.rollback();
    }

    // 5. Leaf chain longer than page_count (leaf points to itself).
    {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let t = Tree::create(&mut p).unwrap();
        t.insert(&mut p, b"k", b"v").unwrap();
        let mut pg = p.read(t.root()).unwrap();
        pg[8..16].copy_from_slice(&t.root().to_le_bytes());
        p.write(t.root(), &pg).unwrap();
        assert_eq!(
            t.scan(&mut p, None, Bound::Unbounded, &mut |_, _| true),
            Err(Error::Corrupt)
        );
        p.rollback();
    }
}

fn gen_key(rng: &mut Rng) -> Vec<u8> {
    match rng.range(0, 10) {
        0 => Vec::new(),
        1 => rng.bytes(1024),
        2 => {
            let mut k = vec![0x33; 1024];
            let tail = rng.u64().to_be_bytes();
            k[1016..1024].copy_from_slice(&tail);
            k
        }
        _ => {
            let len = rng.range(1, 48) as usize;
            let mut k = vec![0u8; len];
            rng.fill(&mut k);
            k
        }
    }
}

fn gen_val(rng: &mut Rng) -> Vec<u8> {
    let len = match rng.range(0, 10) {
        0 => 0,
        1 => 1199,
        2 => 1200,
        3 => 1201,
        4 => rng.range(4000, 9500) as usize,
        _ => rng.range(1, 250) as usize,
    };
    let mut v = vec![0u8; len];
    rng.fill(&mut v);
    v
}

#[test]
fn random_ops_model_property() {
    forall(15, 0x5440_2026, |rng| {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let t = Tree::create(&mut p).unwrap();
        p.commit().unwrap();

        let mut committed_model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let mut tx_model = committed_model.clone();
        let mut in_tx = false;
        let mut snapshots = Vec::new();

        for step in 0..140 {
            if !in_tx {
                p.begin().unwrap();
                tx_model = committed_model.clone();
                in_tx = true;
            }
            match rng.range(0, 10) {
                0..=4 => {
                    let k = if !tx_model.is_empty() && rng.chance(1, 4) {
                        let keys: Vec<Vec<u8>> = tx_model.keys().cloned().collect();
                        rng.pick(&keys).clone()
                    } else {
                        gen_key(rng)
                    };
                    let v = gen_val(rng);
                    let got = t.insert(&mut p, &k, &v).unwrap();
                    let exp = tx_model.insert(k, v);
                    assert_eq!(got, exp);
                }
                5..=6 => {
                    let k = if !tx_model.is_empty() && rng.chance(3, 4) {
                        let keys: Vec<Vec<u8>> = tx_model.keys().cloned().collect();
                        rng.pick(&keys).clone()
                    } else {
                        gen_key(rng)
                    };
                    let got = t.remove(&mut p, &k).unwrap();
                    let exp = tx_model.remove(&k);
                    assert_eq!(got, exp);
                }
                7 => {
                    let k = if !tx_model.is_empty() && rng.chance(1, 2) {
                        let keys: Vec<Vec<u8>> = tx_model.keys().cloned().collect();
                        rng.pick(&keys).clone()
                    } else {
                        gen_key(rng)
                    };
                    assert_eq!(t.get(&mut p, None, &k).unwrap(), tx_model.get(&k).cloned());
                }
                8 => {
                    let a = gen_key(rng);
                    let b = gen_key(rng);
                    let (lo_k, hi_k) = if a <= b { (a, b) } else { (b, a) };
                    let got = t
                        .range(&mut p, None, Bound::Included(&lo_k), Bound::Excluded(&hi_k))
                        .unwrap();
                    let exp: Vec<(Vec<u8>, Vec<u8>)> = tx_model
                        .range::<[u8], _>((
                            Bound::Included(lo_k.as_slice()),
                            Bound::Excluded(hi_k.as_slice()),
                        ))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    assert_eq!(got, exp);
                }
                _ => {
                    let stop_at = rng.range(1, 10) as usize;
                    let mut got = Vec::new();
                    t.scan(&mut p, None, Bound::Unbounded, &mut |k, v| {
                        got.push((k.to_vec(), v.to_vec()));
                        got.len() < stop_at
                    })
                    .unwrap();
                    let exp: Vec<(Vec<u8>, Vec<u8>)> = tx_model
                        .iter()
                        .take(stop_at)
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    assert_eq!(got, exp);
                }
            }

            if step % 15 == 14 {
                if rng.chance(1, 5) {
                    p.rollback();
                    in_tx = false;
                } else {
                    p.commit().unwrap();
                    committed_model = tx_model.clone();
                    in_tx = false;
                    if snapshots.len() < 3 && rng.chance(1, 2) {
                        snapshots.push((p.snapshot(), committed_model.clone()));
                    }
                    if rng.chance(1, 3) {
                        for (s, _) in snapshots.drain(..) {
                            p.release(s);
                        }
                        let (main, log) = p.close().unwrap();
                        p = Pager::open(main, log).unwrap();
                    }
                }
                for (s, m) in &snapshots {
                    let got = t
                        .range(&mut p, Some(*s), Bound::Unbounded, Bound::Unbounded)
                        .unwrap();
                    let exp: Vec<(Vec<u8>, Vec<u8>)> =
                        m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    assert_eq!(got, exp);
                }
                if !in_tx {
                    verify_tree_and_accounting(&mut p, &[(t, &committed_model)]);
                }
            }
        }

        if in_tx {
            p.commit().unwrap();
            committed_model = tx_model;
        }
        verify_tree_and_accounting(&mut p, &[(t, &committed_model)]);
        for (s, _) in snapshots.drain(..) {
            p.release(s);
        }
        p.begin().unwrap();
        t.destroy(&mut p).unwrap();
        p.commit().unwrap();
        verify_tree_and_accounting(&mut p, &[]);
    });
}
