//! Leader-run crash check for the Shale journal (public API only, own crash device, fresh seed).
//! Each case runs a random operation sequence with an unmount and remount after every operation,
//! counting block writes. Then it cuts the power after every possible write: the remounted tree
//! must be the state after the last finished operation or after the one in progress, and `fsck`
//! must report nothing. `STRATA_SEED` and `STRATA_CASES` change the run.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use shale::{fsck, mkfs, BlockDevice, Error, Fs, Kind, MemDisk, MkfsOptions, BLOCK};
use std::cell::RefCell;
use std::rc::Rc;

const BLOCKS: u64 = 1024;
type Tree = Vec<(String, Option<Vec<u8>>)>;

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A device over a shared disk that silently drops every write after `budget` writes.
struct Cut {
    disk: Rc<RefCell<MemDisk>>,
    writes: Rc<RefCell<u64>>,
    budget: u64,
}

impl BlockDevice for Cut {
    fn block_count(&self) -> u64 {
        self.disk.borrow().block_count()
    }
    fn read(&mut self, block: u64, buf: &mut [u8; BLOCK]) -> Result<(), Error> {
        self.disk.borrow_mut().read(block, buf)
    }
    fn write(&mut self, block: u64, buf: &[u8; BLOCK]) -> Result<(), Error> {
        let mut w = self.writes.borrow_mut();
        *w += 1;
        if *w <= self.budget {
            self.disk.borrow_mut().write(block, buf)
        } else {
            Ok(())
        }
    }
    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

fn copy(d: &MemDisk) -> MemDisk {
    let mut c = MemDisk::new(BLOCKS);
    c.bytes_mut().copy_from_slice(d.bytes());
    c
}

fn dump<D: BlockDevice>(fs: &mut Fs<D>) -> Tree {
    let mut out = Vec::new();
    let mut todo = vec![String::new()];
    while let Some(dir) = todo.pop() {
        let list = fs.list(if dir.is_empty() { "/" } else { &dir }).unwrap();
        for e in list {
            let p = format!("{dir}/{}", e.name);
            match e.kind {
                Kind::Dir => {
                    out.push((p.clone(), None));
                    todo.push(p);
                }
                Kind::File => {
                    let size = fs.stat(&p).unwrap().size as usize;
                    let mut buf = vec![0; size];
                    let n = fs.read(&p, 0, &mut buf).unwrap();
                    assert_eq!(n, size, "short read of {p}");
                    out.push((p, Some(buf)));
                }
            }
        }
    }
    out.sort();
    out
}

#[derive(Clone, Debug)]
enum OpK {
    Create(String),
    Mkdir(String),
    Write(String, u64, Vec<u8>),
    Truncate(String, u64),
    Remove(String),
    Rmdir(String),
    Rename(String, String),
}

fn path(r: &mut Rng) -> String {
    const DIRS: [&str; 3] = ["", "/a", "/a/b"];
    const NAMES: [&str; 4] = ["a", "b", "f", "g"];
    format!("{}/{}", r.pick(&DIRS), r.pick(&NAMES))
}

fn random_op(r: &mut Rng) -> OpK {
    match r.below(9) {
        0 => OpK::Mkdir(path(r)),
        1 | 2 => OpK::Create(path(r)),
        3..=5 => {
            let mut d = r.bytes(30_000);
            d.push(7);
            OpK::Write(path(r), r.below(20_000) as u64, d)
        }
        6 => OpK::Truncate(path(r), r.below(40_000) as u64),
        7 => {
            if r.chance(3, 4) {
                OpK::Remove(path(r))
            } else {
                OpK::Rmdir(path(r))
            }
        }
        _ => OpK::Rename(path(r), path(r)),
    }
}

fn apply<D: BlockDevice>(fs: &mut Fs<D>, op: &OpK) {
    let _ = match op {
        OpK::Create(p) => fs.create(p),
        OpK::Mkdir(p) => fs.mkdir(p),
        OpK::Write(p, o, d) => fs.write(p, *o, d).map(|_| ()),
        OpK::Truncate(p, s) => fs.truncate(p, *s),
        OpK::Remove(p) => fs.remove(p),
        OpK::Rmdir(p) => fs.rmdir(p),
        OpK::Rename(a, b) => fs.rename(a, b),
    };
}

/// Run all ops on `base` with a write budget; returns the disk, the write count after each op
/// (unmount included) and, when `snap` is set, the tree after each op.
fn drive(base: &MemDisk, ops: &[OpK], budget: u64, snap: bool) -> (MemDisk, Vec<u64>, Vec<Tree>) {
    let disk = Rc::new(RefCell::new(copy(base)));
    let writes = Rc::new(RefCell::new(0u64));
    let dev = || Cut {
        disk: disk.clone(),
        writes: writes.clone(),
        budget,
    };
    let (mut done, mut snaps) = (Vec::new(), Vec::new());
    'ops: {
        let Ok(mut fs) = Fs::mount(dev()) else {
            break 'ops;
        };
        if snap {
            snaps.push(dump(&mut fs));
        }
        for op in ops {
            apply(&mut fs, op);
            if snap {
                snaps.push(dump(&mut fs));
            }
            if fs.unmount().is_err() {
                break 'ops;
            }
            done.push(*writes.borrow());
            fs = match Fs::mount(dev()) {
                Ok(f) => f,
                Err(_) => break 'ops,
            };
        }
    }
    let d = copy(&disk.borrow());
    (d, done, snaps)
}

fn one_case(r: &mut Rng, n_ops: usize) {
    let mut base = MemDisk::new(BLOCKS);
    mkfs(&mut base, MkfsOptions::default()).unwrap();
    let mut fs = Fs::mount(base).unwrap();
    for d in ["/a", "/a/b"] {
        fs.mkdir(d).unwrap();
    }
    for f in ["/f", "/a/f", "/a/b/f", "/g"] {
        fs.create(f).unwrap();
        fs.write(f, 0, &r.bytes(9000)).unwrap();
    }
    let base = fs.unmount().unwrap();
    let ops: Vec<OpK> = (0..n_ops).map(|_| random_op(r)).collect();
    let (_, done, snaps) = drive(&base, &ops, u64::MAX, true);
    assert_eq!(done.len(), ops.len(), "reference run failed to remount");
    let total = *done.last().unwrap();
    let changed = snaps.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(
        changed >= 5,
        "weak case: {changed} of {} ops changed the tree",
        ops.len()
    );
    for budget in 0..=total {
        let (disk, _, _) = drive(&base, &ops, budget, false);
        let k = done.iter().take_while(|w| **w <= budget).count();
        let mut fs = Fs::mount(disk)
            .unwrap_or_else(|e| panic!("mount after a cut at write {budget}: {e:?}"));
        let got = dump(&mut fs);
        let ok = got == snaps[k] || (k + 1 < snaps.len() && got == snaps[k + 1]);
        assert!(
            ok,
            "cut at write {budget}: tree is neither state {k} nor {}",
            k + 1
        );
        let mut disk = fs.unmount().unwrap();
        let report = fsck(&mut disk, false).unwrap();
        assert!(
            report.errors.is_empty(),
            "fsck after a cut at {budget}: {:?}",
            report.errors
        );
    }
}

#[test]
fn leader_crash_at_every_write() {
    let seed = env("STRATA_SEED", 0x5a1e_2026_1114);
    let cases = env("STRATA_CASES", 12) as u32;
    forall(cases, seed, |r| one_case(r, 40));
}
