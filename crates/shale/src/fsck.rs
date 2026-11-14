//! Consistency check: superblock, journal, inodes, the directory tree, link counts and the block
//! bitmap. Read-only unless `repair` is set; repairs are made in one journaled commit.
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::journal::{self, Peek};
use crate::layout::{self, Geo, Inode, BLOCK64, INODE_SIZE, KIND_DIR, KIND_FREE, ROOT};
use crate::ops;
use crate::txn::Txn;
use crate::{Error, BLOCK};

/// What a check found: the problems it could not repair, and how many repairs it made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Problems left in place, in the order they were found.
    pub errors: Vec<String>,
    /// Repairs made. Always 0 without `repair`.
    pub fixed: usize,
}

/// Check the file system on `dev`. Corrupt if the superblock is unreadable; every other problem
/// goes into the report. With `repair`, a pending journal transaction is replayed, orphan inodes
/// are freed and the bitmap is made to match the tree. Nothing is freed while a bad inode or
/// directory could hide live blocks; those problems are only reported.
pub fn fsck<D: BlockDevice>(dev: &mut D, repair: bool) -> Result<Report, Error> {
    let geo = crate::fs::read_geo(dev)?;
    let mut rep = Report {
        errors: Vec::new(),
        fixed: 0,
    };
    match journal::peek(dev, &geo)? {
        Peek::Clean => {}
        Peek::Committed(_) if !repair => rep.errors.push(String::from(
            "journal holds a committed transaction that is not replayed",
        )),
        Peek::Invalid if !repair => rep
            .errors
            .push(String::from("journal holds a torn commit record")),
        _ => {
            journal::recover(dev, &geo)?;
            rep.fixed += 1;
        }
    }
    check(dev, geo, repair, &mut rep)?;
    Ok(rep)
}

/// Every inode of the table, decoded. Free inodes and unreadable ones are None; the indirect
/// extents of a used inode are appended to its direct ones.
fn read_inodes<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    geo: &Geo,
    rep: &mut Report,
    bad: &mut bool,
) -> Result<Vec<Option<Inode>>, Error> {
    let count = usize::try_from(geo.inodes).map_err(|_| Error::TooLarge)?;
    let mut live: Vec<Option<Inode>> = (0..count).map(|_| None).collect();
    for n in 1..geo.inodes {
        let (blk, off) = t.inode_pos(n)?;
        let decoded = {
            let page = t.get(blk)?;
            layout::decode_inode(layout::sub(page, off, INODE_SIZE)?)
        };
        let mut ino = match decoded {
            Ok(i) => i,
            Err(_) => {
                rep.errors.push(format!("inode {n}: bad checksum or kind"));
                *bad = true;
                continue;
            }
        };
        if ino.kind == KIND_FREE {
            continue;
        }
        if ino.indirect != 0 {
            let ib = u64::from(ino.indirect);
            if geo.in_data(ib) {
                let page = t.get(ib)?;
                match layout::decode_indirect(page) {
                    Ok(more) => ino.extents.extend(more),
                    Err(_) => {
                        rep.errors
                            .push(format!("inode {n}: bad indirect block {ib}"));
                        *bad = true;
                    }
                }
            } else {
                rep.errors.push(format!(
                    "inode {n}: indirect block {ib} outside the data area"
                ));
                *bad = true;
            }
        }
        let mut sum = 0u64;
        for e in &ino.extents {
            sum = sum.saturating_add(e.len);
            if !geo.valid(e) {
                rep.errors.push(format!(
                    "inode {n}: extent at block {} of {} blocks is outside the data area",
                    e.start, e.len
                ));
                *bad = true;
            }
        }
        if sum != ino.size.div_ceil(BLOCK64) {
            rep.errors.push(format!(
                "inode {n}: size {} does not match its {sum} extent blocks",
                ino.size
            ));
        }
        if let Some(slot) = live.get_mut(n as usize) {
            *slot = Some(ino);
        }
    }
    Ok(live)
}

/// The bytes of a directory or file: its valid extents, up to its size.
fn read_bytes<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    geo: &Geo,
    ino: &Inode,
) -> Result<Vec<u8>, Error> {
    let valid = || ino.extents.iter().filter(|e| geo.valid(e));
    let have = valid().fold(0u64, |acc, e| {
        acc.saturating_add(e.len.saturating_mul(BLOCK64))
    });
    let len = usize::try_from(ino.size.min(have)).map_err(|_| Error::TooLarge)?;
    let mut out = Vec::with_capacity(len);
    'extents: for e in valid() {
        for k in 0..e.len {
            if out.len() >= len {
                break 'extents;
            }
            let page = t.get(e.start + k)?;
            let take = (len - out.len()).min(BLOCK);
            out.extend_from_slice(&page[..take]);
        }
    }
    Ok(out)
}

/// Walk the tree from the root. Marks every reachable inode in `seen`, counts the entries that
/// refer to each inode in `refs`, and reports entries that point at free or bad inodes.
fn walk<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    geo: &Geo,
    live: &[Option<Inode>],
    seen: &mut [bool],
    refs: &mut [u32],
    rep: &mut Report,
    corrupt_dir: &mut bool,
) -> Result<(), Error> {
    let root = ROOT as usize;
    match live.get(root) {
        Some(Some(r)) if r.kind == KIND_DIR => {}
        _ => {
            rep.errors.push(String::from("root is not a directory"));
            *corrupt_dir = true;
            return Ok(());
        }
    }
    if let Some(s) = seen.get_mut(root) {
        *s = true;
    }
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        let Some(Some(dir)) = live.get(d) else {
            continue;
        };
        let bytes = read_bytes(t, geo, dir)?;
        let recs = match ops::parse_dir(&bytes) {
            Ok(r) => r,
            Err(_) => {
                rep.errors
                    .push(format!("directory {d}: corrupt entry table"));
                *corrupt_dir = true;
                continue;
            }
        };
        for r in recs.iter().filter(|r| r.ino != 0) {
            let c = r.ino as usize;
            match live.get(c) {
                Some(Some(child)) => {
                    if let Some(k) = refs.get_mut(c) {
                        *k = k.saturating_add(1);
                    }
                    if let Some(s) = seen.get_mut(c) {
                        if !*s {
                            *s = true;
                            if child.kind == KIND_DIR {
                                stack.push(c);
                            }
                        }
                    }
                }
                _ => rep.errors.push(format!(
                    "entry {} in directory {d} points to free or bad inode {}",
                    String::from_utf8_lossy(r.name),
                    r.ino
                )),
            }
        }
    }
    Ok(())
}

/// Owner inode of every block, from the reachable inodes. A block claimed twice is reported.
fn owners(
    geo: &Geo,
    live: &[Option<Inode>],
    seen: &[bool],
    rep: &mut Report,
) -> Result<Vec<u32>, Error> {
    let blocks = usize::try_from(geo.blocks).map_err(|_| Error::TooLarge)?;
    let mut owner = vec![0u32; blocks];
    for (n, slot) in live.iter().enumerate() {
        if !seen.get(n).copied().unwrap_or(false) {
            continue;
        }
        let Some(node) = slot else {
            continue;
        };
        let me = n as u32;
        let direct = node
            .extents
            .iter()
            .filter(|e| geo.valid(e))
            .flat_map(|e| e.start..e.start + e.len);
        let indirect = (node.indirect != 0).then_some(u64::from(node.indirect));
        for b in direct.chain(indirect) {
            if !geo.in_data(b) {
                continue;
            }
            let Some(cell) = usize::try_from(b).ok().and_then(|i| owner.get_mut(i)) else {
                continue;
            };
            if *cell == 0 {
                *cell = me;
            } else {
                rep.errors.push(format!(
                    "block {b} is owned by inode {} and inode {me}",
                    *cell
                ));
            }
        }
    }
    Ok(owner)
}

/// The inode that owns block `b` (0 for none).
fn owner_of(owner: &[u32], b: u64) -> u32 {
    usize::try_from(b)
        .ok()
        .and_then(|i| owner.get(i))
        .copied()
        .unwrap_or(0)
}

/// Run every check on the device, and repair in one commit when asked.
fn check<D: BlockDevice>(
    dev: &mut D,
    geo: Geo,
    repair: bool,
    rep: &mut Report,
) -> Result<(), Error> {
    let mut t = Txn::new(dev, geo);
    let mut bad = false;
    let live = read_inodes(&mut t, &geo, rep, &mut bad)?;
    let n = live.len();
    let mut seen = vec![false; n];
    let mut refs = vec![0u32; n];
    let mut corrupt_dir = false;
    walk(
        &mut t,
        &geo,
        &live,
        &mut seen,
        &mut refs,
        rep,
        &mut corrupt_dir,
    )?;
    let trusted = !bad && !corrupt_dir;
    let owner = owners(&geo, &live, &seen, rep)?;
    let mut fixed = 0usize;

    for (ino, slot) in live.iter().enumerate().skip(1) {
        let Some(node) = slot else {
            continue;
        };
        if !seen.get(ino).copied().unwrap_or(false) {
            if repair && trusted {
                t.write_inode(ino as u32, &mut Inode::new(KIND_FREE))?;
                fixed += 1;
            } else {
                let note = if trusted {
                    ""
                } else {
                    " (not freed: the tree is damaged)"
                };
                rep.errors.push(format!(
                    "inode {ino} is an orphan, not reachable from the root{note}"
                ));
            }
            continue;
        }
        let expect = if ino == ROOT as usize {
            1
        } else {
            refs.get(ino).copied().unwrap_or(0)
        };
        if node.links != expect {
            rep.errors.push(format!(
                "inode {ino}: link count {} but {expect} entries refer to it",
                node.links
            ));
        }
    }

    for b in 0..geo.blocks {
        let expect = b < geo.first_data || owner_of(&owner, b) != 0;
        if t.is_used(b)? == expect {
            continue;
        }
        if expect {
            if repair {
                t.set_used(b, true)?;
                fixed += 1;
            } else {
                rep.errors.push(format!(
                    "block {b} is owned by an inode but free in the bitmap"
                ));
            }
        } else if trusted && repair {
            t.set_used(b, false)?;
            fixed += 1;
        } else {
            let note = if trusted {
                ""
            } else {
                " (not freed: the tree is damaged)"
            };
            rep.errors.push(format!(
                "block {b} is marked used in the bitmap but owned by no inode{note}"
            ));
        }
    }

    rep.fixed += fixed;
    if repair {
        t.commit()?;
    }
    Ok(())
}
