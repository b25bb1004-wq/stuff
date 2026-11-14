#![allow(clippy::unwrap_used)]
//! fsck tests: each planted fault is found, the fixable ones are repaired, a clean image stays
//! clean, a committed journal is replayed, and no check panics on damaged images.
use alloc::vec::Vec;

use assay::{forall, Rng};

use crate::journal;
use crate::layout::{self, Geo, Inode, KIND_FREE, ROOT};
use crate::tests_prop::{apply, gen_op, Model, NAMES};
use crate::{fsck, mkfs, CrashDisk, Error, Fs, MemDisk, MkfsOptions, BLOCK};

fn blank(blocks: u64, inodes: u32, journal_blocks: u32) -> MemDisk {
    let mut d = MemDisk::new(blocks);
    mkfs(
        &mut d,
        MkfsOptions {
            inodes,
            journal_blocks,
        },
    )
    .unwrap();
    d
}

/// A small file system with two files, a directory and a file in it.
fn populated() -> MemDisk {
    let mut fs = Fs::mount(blank(256, 32, 8)).unwrap();
    fs.create("/a").unwrap();
    fs.write("/a", 0, &[1u8; 4096]).unwrap();
    fs.create("/b").unwrap();
    fs.write("/b", 0, &[2u8; 4096]).unwrap();
    fs.mkdir("/d").unwrap();
    fs.create("/d/x").unwrap();
    fs.write("/d/x", 0, &[3u8; 10_000]).unwrap();
    fs.unmount().unwrap()
}

/// Inode numbers of `/a` and `/b` in `populated` (the root is inode 1).
const A: u32 = 2;
const B: u32 = 3;

fn geo_of(d: &MemDisk) -> Geo {
    layout::decode_sb(&d.bytes()[..BLOCK]).unwrap().0
}

fn inode_offset(g: &Geo, n: u32) -> usize {
    (g.itable_start as usize + (n / 32) as usize) * BLOCK + (n % 32) as usize * layout::INODE_SIZE
}

fn inode_at(d: &MemDisk, n: u32) -> Inode {
    let g = geo_of(d);
    let at = inode_offset(&g, n);
    layout::decode_inode(&d.bytes()[at..at + layout::INODE_SIZE]).unwrap()
}

fn put_inode(d: &mut MemDisk, n: u32, ino: &Inode) {
    let g = geo_of(d);
    let at = inode_offset(&g, n);
    layout::encode_inode(ino, &mut d.bytes_mut()[at..at + layout::INODE_SIZE]).unwrap();
}

/// First data block of inode `n`.
fn first_block(d: &MemDisk, n: u32) -> u64 {
    inode_at(d, n).extents[0].start
}

fn set_bitmap(d: &mut MemDisk, b: u64, used: bool) {
    let g = geo_of(d);
    let at = g.bitmap_start as usize * BLOCK + (b / 8) as usize;
    let mask = 1u8 << (b % 8) as u32;
    let byte = &mut d.bytes_mut()[at];
    if used {
        *byte |= mask;
    } else {
        *byte &= !mask;
    }
}

fn check(d: &mut MemDisk, repair: bool) -> crate::Report {
    fsck(d, repair).unwrap()
}

#[test]
fn clean_image_has_no_errors_and_fsck_does_not_write() {
    let mut d = populated();
    let before = d.bytes().to_vec();
    let rep = check(&mut d, false);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!(rep.fixed, 0);
    assert!(d.bytes() == before.as_slice());
}

#[test]
fn double_owned_block_is_found_and_not_fixable() {
    let mut d = populated();
    let mut b = inode_at(&d, B);
    let a_start = first_block(&d, A);
    b.extents[0].start = a_start;
    put_inode(&mut d, B, &b);
    let rep = check(&mut d, false);
    assert!(rep.errors.iter().any(|e| e.contains("owned by inode")));
    let rep = check(&mut d, true);
    assert!(rep.fixed >= 1, "the block b lost should be freed");
    assert!(!rep.errors.is_empty());
    let rep = check(&mut d, false);
    assert!(rep.errors.iter().all(|e| e.contains("owned by inode")));
    assert!(!rep.errors.is_empty());
}

#[test]
fn cleared_bitmap_bit_is_found_and_repaired() {
    let mut d = populated();
    let b = first_block(&d, A);
    set_bitmap(&mut d, b, false);
    let rep = check(&mut d, false);
    assert!(rep.errors.iter().any(|e| e.contains("free in the bitmap")));
    let rep = check(&mut d, true);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!(rep.fixed, 1);
    assert!(check(&mut d, false).errors.is_empty());
}

#[test]
fn entry_to_free_inode_is_found_and_reported() {
    let mut d = populated();
    let mut a = inode_at(&d, A);
    a.kind = KIND_FREE;
    put_inode(&mut d, A, &a);
    let rep = check(&mut d, false);
    assert!(rep.errors.iter().any(|e| e.contains("free or bad inode")));
    let rep = check(&mut d, true);
    assert!(rep.errors.iter().any(|e| e.contains("free or bad inode")));
}

#[test]
fn wrong_link_count_is_found_and_reported() {
    let mut d = populated();
    let mut a = inode_at(&d, A);
    a.links = 3;
    put_inode(&mut d, A, &a);
    let rep = check(&mut d, false);
    assert!(rep
        .errors
        .iter()
        .any(|e| e.contains("inode 2: link count 3")));
    let rep = check(&mut d, true);
    assert!(rep
        .errors
        .iter()
        .any(|e| e.contains("inode 2: link count 3")));
}

#[test]
fn orphan_inode_is_found_and_freed_by_repair() {
    let mut d = populated();
    // Blank the first root entry ("a") so inode 1 is no longer reachable.
    let root_block = first_block(&d, ROOT);
    d.bytes_mut()[root_block as usize * BLOCK..root_block as usize * BLOCK + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    let rep = check(&mut d, false);
    assert!(rep.errors.iter().any(|e| e.contains("orphan")));
    let rep = check(&mut d, true);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(rep.fixed >= 2, "inode and its blocks");
    assert!(check(&mut d, false).errors.is_empty());
}

#[test]
fn bad_superblock_is_corrupt() {
    let mut d = populated();
    d.bytes_mut()[9] ^= 0xFF;
    assert_eq!(fsck(&mut d, false).err(), Some(Error::Corrupt));
}

#[test]
fn committed_journal_is_reported_then_replayed_by_repair() {
    let d = blank(256, 32, 8);
    let g = geo_of(&d);
    let data = [0x5Au8; BLOCK];
    // Budget 2 keeps the logged copy and the commit record, and drops the write home.
    let mut cut = CrashDisk::new(d, 2);
    journal::commit(&mut cut, &g, &[], &[(g.first_data, &data)]).unwrap();
    let mut d = cut.into_inner();
    let rep = check(&mut d, false);
    assert!(rep
        .errors
        .iter()
        .any(|e| e.contains("committed transaction")));
    let rep = check(&mut d, true);
    assert_eq!(rep.fixed, 1);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    let at = g.first_data as usize * BLOCK;
    assert!(d.bytes()[at..at + BLOCK] == data[..]);
    assert!(check(&mut d, false).errors.is_empty());
}

#[test]
fn torn_journal_record_is_reported_then_dropped_by_repair() {
    let mut d = blank(256, 32, 8);
    let g = geo_of(&d);
    let at = g.journal_start as usize * BLOCK;
    d.bytes_mut()[at..at + 8].copy_from_slice(b"SHJRNL02");
    let rep = check(&mut d, false);
    assert!(rep.errors.iter().any(|e| e.contains("torn")));
    let rep = check(&mut d, true);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(check(&mut d, false).errors.is_empty());
}

#[test]
fn write_over_journal_capacity_is_refused_before_anything_is_written() {
    // 8 journal blocks hold 7 logged blocks. Overwriting 10 existing blocks needs more than that.
    let mut fs = Fs::mount(blank(512, 32, 8)).unwrap();
    fs.create("/f").unwrap();
    fs.write("/f", 0, &[1u8; 40_960]).unwrap();
    let d = fs.unmount().unwrap();
    let snap = d.bytes().to_vec();
    let mut fs = Fs::mount(d).unwrap();
    assert_eq!(fs.write("/f", 0, &[2u8; 40_960]), Err(Error::TooLarge));
    let d = fs.unmount().unwrap();
    assert!(d.bytes() == snap.as_slice());
    // The same bytes written to blocks that are free (not yet in use) need no journal room.
    let mut fs = Fs::mount(d).unwrap();
    fs.create("/g").unwrap();
    assert_eq!(fs.write("/g", 0, &[3u8; 40_960]), Ok(40_960));
}

#[test]
fn random_operations_leave_a_clean_image() {
    forall(20, 0xF5C4_0007, |r| {
        let mut fs = Fs::mount(blank(512, 0, 16)).unwrap();
        let mut m = Model::new();
        for _ in 0..300 {
            let op = gen_op(r, &NAMES);
            apply(&mut fs, &mut m, &op);
        }
        let d = fs.unmount().unwrap();
        let mut d = d;
        let rep = check(&mut d, false);
        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    });
}

#[test]
fn damaged_images_never_panic_and_read_only_fsck_writes_nothing() {
    let image = populated().bytes().to_vec();
    forall(300, 0xDA7A_0007, |r: &mut Rng| {
        let mut d = MemDisk::new(256);
        d.bytes_mut().copy_from_slice(&image);
        for _ in 0..r.range(1, 12) {
            let blk = match r.below(4) {
                0 => 0,
                1 => 1 + r.range(0, 8),
                2 => 2 + r.range(0, 8),
                _ => r.range(0, 256),
            };
            let at = blk as usize * BLOCK + r.below(BLOCK);
            let flip = 1 + r.below(255) as u8;
            if let Some(b) = d.bytes_mut().get_mut(at) {
                *b ^= flip;
            }
        }
        let before: Vec<u8> = d.bytes().to_vec();
        if let Ok(rep) = fsck(&mut d, false) {
            assert_eq!(rep.fixed, 0);
        }
        assert!(d.bytes() == before.as_slice(), "read-only fsck wrote");
        let _ = fsck(&mut d, true);
        let _ = Fs::mount(d);
    });
}
