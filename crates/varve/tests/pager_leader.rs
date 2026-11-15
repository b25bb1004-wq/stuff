//! Leader check for T39 (varve pager, log, recovery). Copied to crates/varve/tests/pager_leader.rs.
//! Its own disk keeps unsynced writes pending and, at a crash, keeps or loses each 512-byte sector
//! of them independently (docs/VARVE.md section 1). `set_len` here also makes earlier writes
//! durable (more lenient than the doc; the pager only calls it on a broken log header).
#![allow(clippy::unwrap_used)]
use assay::Rng;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use varve::{Error, FileStorage, MemStorage, Page, Pager, Storage, PAGE, USABLE};

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn put(v: &mut Vec<u8>, off: usize, d: &[u8]) {
    if v.len() < off + d.len() {
        v.resize(off + d.len(), 0);
    }
    v[off..off + d.len()].copy_from_slice(d);
}

struct Pend {
    off: usize,
    new: Vec<u8>,
    old: Vec<u8>,
    prev: usize,
}
#[derive(Default)]
struct File {
    cur: Vec<u8>,
    pending: Vec<Pend>,
}
#[derive(Default)]
struct Shared {
    files: [File; 2],
    ops: u64,
    budget: u64,
    dead: bool,
}
#[derive(Clone)]
struct Disk {
    s: Rc<RefCell<Shared>>,
    f: usize,
}
type Sh = Rc<RefCell<Shared>>;

fn disks_from(main: Vec<u8>, log: Vec<u8>) -> (Disk, Disk, Sh) {
    let s = Rc::new(RefCell::new(Shared {
        budget: u64::MAX,
        ..Shared::default()
    }));
    s.borrow_mut().files[0].cur = main;
    s.borrow_mut().files[1].cur = log;
    (Disk { s: s.clone(), f: 0 }, Disk { s: s.clone(), f: 1 }, s)
}
fn disks() -> (Disk, Disk, Sh) {
    disks_from(Vec::new(), Vec::new())
}

impl Disk {
    fn op(&self) -> Result<(), Error> {
        let mut s = self.s.borrow_mut();
        if s.dead {
            return Err(Error::Io);
        }
        s.ops += 1;
        if s.ops > s.budget {
            s.dead = true;
            return Err(Error::Io);
        }
        Ok(())
    }
}
impl Storage for Disk {
    fn len(&mut self) -> Result<u64, Error> {
        let s = self.s.borrow();
        if s.dead {
            return Err(Error::Io);
        }
        Ok(s.files[self.f].cur.len() as u64)
    }
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), Error> {
        let s = self.s.borrow();
        let c = &s.files[self.f].cur;
        let o = usize::try_from(off).map_err(|_| Error::Io)?;
        let end = o.checked_add(buf.len()).ok_or(Error::Io)?;
        if s.dead || end > c.len() {
            return Err(Error::Io);
        }
        buf.copy_from_slice(&c[o..end]);
        Ok(())
    }
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), Error> {
        self.op()?;
        let mut s = self.s.borrow_mut();
        let f = &mut s.files[self.f];
        let o = off as usize;
        let prev = f.cur.len();
        let old = f.cur[o.min(prev)..(o + data.len()).min(prev)].to_vec();
        put(&mut f.cur, o, data);
        f.pending.push(Pend {
            off: o,
            new: data.to_vec(),
            old,
            prev,
        });
        Ok(())
    }
    fn set_len(&mut self, n: u64) -> Result<(), Error> {
        self.op()?;
        let mut s = self.s.borrow_mut();
        let f = &mut s.files[self.f];
        f.pending.clear();
        f.cur.resize(n as usize, 0);
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        self.op()?;
        self.s.borrow_mut().files[self.f].pending.clear();
        Ok(())
    }
}

/// The file as it is after a crash. mode 0 loses every unsynced sector, 1 keeps all, else random.
fn image(f: &File, mode: u64) -> Vec<u8> {
    let mut v = f.cur.clone();
    for p in f.pending.iter().rev() {
        v[p.off..p.off + p.old.len()].copy_from_slice(&p.old);
        v.truncate(p.prev);
    }
    let mut r = Rng::new(0x39_5ec7_0001 ^ mode);
    for p in &f.pending {
        let mut i = 0;
        while i < p.new.len() {
            let abs = p.off + i;
            let end = ((abs / 512 + 1) * 512 - p.off).min(p.new.len());
            let keep = match mode {
                0 => false,
                1 => true,
                _ => r.below(2) == 1,
            };
            if keep {
                put(&mut v, abs, &p.new[i..end]);
            }
            i = end;
        }
    }
    v
}
fn crash_images(sh: &Sh, mode: u64) -> (Vec<u8>, Vec<u8>) {
    let s = sh.borrow();
    (image(&s.files[0], mode), image(&s.files[1], mode))
}

#[derive(Clone, Debug, PartialEq)]
struct State {
    count: u64,
    root: u64,
    free: Vec<u64>,
    pages: BTreeMap<u64, Vec<u8>>,
}
impl State {
    fn new() -> State {
        State {
            count: 1,
            root: 0,
            free: Vec::new(),
            pages: BTreeMap::new(),
        }
    }
    fn alloc(&mut self) -> u64 {
        self.free.pop().unwrap_or_else(|| {
            self.count += 1;
            self.count - 1
        })
    }
}
enum Op {
    Alloc(u64, Vec<u8>),
    Write(u64, Vec<u8>),
    Free(u64),
    Root(u64),
}
enum Step {
    Txn(Vec<Op>, bool),
    Checkpoint,
}

fn page_of(c: &[u8]) -> Page {
    let mut p = [0u8; PAGE];
    p[..USABLE].copy_from_slice(c);
    p[USABLE..].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    p
}

/// Steps and the committed state after each one.
fn workload(r: &mut Rng, n: usize) -> (Vec<Step>, Vec<State>) {
    let (mut s, mut steps, mut states) = (State::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        if r.below(6) == 0 {
            steps.push(Step::Checkpoint);
        } else {
            let mut t = s.clone();
            let mut ops = Vec::new();
            for _ in 0..1 + r.below(4) {
                let live: Vec<u64> = t.pages.keys().copied().collect();
                if live.is_empty() || r.below(3) == 0 {
                    let p = t.alloc();
                    let mut c = vec![0u8; USABLE];
                    r.fill(&mut c);
                    t.pages.insert(p, c.clone());
                    ops.push(Op::Alloc(p, c));
                } else if r.below(4) == 0 {
                    let p = live[r.below(live.len())];
                    t.pages.remove(&p);
                    t.free.push(p);
                    ops.push(Op::Free(p));
                } else if r.below(6) == 0 {
                    let x = r.u64() % 1000;
                    t.root = x;
                    ops.push(Op::Root(x));
                } else {
                    let p = live[r.below(live.len())];
                    let mut c = vec![0u8; USABLE];
                    r.fill(&mut c);
                    t.pages.insert(p, c.clone());
                    ops.push(Op::Write(p, c));
                }
            }
            let commit = r.below(6) != 0;
            if commit {
                s = t;
            }
            steps.push(Step::Txn(ops, commit));
        }
        states.push(s.clone());
    }
    (steps, states)
}

fn apply<S: Storage>(p: &mut Pager<S>, st: &Step) -> Result<(), Error> {
    match st {
        Step::Checkpoint => {
            p.checkpoint()?;
        }
        Step::Txn(ops, commit) => {
            p.begin()?;
            for op in ops {
                match op {
                    Op::Alloc(n, c) => {
                        let got = p.alloc()?;
                        assert_eq!(got, *n, "alloc pops the free list (LIFO), else grows");
                        p.write(*n, &page_of(c))?;
                    }
                    Op::Write(n, c) => p.write(*n, &page_of(c))?,
                    Op::Free(n) => p.free(*n)?,
                    Op::Root(x) => p.set_root(*x)?,
                }
            }
            if *commit {
                p.commit()?;
            } else {
                p.rollback();
            }
        }
    }
    Ok(())
}

fn matches<S: Storage>(p: &mut Pager<S>, s: &State) -> bool {
    if p.page_count() != s.count || p.root() != s.root {
        return false;
    }
    let Ok(h) = p.read(0) else { return false };
    if u64_at(&h, 12) != s.count || u64_at(&h, 28) != s.root {
        return false;
    }
    for (n, c) in &s.pages {
        match p.read(*n) {
            Ok(pg) if pg[..USABLE] == c[..] && u32_at(&pg, USABLE) == crc32(c) => {}
            _ => return false,
        }
    }
    let mut at = u64_at(&h, 20);
    for &want in s.free.iter().rev() {
        if at != want {
            return false;
        }
        let Ok(pg) = p.read(at) else { return false };
        if pg[0] != 5 {
            return false;
        }
        at = u64_at(&pg, 16);
    }
    at == 0
}

fn check_state<S: Storage>(p: &mut Pager<S>, allowed: &[State], what: &str) {
    assert!(
        allowed.iter().any(|s| matches(p, s)),
        "{what}: state is none of the {} allowed ones",
        allowed.len()
    );
}

#[test]
fn leader_model_and_reopen() {
    let mut r = Rng::new(0x39_0de1_0001);
    let (steps, states) = workload(&mut r, 40);
    let (m, l, sh) = disks();
    let mut p = Pager::open(m, l).unwrap();
    for (i, st) in steps.iter().enumerate() {
        apply(&mut p, st).unwrap();
        check_state(&mut p, &states[i..=i], &format!("step {i}"));
        let (m, l) = p.into_storage();
        p = Pager::open(m, l).unwrap();
        check_state(&mut p, &states[i..=i], &format!("reopen after step {i}"));
    }
    let _ = p.close().unwrap();
    let (mi, li) = crash_images(&sh, 0);
    let (m, l, _) = disks_from(mi, li);
    let mut p = Pager::open(m, l).unwrap();
    check_state(&mut p, &states[states.len() - 1..], "after close");
    assert_eq!(p.log_frames(), 0, "close checkpoints");
}

#[test]
fn leader_crash_at_every_storage_operation() {
    for w in 0..3u64 {
        let mut r = Rng::new(0x39_c4a5_0001 + w);
        let (steps, states) = workload(&mut r, 14);
        let total = {
            let (m, l, sh) = disks();
            let mut p = Pager::open(m, l).unwrap();
            let base = sh.borrow().ops;
            for st in &steps {
                apply(&mut p, st).unwrap();
            }
            let _ = p.close().unwrap();
            let ops = sh.borrow().ops;
            ops - base
        };
        assert!(total > 20, "workload {w}: only {total} storage operations");
        for c in 0..total {
            let (m, l, sh) = disks();
            let mut p = Pager::open(m, l).unwrap();
            let start = sh.borrow().ops;
            sh.borrow_mut().budget = start + c;
            let mut allowed = vec![State::new()];
            let mut crashed = false;
            for (i, st) in steps.iter().enumerate() {
                if apply(&mut p, st).is_err() {
                    if matches!(st, Step::Txn(_, true)) {
                        allowed.push(states[i].clone());
                    }
                    crashed = true;
                    break;
                }
                allowed = vec![states[i].clone()];
            }
            if crashed {
                let _ = p.into_storage();
            } else {
                let _ = p.close();
            }
            for mode in [0, 1, 2 + c] {
                let (mi, li) = crash_images(&sh, mode);
                let (m2, l2, sh2) = disks_from(mi, li);
                let what = format!("workload {w}, crash at op {c}, loss pattern {mode}");
                let mut p2 = Pager::open(m2, l2).unwrap_or_else(|e| panic!("{what}: reopen {e:?}"));
                check_state(&mut p2, &allowed, &what);
                // A second crash inside the recovery's checkpoint.
                if mode > 1 {
                    let at = sh2.borrow().ops;
                    sh2.borrow_mut().budget = at + c % 5;
                    let _ = p2.checkpoint();
                    let _ = p2.into_storage();
                    let (mi, li) = crash_images(&sh2, mode + 1);
                    let (m3, l3, _) = disks_from(mi, li);
                    let mut p3 = Pager::open(m3, l3)
                        .unwrap_or_else(|e| panic!("{what}, second crash: reopen {e:?}"));
                    check_state(&mut p3, &allowed, &format!("{what}, second crash"));
                    p2 = p3;
                }
                // Still writable.
                p2.begin().unwrap();
                let n = p2.alloc().unwrap();
                p2.write(n, &page_of(&[7u8; USABLE])).unwrap();
                p2.commit().unwrap();
                assert_eq!(p2.read(n).unwrap()[..USABLE], [7u8; USABLE][..], "{what}");
            }
        }
    }
}

#[test]
fn leader_formats_byte_exact() {
    let (m, l, sh) = disks();
    let mut p = Pager::open(m, l).unwrap();
    let main0 = sh.borrow().files[0].cur.clone();
    assert_eq!(main0.len(), PAGE, "fresh main file is page 0");
    assert_eq!(&main0[..8], b"VARVE\0v1");
    assert_eq!(u32_at(&main0, 8), 4096);
    assert_eq!(
        (
            u64_at(&main0, 12),
            u64_at(&main0, 20),
            u64_at(&main0, 28),
            u64_at(&main0, 36)
        ),
        (1, 0, 0, 0)
    );
    assert!(main0[44..USABLE].iter().all(|&b| b == 0));
    assert_eq!(u32_at(&main0, USABLE), crc32(&main0[..USABLE]));
    let log0 = sh.borrow().files[1].cur.clone();
    assert_eq!(log0.len(), 32, "fresh log is a header");
    assert_eq!(&log0[..8], b"VARVEWAL");
    assert_eq!(
        (u32_at(&log0, 8), u32_at(&log0, 12), u64_at(&log0, 16)),
        (1, 4096, 1)
    );
    assert_eq!(u32_at(&log0, 24), 0);
    assert_eq!(u32_at(&log0, 28), crc32(&log0[..28]));

    let mut a = vec![0u8; USABLE];
    Rng::new(0x39_f0a7_0001).fill(&mut a);
    p.begin().unwrap();
    assert_eq!(p.alloc().unwrap(), 1);
    p.write(1, &page_of(&a)).unwrap();
    p.commit().unwrap();
    assert_eq!(
        sh.borrow().files[0].cur,
        main0,
        "a commit never touches the main file"
    );
    let log = sh.borrow().files[1].cur.clone();
    assert_eq!(
        log.len(),
        32 + 2 * (32 + PAGE),
        "two frames: page 0 and page 1"
    );
    let mut seen = Vec::new();
    for k in 0..2 {
        let f = &log[32 + k * (32 + PAGE)..32 + (k + 1) * (32 + PAGE)];
        let (pn, commit, salt) = (u64_at(f, 0), u64_at(f, 8), u64_at(f, 16));
        assert_eq!(salt, 1);
        assert_eq!(u32_at(f, 24), 0);
        let mut body = f[..28].to_vec();
        body.extend_from_slice(&f[32..]);
        assert_eq!(u32_at(f, 28), crc32(&body), "frame {k} CRC");
        assert_eq!(
            commit,
            if k == 1 { 2 } else { 0 },
            "commit field only on the last frame"
        );
        let img = &f[32..];
        assert_eq!(
            u32_at(img, USABLE),
            crc32(&img[..USABLE]),
            "page CRC in frame {k}"
        );
        if pn == 0 {
            assert_eq!(&img[..8], b"VARVE\0v1");
            assert_eq!(
                (u64_at(img, 12), u64_at(img, 20), u64_at(img, 36)),
                (2, 0, 1)
            );
        } else {
            assert_eq!(pn, 1);
            assert_eq!(&img[..USABLE], &a[..]);
        }
        seen.push(pn);
    }
    seen.sort_unstable();
    assert_eq!(seen, [0, 1]);
    assert_eq!(p.log_frames(), 2);

    // Frames after the last commit, or with another salt, are ignored.
    let z = vec![0x5a; USABLE];
    let mut extra = Vec::new();
    for (salt, commit) in [(1u64, 0u64), (9, 2)] {
        let mut f = vec![0u8; 32];
        f[..8].copy_from_slice(&1u64.to_le_bytes());
        f[8..16].copy_from_slice(&commit.to_le_bytes());
        f[16..24].copy_from_slice(&salt.to_le_bytes());
        let mut img = z.clone();
        img.extend_from_slice(&crc32(&z).to_le_bytes());
        let mut body = f[..28].to_vec();
        body.extend_from_slice(&img);
        let c = crc32(&body);
        f[28..32].copy_from_slice(&c.to_le_bytes());
        f.extend_from_slice(&img);
        extra.push(f);
    }
    for f in &extra {
        let mut l2 = log.clone();
        l2.extend_from_slice(f);
        let (m, l, _) = disks_from(main0.clone(), l2);
        let mut q = Pager::open(m, l).unwrap();
        assert_eq!(
            q.read(1).unwrap()[..USABLE],
            a[..],
            "uncommitted or foreign frame was used"
        );
    }

    // Checkpoint: main file gets the pages, the log gets salt 2.
    assert!(p.checkpoint().unwrap());
    assert_eq!(p.log_frames(), 0);
    let main = sh.borrow().files[0].cur.clone();
    assert_eq!(main.len(), 2 * PAGE);
    assert_eq!(&main[PAGE..PAGE + USABLE], &a[..]);
    assert_eq!(u32_at(&main, PAGE + USABLE), crc32(&a));
    assert_eq!((u64_at(&main, 12), u64_at(&main, 36)), (2, 1));
    let log = sh.borrow().files[1].cur.clone();
    assert_eq!((&log[..8], u64_at(&log, 16)), (&b"VARVEWAL"[..], 2));
    assert_eq!(u32_at(&log, 28), crc32(&log[..28]));
    p.begin().unwrap();
    p.write(1, &page_of(&z)).unwrap();
    p.commit().unwrap();
    let log = sh.borrow().files[1].cur.clone();
    assert_eq!(u64_at(&log, 32 + 16), 2, "new frames carry the new salt");
    assert_eq!(p.read(1).unwrap()[..USABLE], z[..]);
    // A broken log header after a checkpoint loses nothing that was checkpointed.
    let _ = p.into_storage();
    let mut l2 = sh.borrow().files[1].cur.clone();
    let main = sh.borrow().files[0].cur.clone();
    l2[3] ^= 1;
    let (m, l, _) = disks_from(main, l2);
    let mut q = Pager::open(m, l).unwrap();
    assert_eq!(q.read(1).unwrap()[..USABLE], a[..]);
}

#[test]
fn leader_free_list() {
    let (m, l, _) = disks();
    let mut p = Pager::open(m, l).unwrap();
    p.begin().unwrap();
    for want in 1..=3 {
        assert_eq!(p.alloc().unwrap(), want);
    }
    p.commit().unwrap();
    p.begin().unwrap();
    p.free(2).unwrap();
    p.free(3).unwrap();
    assert_eq!(p.free(3), Err(Error::Misuse), "double free");
    p.commit().unwrap();
    let h = p.read(0).unwrap();
    assert_eq!(u64_at(&h, 20), 3, "last freed is the head");
    let f3 = p.read(3).unwrap();
    assert_eq!((f3[0], u64_at(&f3, 16)), (5, 2));
    assert!(f3[1..16].iter().chain(&f3[24..USABLE]).all(|&b| b == 0));
    let (m, l) = p.into_storage();
    let mut p = Pager::open(m, l).unwrap();
    p.begin().unwrap();
    assert_eq!(
        (p.alloc().unwrap(), p.alloc().unwrap(), p.alloc().unwrap()),
        (3, 2, 4)
    );
    assert!(
        p.read(2).unwrap()[..USABLE].iter().all(|&b| b == 0),
        "allocated pages are zero"
    );
    assert_eq!(p.page_count(), 5);
    p.rollback();
    assert_eq!(p.page_count(), 4, "rollback restores the page count");
    assert_eq!(u64_at(&p.read(0).unwrap(), 20), 3, "and the free list");
}

#[test]
fn leader_misuse_errors() {
    let (m, l, _) = disks();
    let mut p = Pager::open(m, l).unwrap();
    let pg = [0u8; PAGE];
    assert_eq!(p.write(1, &pg), Err(Error::Misuse));
    assert_eq!(p.alloc(), Err(Error::Misuse));
    assert_eq!(p.free(1), Err(Error::Misuse));
    assert_eq!(p.set_root(1), Err(Error::Misuse));
    assert_eq!(p.commit(), Err(Error::Misuse));
    assert_eq!(p.read(1), Err(Error::Misuse));
    p.begin().unwrap();
    assert_eq!(p.begin(), Err(Error::Busy));
    assert_eq!(p.write(0, &pg), Err(Error::Misuse));
    assert_eq!(p.free(0), Err(Error::Misuse));
    assert_eq!(p.write(1, &pg), Err(Error::Misuse), "past the end");
    let n = p.alloc().unwrap();
    p.write(n, &pg).unwrap();
    p.commit().unwrap();
    let s = p.snapshot();
    p.release(s);
    assert_eq!(p.read_snapshot(s, 1), Err(Error::Misuse));
}

#[test]
fn leader_snapshots_and_checkpoints() {
    let (m, l, _) = disks();
    let mut p = Pager::open(m, l).unwrap();
    let (x, y) = (vec![1u8; USABLE], vec![2u8; USABLE]);
    p.begin().unwrap();
    let n = p.alloc().unwrap();
    p.write(n, &page_of(&x)).unwrap();
    p.commit().unwrap();
    let s = p.snapshot();
    p.begin().unwrap();
    p.write(n, &page_of(&y)).unwrap();
    p.commit().unwrap();
    assert_eq!(p.read_snapshot(s, n).unwrap()[..USABLE], x[..]);
    assert_eq!(p.read(n).unwrap()[..USABLE], y[..]);
    assert!(
        !p.checkpoint().unwrap(),
        "no checkpoint while a snapshot is held"
    );
    assert!(p.log_frames() > 0);
    assert_eq!(p.read_snapshot(s, n).unwrap()[..USABLE], x[..]);
    p.release(s);
    assert!(p.checkpoint().unwrap());
    assert_eq!(p.log_frames(), 0);
    assert_eq!(p.read(n).unwrap()[..USABLE], y[..]);
}

#[test]
fn leader_auto_checkpoint() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let n = p.alloc().unwrap();
    p.commit().unwrap();
    let (mut last, mut resets) = (0, 0);
    for i in 0..700u32 {
        let c = vec![(i % 251) as u8; USABLE];
        p.begin().unwrap();
        p.write(n, &page_of(&c)).unwrap();
        p.commit().unwrap();
        let f = p.log_frames();
        assert!(f <= 1000, "log holds {f} frames after commit {i}");
        if f < last {
            resets += 1;
        }
        last = f;
    }
    assert!(resets >= 1, "the log never checkpointed by itself");
    let (main, log) = p.into_storage();
    let mb = main.bytes();
    assert_eq!(u64_at(mb, 12), 2);
    assert_eq!(u32_at(&mb[PAGE..], USABLE), crc32(&mb[PAGE..PAGE + USABLE]));
    let mut p = Pager::open(main, log).unwrap();
    assert_eq!(
        p.read(n).unwrap()[..USABLE],
        [(699 % 251) as u8; USABLE][..]
    );
}

#[test]
fn leader_hostile_log_and_main() {
    let mut r = Rng::new(0x39_a77a_0001);
    // Commits only (no checkpoints): every damaged log must give some earlier commit's state.
    let (m, l, sh) = disks();
    let mut p = Pager::open(m, l).unwrap();
    let mut states = vec![State::new()];
    let mut s = State::new();
    for _ in 0..12 {
        let mut ops = Vec::new();
        for _ in 0..1 + r.below(3) {
            let live: Vec<u64> = s.pages.keys().copied().collect();
            if live.is_empty() || r.below(2) == 0 {
                let q = s.alloc();
                let mut c = vec![0u8; USABLE];
                r.fill(&mut c);
                s.pages.insert(q, c.clone());
                ops.push(Op::Alloc(q, c));
            } else {
                let q = live[r.below(live.len())];
                let mut c = vec![0u8; USABLE];
                r.fill(&mut c);
                s.pages.insert(q, c.clone());
                ops.push(Op::Write(q, c));
            }
        }
        apply(&mut p, &Step::Txn(ops, true)).unwrap();
        states.push(s.clone());
    }
    let _ = p.into_storage();
    let (main, log) = {
        let b = sh.borrow();
        (b.files[0].cur.clone(), b.files[1].cur.clone())
    };
    for case in 0..300 {
        let mut l2 = log.clone();
        if case % 3 == 0 {
            l2.truncate(r.below(log.len() + 1));
        } else {
            let at = r.below(l2.len());
            l2[at] ^= 1 << r.below(8);
        }
        let (m, l, _) = disks_from(main.clone(), l2);
        let mut q = Pager::open(m, l).unwrap_or_else(|e| panic!("damaged log case {case}: {e:?}"));
        check_state(&mut q, &states, &format!("damaged log case {case}"));
    }
    for case in 0..50 {
        let len = r.below(3 * (32 + PAGE));
        let junk = r.bytes(len);
        let (m, l, _) = disks_from(main.clone(), junk);
        let mut q = Pager::open(m, l).unwrap();
        check_state(&mut q, &states[..1], &format!("junk log case {case}"));
    }
    // Corrupt main pages after a full checkpoint.
    let (m, l, _) = disks_from(main.clone(), log.clone());
    let (m, l) = Pager::open(m, l).unwrap().close().unwrap();
    let main = m.s.borrow().files[0].cur.clone();
    drop(l);
    let last = states.last().unwrap();
    for (&pg, _) in last.pages.iter().take(5) {
        let mut m2 = main.clone();
        m2[pg as usize * PAGE + r.below(PAGE)] ^= 0x10;
        let (m, l, _) = disks_from(m2, Vec::new());
        let mut q = Pager::open(m, l).unwrap();
        assert_eq!(q.read(pg), Err(Error::Corrupt), "corrupt page {pg}");
    }
    let mut m2 = main.clone();
    m2[100] ^= 1;
    let (m, l, _) = disks_from(m2, Vec::new());
    assert_eq!(
        Pager::open(m, l).err(),
        Some(Error::Corrupt),
        "corrupt page 0"
    );
    for _ in 0..30 {
        let len = PAGE * (1 + r.below(3));
        let mut junk = vec![0u8; len];
        r.fill(&mut junk);
        let (m, l, _) = disks_from(junk, Vec::new());
        assert!(Pager::open(m, l).is_err(), "random main file opened");
    }
}

#[test]
fn leader_file_storage() {
    let dir = std::env::temp_dir().join(format!("varve-leader-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("db"), dir.join("db-wal"));
    let c = vec![0x42u8; USABLE];
    {
        let mut p = Pager::open(
            FileStorage::open(&a).unwrap(),
            FileStorage::open(&b).unwrap(),
        )
        .unwrap();
        p.begin().unwrap();
        let n = p.alloc().unwrap();
        p.write(n, &page_of(&c)).unwrap();
        p.set_root(n).unwrap();
        p.commit().unwrap();
        let _ = p.into_storage();
    }
    let mut p = Pager::open(
        FileStorage::open(&a).unwrap(),
        FileStorage::open(&b).unwrap(),
    )
    .unwrap();
    let n = p.root();
    assert_eq!(p.read(n).unwrap()[..USABLE], c[..]);
    let _ = p.close().unwrap();
    assert_eq!(std::fs::metadata(&a).unwrap().len(), 2 * PAGE as u64);
    std::fs::remove_dir_all(&dir).unwrap();
}
