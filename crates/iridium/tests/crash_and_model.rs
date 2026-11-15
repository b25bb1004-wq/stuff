//! Model-based property tests and exhaustive crash-at-every-op tests with CrashStorage.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use assay::{forall, Rng};
use iridium::{CrashStorage, MemStorage, Page, Pager, PAGE, USABLE};

#[derive(Clone)]
struct Model {
    page_count: u64,
    root: u64,
    free_list: Vec<u64>, // stack: top is last
    pages: BTreeMap<u64, Page>,
}

impl Model {
    fn new() -> Self {
        Self {
            page_count: 1,
            root: 0,
            free_list: Vec::new(),
            pages: BTreeMap::new(),
        }
    }

    fn is_free(&self, p: u64) -> bool {
        self.free_list.contains(&p)
    }

    fn alloc(&mut self) -> u64 {
        let p = if let Some(recycled) = self.free_list.pop() {
            recycled
        } else {
            let n = self.page_count;
            self.page_count += 1;
            n
        };
        self.pages.insert(p, [0u8; PAGE]);
        p
    }

    fn free(&mut self, p: u64) {
        self.free_list.push(p);
        self.pages.remove(&p);
    }

    fn write(&mut self, p: u64, data: &Page) {
        self.pages.insert(p, *data);
    }

    fn verify(&self, pager: &mut Pager<MemStorage>) {
        assert_eq!(pager.page_count(), self.page_count);
        assert_eq!(pager.root(), self.root);
        for (&pg, expected) in &self.pages {
            let actual = pager.read(pg).unwrap();
            assert_eq!(&actual[..USABLE], &expected[..USABLE], "page {pg} mismatch");
        }
    }
}

#[test]
fn model_based_random_transactions_reopened_every_step() {
    forall(40, 0x1234_5678, |rng| {
        let mut model = Model::new();
        let (mut main, mut log) = (MemStorage::new(), MemStorage::new());

        for _step in 0..35 {
            let mut pager = Pager::open(main, log).unwrap();
            model.verify(&mut pager);

            let mut staged = model.clone();
            pager.begin().unwrap();
            let ops = rng.range(1, 8);
            for _ in 0..ops {
                let choice = rng.range(0, 10);
                if choice < 3 {
                    let p1 = pager.alloc().unwrap();
                    let p2 = staged.alloc();
                    assert_eq!(p1, p2);
                } else if choice < 7 {
                    let live: Vec<u64> = (1..staged.page_count)
                        .filter(|p| !staged.is_free(*p))
                        .collect();
                    if let Some(&pg) = live.get(rng.below(live.len())) {
                        let mut buf = [0u8; PAGE];
                        rng.fill(&mut buf[..USABLE]);
                        pager.write(pg, &buf).unwrap();
                        staged.write(pg, &buf);
                    }
                } else if choice < 9 {
                    let live: Vec<u64> = (1..staged.page_count)
                        .filter(|p| !staged.is_free(*p))
                        .collect();
                    if let Some(&pg) = live.get(rng.below(live.len())) {
                        pager.free(pg).unwrap();
                        staged.free(pg);
                    }
                } else {
                    let new_root = rng.range(0, staged.page_count);
                    pager.set_root(new_root).unwrap();
                    staged.root = new_root;
                }
            }

            if rng.chance(3, 4) {
                pager.commit().unwrap();
                model = staged;
            } else {
                pager.rollback();
            }

            if rng.chance(1, 5) {
                assert!(pager.checkpoint().unwrap());
            }

            (main, log) = pager.into_storage();
        }

        let mut pager = Pager::open(main, log).unwrap();
        model.verify(&mut pager);
    });
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct DbSnapshot {
    page_count: u64,
    root: u64,
    pages: BTreeMap<u64, Vec<u8>>,
}

fn capture_db(p: &mut Pager<MemStorage>) -> DbSnapshot {
    let mut pages = BTreeMap::new();
    for i in 1..p.page_count() {
        let pg = p.read(i).unwrap();
        pages.insert(i, pg[..USABLE].to_vec());
    }
    DbSnapshot {
        page_count: p.page_count(),
        root: p.root(),
        pages,
    }
}

fn run_workload_with_budget(
    main_init: MemStorage,
    log_init: MemStorage,
    budget: usize,
) -> (
    CrashStorage<MemStorage>,
    CrashStorage<MemStorage>,
    Vec<DbSnapshot>,
    bool,
) {
    let gate = Arc::new(AtomicUsize::new(budget));
    let cm = CrashStorage::with_budget(main_init, gate.clone()).unwrap();
    let cl = CrashStorage::with_budget(log_init, gate).unwrap();
    let mut states = Vec::new();

    let mut p = match Pager::open(cm, cl) {
        Ok(p) => p,
        Err(_) => {
            // Failed during open; cannot extract storages from open error directly here,
            // so caller uses total op counting starting from initialized DB or separate test.
            unreachable!()
        }
    };

    // Capture initial state
    for tx_idx in 0..4u64 {
        if p.begin().is_err() {
            let (m, l) = p.into_storage();
            return (m, l, states, false);
        }
        let alloc_res = p.alloc();
        let Ok(pg) = alloc_res else {
            let (m, l) = p.into_storage();
            return (m, l, states, false);
        };
        let mut buf = [0u8; PAGE];
        buf[0..8].copy_from_slice(&(tx_idx + 1).to_le_bytes());
        buf[100..108].copy_from_slice(&0xCAFE_BABEu64.to_le_bytes());
        if p.write(pg, &buf).is_err() || p.set_root(pg).is_err() || p.commit().is_err() {
            let (m, l) = p.into_storage();
            return (m, l, states, false);
        }
        // Snapshot committed state after commit succeeded.
        let mut snap_pages = BTreeMap::new();
        for i in 1..p.page_count() {
            snap_pages.insert(i, p.read(i).unwrap()[..USABLE].to_vec());
        }
        states.push(DbSnapshot {
            page_count: p.page_count(),
            root: p.root(),
            pages: snap_pages,
        });

        if tx_idx == 2 && p.checkpoint().is_err() {
            let (m, l) = p.into_storage();
            return (m, l, states, false);
        }
    }

    let (m, l) = p.into_storage();
    (m, l, states, true)
}

#[test]
fn crash_at_every_storage_op_across_sector_loss_patterns() {
    // First find total ops on an unlimited run.
    let p0 = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    let (base_main, base_log) = p0.into_storage();

    let (m_full, l_full, full_states, completed) =
        run_workload_with_budget(base_main.clone(), base_log.clone(), 100_000);
    assert!(completed);
    let total_ops = m_full.op_count() + l_full.op_count();
    assert_eq!(full_states.len(), 4);

    for fail_at in 0..=total_ops {
        for pattern in 0..4 {
            let (mut cm, mut cl, committed_states, finished) =
                run_workload_with_budget(base_main.clone(), base_log.clone(), fail_at);
            if finished {
                continue;
            }
            let mut rng = Rng::new((fail_at as u64) * 31 + pattern as u64 + 1);
            let mut apply_crash = |cs: &mut CrashStorage<MemStorage>| match pattern {
                0 => cs.crash_drop_all().unwrap(),
                1 => cs.crash_keep_all().unwrap(),
                2 => cs.crash_with(|_, sector| sector % 2 == 0).unwrap(),
                _ => cs.crash_with(|_, _| rng.chance(1, 2)).unwrap(),
            };
            apply_crash(&mut cm);
            apply_crash(&mut cl);

            let mut recovered = Pager::open(cm.into_inner(), cl.into_inner()).unwrap();
            let got = capture_db(&mut recovered);

            let min_idx = committed_states.len();
            let valid_after_in_flight = if min_idx < full_states.len() {
                got == full_states[min_idx]
            } else {
                false
            };
            let valid_before_in_flight = if min_idx == 0 {
                got.page_count == 1 && got.root == 0 && got.pages.is_empty()
            } else {
                got == full_states[min_idx - 1]
            };

            assert!(
                valid_before_in_flight || valid_after_in_flight,
                "crash at op {fail_at} pattern {pattern}: committed={}, got={got:?}",
                committed_states.len()
            );

            // Second crash during recovery's checkpoint!
            let (rec_m, rec_l) = recovered.into_storage();
            let mut p_ck = Pager::open(rec_m.clone(), rec_l.clone()).unwrap();
            p_ck.checkpoint().unwrap();
            let expected_after_ck = capture_db(&mut p_ck);

            for ck_fail in 0..=8 {
                let gate = Arc::new(AtomicUsize::new(ck_fail));
                let ck_m = CrashStorage::with_budget(rec_m.clone(), gate.clone()).unwrap();
                let ck_l = CrashStorage::with_budget(rec_l.clone(), gate).unwrap();
                let mut p2 = Pager::open(ck_m, ck_l).unwrap();
                let _ = p2.checkpoint();
                let (mut ck_m, mut ck_l) = p2.into_storage();
                ck_m.crash_with(|_, sector| sector % 2 == 1).unwrap();
                ck_l.crash_with(|_, sector| sector % 2 == 0).unwrap();
                let mut p_verify = Pager::open(ck_m.into_inner(), ck_l.into_inner()).unwrap();
                assert_eq!(capture_db(&mut p_verify), expected_after_ck);
            }
        }
    }
}

#[test]
fn second_crash_during_checkpoint_recovery() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    for i in 1..=5u64 {
        p.begin().unwrap();
        let pg = p.alloc().unwrap();
        let mut buf = [0u8; PAGE];
        buf[0..8].copy_from_slice(&i.to_le_bytes());
        p.write(pg, &buf).unwrap();
        p.set_root(pg).unwrap();
        p.commit().unwrap();
    }
    let expected = {
        let (m, l) = p.into_storage();
        let mut p2 = Pager::open(m, l).unwrap();
        let snap = capture_db(&mut p2);
        let (m, l) = p2.into_storage();
        (snap, m, l)
    };
    let (expected_snap, rec_m, rec_l) = expected;

    for fail_at in 0..=15 {
        for pattern in 0..3 {
            let gate = Arc::new(AtomicUsize::new(fail_at));
            let cm = CrashStorage::with_budget(rec_m.clone(), gate.clone()).unwrap();
            let cl = CrashStorage::with_budget(rec_l.clone(), gate).unwrap();
            let mut p_ck = Pager::open(cm, cl).unwrap();
            let _ = p_ck.checkpoint();
            let (mut cm, mut cl) = p_ck.into_storage();
            let mut rng = Rng::new((fail_at as u64) * 17 + pattern + 7);
            match pattern {
                0 => {
                    cm.crash_drop_all().unwrap();
                    cl.crash_drop_all().unwrap();
                }
                1 => {
                    cm.crash_keep_all().unwrap();
                    cl.crash_keep_all().unwrap();
                }
                _ => {
                    cm.crash_with(|_, _| rng.chance(1, 2)).unwrap();
                    cl.crash_with(|_, _| rng.chance(1, 2)).unwrap();
                }
            }
            let mut p_after = Pager::open(cm.into_inner(), cl.into_inner()).unwrap();
            assert_eq!(capture_db(&mut p_after), expected_snap);
            // Finish checkpoint cleanly afterwards.
            assert!(p_after.checkpoint().unwrap());
            let (m_done, l_done) = p_after.into_storage();
            let mut p_done = Pager::open(m_done, l_done).unwrap();
            assert_eq!(capture_db(&mut p_done), expected_snap);
        }
    }
}

#[test]
fn log_truncation_and_byte_flips_always_recover_earlier_commit() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    let mut history = Vec::new();
    {
        let (m, l) = p.into_storage();
        let mut pt = Pager::open(m, l).unwrap();
        history.push(capture_db(&mut pt));
        let (m, l) = pt.into_storage();
        p = Pager::open(m, l).unwrap();
    }

    for i in 1..=5u64 {
        p.begin().unwrap();
        let pg = p.alloc().unwrap();
        let mut buf = [0u8; PAGE];
        buf[0..8].copy_from_slice(&i.to_le_bytes());
        p.write(pg, &buf).unwrap();
        p.set_root(pg).unwrap();
        p.commit().unwrap();
        let (m, l) = p.into_storage();
        let mut pt = Pager::open(m, l).unwrap();
        history.push(capture_db(&mut pt));
        let (m, l) = pt.into_storage();
        p = Pager::open(m, l).unwrap();
    }

    let (main, log) = p.into_storage();
    let log_bytes = log.bytes().to_vec();

    // Truncate at various offsets.
    for cut in (0..log_bytes.len()).step_by(500) {
        let truncated = log_bytes[..cut].to_vec();
        let mut pt = Pager::open(main.clone(), MemStorage::from_bytes(truncated)).unwrap();
        let got = capture_db(&mut pt);
        assert!(
            history.contains(&got),
            "truncation at {cut} gave invalid state"
        );
    }

    // Flip bytes at random positions in the log.
    forall(100, 0xBEEF, |rng| {
        let mut corrupted = log_bytes.clone();
        let idx = rng.below(corrupted.len());
        corrupted[idx] ^= 0xFF;
        let mut pt = Pager::open(main.clone(), MemStorage::from_bytes(corrupted)).unwrap();
        let got = capture_db(&mut pt);
        assert!(history.contains(&got));
    });
}
