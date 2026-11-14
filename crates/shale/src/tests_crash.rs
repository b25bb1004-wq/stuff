#![allow(clippy::unwrap_used)]
//! Crash injection: random operation sequences are cut at every device write. After each cut the
//! inner device is remounted; it must mount, pass fsck, and show the tree from just before or
//! just after the cut operation.
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use assay::forall;

use crate::tests_prop::{apply, gen_op, Model, Op, NAMES};
use crate::{fsck, mkfs, BlockDevice, CrashDisk, Fs, Kind, MemDisk, MkfsOptions};

const BLOCKS: u64 = 512;
const OPS: usize = 30;
const SEQUENCES: u32 = 100;

/// The visible tree: every path, with `None` for a directory and the bytes of a file.
type Tree = BTreeMap<String, Option<Vec<u8>>>;

fn key(c: &[String]) -> String {
    if c.is_empty() {
        String::from("/")
    } else {
        format!("/{}", c.join("/"))
    }
}

fn model_tree(m: &Model) -> Tree {
    let mut t = Tree::new();
    t.insert(String::from("/"), None);
    for d in &m.dirs {
        t.insert(key(d), None);
    }
    for (f, data) in &m.files {
        t.insert(key(f), Some(data.clone()));
    }
    t
}

fn fs_tree<D: BlockDevice>(fs: &mut Fs<D>) -> Option<Tree> {
    let mut out = Tree::new();
    out.insert(String::from("/"), None);
    let mut stack = vec![String::from("/")];
    while let Some(d) = stack.pop() {
        for e in fs.list(&d).ok()? {
            let p = if d == "/" {
                format!("/{}", e.name)
            } else {
                format!("{d}/{}", e.name)
            };
            match e.kind {
                Kind::Dir => {
                    out.insert(p.clone(), None);
                    stack.push(p);
                }
                Kind::File => {
                    let size = fs.stat(&p).ok()?.size;
                    let mut buf = vec![0u8; usize::try_from(size).ok()?];
                    let n = fs.read(&p, 0, &mut buf).ok()?;
                    buf.truncate(n);
                    out.insert(p, Some(buf));
                }
            }
        }
    }
    Some(out)
}

fn copy_of(d: &MemDisk) -> MemDisk {
    let mut c = MemDisk::new(d.block_count());
    c.bytes_mut().copy_from_slice(d.bytes());
    c
}

fn base_image() -> MemDisk {
    let mut d = MemDisk::new(BLOCKS);
    mkfs(
        &mut d,
        MkfsOptions {
            inodes: 0,
            journal_blocks: 64,
        },
    )
    .unwrap();
    d
}

/// Run `ops` with no cut, checking each result against the model `m` (which must describe `base`)
/// and unmounting between them. Returns the write count after each operation, and the model tree
/// after each prefix of `ops`.
fn count_writes(base: &MemDisk, mut m: Model, ops: &[Op]) -> (Vec<u64>, Vec<Tree>) {
    let mut cd = CrashDisk::new(copy_of(base), u64::MAX);
    let mut ends = Vec::new();
    let mut states = vec![model_tree(&m)];
    for op in ops {
        let mut fs = Fs::mount(cd).unwrap();
        apply(&mut fs, &mut m, op);
        cd = fs.unmount().unwrap();
        ends.push(cd.writes());
        states.push(model_tree(&m));
    }
    (ends, states)
}

/// Run one operation and ignore its result: after a cut, results do not matter, only the disk.
fn run_op<D: BlockDevice>(fs: &mut Fs<D>, op: &Op) {
    match op {
        Op::Make { path, dir: true } => {
            let _ = fs.mkdir(path);
        }
        Op::Make { path, dir: false } => {
            let _ = fs.create(path);
        }
        Op::Write { path, off, data } => {
            let _ = fs.write(path, *off, data);
        }
        Op::Read { path, off, len } => {
            let mut buf = vec![0u8; *len];
            let _ = fs.read(path, *off, &mut buf);
        }
        Op::Truncate { path, size } => {
            let _ = fs.truncate(path, *size);
        }
        Op::Remove { path } => {
            let _ = fs.remove(path);
        }
        Op::Rmdir { path } => {
            let _ = fs.rmdir(path);
        }
        Op::Rename { from, to } => {
            let _ = fs.rename(from, to);
        }
        Op::List { path } => {
            let _ = fs.list(path);
        }
        Op::Stat { path } => {
            let _ = fs.stat(path);
        }
    }
}

/// Run `ops` on a copy of `base` that keeps only its first `budget` writes. Returns the inner disk.
fn crash_run(base: &MemDisk, ops: &[Op], budget: u64) -> MemDisk {
    let mut fs = Fs::mount(CrashDisk::new(copy_of(base), budget)).unwrap();
    for op in ops {
        run_op(&mut fs, op);
    }
    fs.unmount().unwrap().into_inner()
}

#[test]
fn crash_at_every_write_leaves_before_or_after() {
    let base = base_image();
    forall(SEQUENCES, 0xC4A5_0007, |r| {
        let ops: Vec<Op> = (0..OPS).map(|_| gen_op(r, &NAMES)).collect();
        let (ends, states) = count_writes(&base, Model::new(), &ops);
        let total = ends.last().copied().unwrap_or(0);
        for budget in 0..=total {
            // Operation k is the one whose writes pass the budget: before it or after it.
            let k = ends.iter().position(|&e| e > budget).unwrap_or(OPS);
            let allowed: Vec<&Tree> = if k == OPS {
                vec![&states[OPS]]
            } else {
                vec![&states[k], &states[k + 1]]
            };
            let mut fs = Fs::mount(crash_run(&base, &ops, budget)).unwrap();
            let got = fs_tree(&mut fs);
            let mut dev = fs.unmount().unwrap();
            let rep = fsck(&mut dev, false).unwrap();
            assert!(
                rep.errors.is_empty(),
                "cut at write {budget} (operation {k}): fsck {:?}",
                rep.errors
            );
            assert!(
                allowed.iter().any(|t| got.as_ref() == Some(*t)),
                "cut at write {budget} (operation {k}): tree is neither before nor after"
            );
        }
    });
}

#[test]
fn cut_inside_one_overwrite_is_all_or_nothing() {
    let mut fs = Fs::mount(base_image()).unwrap();
    fs.create("/f").unwrap();
    fs.write("/f", 0, &[1u8; 9000]).unwrap();
    let base = fs.unmount().unwrap();
    // Overwriting bytes of existing blocks is logged through the journal, not written in place.
    let ops = [Op::Write {
        path: String::from("/f"),
        off: 2000,
        data: vec![7u8; 9000],
    }];
    let before = fs_tree(&mut Fs::mount(copy_of(&base)).unwrap()).unwrap();
    let after = fs_tree(&mut Fs::mount(crash_run(&base, &ops, u64::MAX)).unwrap()).unwrap();
    assert_ne!(before, after);
    let mut start = Model::new();
    start.make("/f", false).unwrap();
    start.write("/f", 0, &[1u8; 9000]).unwrap();
    let (ends, _) = count_writes(&base, start, &ops);
    assert!(ends[0] >= 4, "the operation should write several blocks");
    for budget in 0..=ends[0] {
        let mut fs = Fs::mount(crash_run(&base, &ops, budget)).unwrap();
        let got = fs_tree(&mut fs).unwrap();
        let mut dev = fs.unmount().unwrap();
        assert!(fsck(&mut dev, false).unwrap().errors.is_empty());
        assert!(
            got == before || got == after,
            "budget {budget}: neither before nor after"
        );
    }
}
