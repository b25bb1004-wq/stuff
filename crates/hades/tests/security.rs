#![allow(clippy::unwrap_used)]
//! Security review tests for hades: corrupted images, hostile journal records,
//! fsck on garbage, extent aliasing, directory integrity, and path handling.

use std::time::Instant;

use assay::Rng;
use hades::{fsck, mkfs, BlockDevice, Error, Fs, MemDisk, MkfsOptions, BLOCK};

const MAGIC_SB: &[u8] = b"SHALE002";
const MAGIC_JRNL: &[u8] = b"SHJRNL02";
const KIND_FREE: u32 = 0;
const KIND_FILE: u32 = 1;
const KIND_DIR: u32 = 2;
const ROOT: u32 = 1;
const INODE_SIZE: usize = 128;

fn crc32c(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0x82F6_3B78
            } else {
                c >> 1
            };
        }
    }
    !c
}

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

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

#[derive(Clone, Copy)]
struct GeoView {
    blocks: u64,
    journal_start: u64,
    bitmap_start: u64,
    itable_start: u64,
    first_data: u64,
}

fn sb_geo(d: &MemDisk) -> GeoView {
    let b = &d.bytes()[..BLOCK];
    GeoView {
        blocks: u64_at(b, 8),
        journal_start: u64_at(b, 24),
        bitmap_start: u64_at(b, 40),
        itable_start: u64_at(b, 56),
        first_data: u64_at(b, 72),
    }
}

fn seal_sb(d: &mut MemDisk) {
    let crc = crc32c(&d.bytes()[..BLOCK - 4]);
    d.bytes_mut()[BLOCK - 4..BLOCK].copy_from_slice(&crc.to_le_bytes());
}

fn inode_off(g: &GeoView, n: u32) -> usize {
    (g.itable_start as usize + (n / 32) as usize) * BLOCK + (n % 32) as usize * INODE_SIZE
}

fn write_raw_inode(
    d: &mut MemDisk,
    n: u32,
    kind: u32,
    links: u32,
    size: u64,
    direct: &[(u32, u32)],
    indirect: u32,
) {
    let g = sb_geo(d);
    let at = inode_off(&g, n);
    let slot = &mut d.bytes_mut()[at..at + INODE_SIZE];
    slot.fill(0);
    slot[0..4].copy_from_slice(&kind.to_le_bytes());
    slot[4..8].copy_from_slice(&links.to_le_bytes());
    slot[8..16].copy_from_slice(&size.to_le_bytes());
    for (i, &(start, len)) in direct.iter().take(6).enumerate() {
        slot[16 + 8 * i..20 + 8 * i].copy_from_slice(&start.to_le_bytes());
        slot[20 + 8 * i..24 + 8 * i].copy_from_slice(&len.to_le_bytes());
    }
    slot[64..68].copy_from_slice(&indirect.to_le_bytes());
    let crc = crc32c(&slot[..INODE_SIZE - 4]);
    slot[INODE_SIZE - 4..INODE_SIZE].copy_from_slice(&crc.to_le_bytes());
}

fn write_raw_indirect(d: &mut MemDisk, blk: u64, extents: &[(u32, u32)]) {
    let at = blk as usize * BLOCK;
    let page = &mut d.bytes_mut()[at..at + BLOCK];
    page.fill(0);
    for (i, &(start, len)) in extents.iter().enumerate() {
        page[8 * i..8 * i + 4].copy_from_slice(&start.to_le_bytes());
        page[8 * i + 4..8 * i + 8].copy_from_slice(&len.to_le_bytes());
    }
    let count = extents.len() as u32;
    page[BLOCK - 8..BLOCK - 4].copy_from_slice(&count.to_le_bytes());
    let crc = crc32c(&page[..BLOCK - 4]);
    page[BLOCK - 4..BLOCK].copy_from_slice(&crc.to_le_bytes());
}

fn write_dir_entries(d: &mut MemDisk, blk: u64, entries: &[(u32, &[u8])]) -> u64 {
    let at = blk as usize * BLOCK;
    let page = &mut d.bytes_mut()[at..at + BLOCK];
    page.fill(0);
    let mut pos = 0usize;
    for &(ino, name) in entries {
        page[pos..pos + 4].copy_from_slice(&ino.to_le_bytes());
        page[pos + 4] = name.len() as u8;
        page[pos + 5..pos + 5 + name.len()].copy_from_slice(name);
        pos += 5 + name.len();
    }
    pos as u64
}

fn set_bitmap(d: &mut MemDisk, g: &GeoView, b: u64, used: bool) {
    let at = g.bitmap_start as usize * BLOCK + (b / 8) as usize;
    let mask = 1u8 << (b % 8) as u32;
    if used {
        d.bytes_mut()[at] |= mask;
    } else {
        d.bytes_mut()[at] &= !mask;
    }
}

struct CountingDisk {
    inner: MemDisk,
    reads: usize,
    max_reads: usize,
}

impl BlockDevice for CountingDisk {
    fn block_count(&self) -> u64 {
        self.inner.block_count()
    }

    fn read(&mut self, block: u64, buf: &mut [u8; BLOCK]) -> Result<(), Error> {
        self.reads += 1;
        assert!(
            self.reads <= self.max_reads,
            "excessive block reads: {} > {}",
            self.reads,
            self.max_reads
        );
        self.inner.read(block, buf)
    }

    fn write(&mut self, block: u64, buf: &[u8; BLOCK]) -> Result<(), Error> {
        self.inner.write(block, buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.inner.flush()
    }
}

struct SparseDisk {
    blocks: u64,
    sb: [u8; BLOCK],
}

impl BlockDevice for SparseDisk {
    fn block_count(&self) -> u64 {
        self.blocks
    }

    fn read(&mut self, block: u64, buf: &mut [u8; BLOCK]) -> Result<(), Error> {
        if block == 0 {
            *buf = self.sb;
        } else {
            buf.fill(0);
        }
        Ok(())
    }

    fn write(&mut self, _block: u64, _buf: &[u8; BLOCK]) -> Result<(), Error> {
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

// =========================================================================
// Defect tests (must fail on unpatched src/, pass on patched src/)
// =========================================================================

#[test]
fn aliased_extents_rejected_on_mount_and_stat() {
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.create("/f").unwrap();
    fs.write("/f", 0, b"hello").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);

    let blk = (g.first_data + 1) as u32;
    set_bitmap(&mut d, &g, u64::from(blk), true);
    // Two direct extents pointing to the exact same block.
    write_raw_inode(&mut d, 2, KIND_FILE, 1, 8192, &[(blk, 1), (blk, 1)], 0);

    let mut fs = Fs::mount(d).unwrap();
    assert_eq!(fs.stat("/f"), Err(Error::Corrupt));
    let mut buf = [0u8; 16];
    assert_eq!(fs.read("/f", 0, &mut buf), Err(Error::Corrupt));
}

#[test]
fn indirect_aliasing_data_extent_rejected() {
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.create("/f").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);

    let ib = (g.first_data + 1) as u32;
    let b0 = ib + 1;
    for b in ib..ib + 7 {
        set_bitmap(&mut d, &g, u64::from(b), true);
    }
    // 7 extents where extent[0] aliases the indirect block `ib` itself.
    write_raw_indirect(&mut d, u64::from(ib), &[(b0 + 5, 1)]);
    write_raw_inode(
        &mut d,
        2,
        KIND_FILE,
        1,
        7 * BLOCK as u64,
        &[
            (ib, 1),
            (b0, 1),
            (b0 + 1, 1),
            (b0 + 2, 1),
            (b0 + 3, 1),
            (b0 + 4, 1),
        ],
        ib,
    );

    let mut fs = Fs::mount(d).unwrap();
    assert_eq!(fs.stat("/f"), Err(Error::Corrupt));
    assert_eq!(fs.write("/f", 0, b"corrupt"), Err(Error::Corrupt));
}

#[test]
fn aliased_extents_oom_dos_in_fsck_and_root_mount() {
    let mut d = blank(128, 32, 8);
    let g = sb_geo(&d);

    let ib = g.first_data as u32;
    let data_start = ib + 1;
    let run = (g.blocks - u64::from(data_start)) as u32;
    for b in g.first_data..g.blocks {
        set_bitmap(&mut d, &g, b, true);
    }
    let indirect_extents = [(data_start, run); 511];
    write_raw_indirect(&mut d, u64::from(ib), &indirect_extents);
    let total_size = 517u64 * u64::from(run) * BLOCK as u64;
    write_raw_inode(
        &mut d,
        ROOT,
        KIND_DIR,
        1,
        total_size,
        &[(data_start, run); 6],
        ib,
    );

    // 1. fsck must reject the self-overlapping/oversized extents without reading 50k+ blocks
    //    or allocating >200 MiB.
    let mut cd = CountingDisk {
        inner: d,
        reads: 0,
        max_reads: 1000,
    };
    let rep = fsck(&mut cd, false).unwrap();
    assert!(!rep.errors.is_empty());

    // 2. Fs::mount must reject the corrupt root inode.
    assert!(matches!(Fs::mount(cd.inner), Err(Error::Corrupt)));
}

#[test]
fn dir_entry_dot_and_dotdot_rejected() {
    for bad_name in [b".".as_slice(), b"..".as_slice()] {
        let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
        fs.create("/a").unwrap();
        let mut d = fs.unmount().unwrap();
        let g = sb_geo(&d);

        let dir_blk = g.first_data;
        let sz = write_dir_entries(&mut d, dir_blk, &[(2, bad_name)]);
        write_raw_inode(&mut d, ROOT, KIND_DIR, 1, sz, &[(dir_blk as u32, 1)], 0);

        let rep = fsck(&mut d, false).unwrap();
        assert!(
            !rep.errors.is_empty(),
            "fsck accepted directory entry {:?}",
            bad_name
        );

        let mut fs = Fs::mount(d).unwrap();
        assert_eq!(fs.list("/"), Err(Error::Corrupt));
    }
}

#[test]
fn dir_entry_non_utf8_rejected_by_fsck() {
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.create("/aa").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);

    let dir_blk = g.first_data;
    let sz = write_dir_entries(&mut d, dir_blk, &[(2, b"\xff\xfe")]);
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, sz, &[(dir_blk as u32, 1)], 0);

    let rep = fsck(&mut d, false).unwrap();
    assert!(!rep.errors.is_empty(), "fsck accepted non-utf8 dir entry");
}

#[test]
fn dir_entry_duplicate_names_rejected() {
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.create("/aa").unwrap();
    fs.create("/bb").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);

    let dir_blk = g.first_data;
    let sz = write_dir_entries(&mut d, dir_blk, &[(2, b"dup"), (3, b"dup")]);
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, sz, &[(dir_blk as u32, 1)], 0);

    let rep = fsck(&mut d, false).unwrap();
    assert!(!rep.errors.is_empty(), "fsck accepted duplicate dir entry");

    let mut fs = Fs::mount(d).unwrap();
    assert_eq!(fs.list("/"), Err(Error::Corrupt));
}

#[test]
fn fsck_detects_directory_cycles_and_root_links() {
    // Case 1: Entry in root pointing back to ROOT (inode 1).
    let mut d = blank(128, 32, 8);
    let g = sb_geo(&d);
    let dir_blk = g.first_data;
    set_bitmap(&mut d, &g, dir_blk, true);
    let sz = write_dir_entries(&mut d, dir_blk, &[(ROOT, b"loop")]);
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, sz, &[(dir_blk as u32, 1)], 0);

    let rep = fsck(&mut d, false).unwrap();
    assert!(
        !rep.errors.is_empty(),
        "fsck accepted entry pointing to ROOT"
    );

    // Case 2: Subdirectory cycle with links = 2 (/d -> inode 2, /d/self -> inode 2).
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.mkdir("/d").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);
    let sub_blk = g.first_data + 1;
    set_bitmap(&mut d, &g, sub_blk, true);
    let sz = write_dir_entries(&mut d, sub_blk, &[(2, b"self")]);
    write_raw_inode(&mut d, 2, KIND_DIR, 2, sz, &[(sub_blk as u32, 1)], 0);

    let rep = fsck(&mut d, false).unwrap();
    assert!(!rep.errors.is_empty(), "fsck accepted directory self-cycle");

    // Case 3: Pre-existing directory hardlink (/a -> 2, /b -> 2) cannot rename /a into /b/sub.
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.mkdir("/a").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);
    let root_blk = g.first_data;
    let sz = write_dir_entries(&mut d, root_blk, &[(2, b"a"), (2, b"b")]);
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, sz, &[(root_blk as u32, 1)], 0);
    write_raw_inode(&mut d, 2, KIND_DIR, 2, 0, &[], 0);

    let mut fs = Fs::mount(d).unwrap();
    assert_eq!(fs.rename("/a", "/b/sub"), Err(Error::BadPath));
}

#[test]
fn fsck_repair_preserves_files_when_dir_size_mismatches_extents() {
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.create("/keep").unwrap();
    fs.write("/keep", 0, b"important").unwrap();
    let mut d = fs.unmount().unwrap();
    let g = sb_geo(&d);

    // Damage ROOT so its size says 4096 but it has 0 extents.
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, 4096, &[], 0);

    let rep = fsck(&mut d, true).unwrap();
    assert!(!rep.errors.is_empty());
    // /keep (inode 2) must NOT be freed as an orphan while ROOT's directory is damaged.
    let at = inode_off(&g, 2);
    let kind = u32_at(&d.bytes()[at..at + INODE_SIZE], 0);
    assert_eq!(kind, KIND_FILE);
}

#[test]
fn free_inode_with_nonzero_size_or_extents_rejected() {
    let mut d = blank(128, 32, 8);
    write_raw_inode(&mut d, 2, KIND_FREE, 0, 4096, &[], 0);

    let rep = fsck(&mut d, false).unwrap();
    assert!(
        !rep.errors.is_empty(),
        "fsck accepted KIND_FREE inode with non-zero size"
    );
}

// =========================================================================
// Additional attack tests (malformed, oversized, truncated, hostile inputs)
// =========================================================================

#[test]
fn corrupted_superblock_variants_rejected() {
    // 0-block disk.
    let empty = MemDisk::new(0);
    assert!(matches!(Fs::mount(empty), Err(Error::Corrupt)));

    let base = blank(128, 32, 8);

    // Bad magic.
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    d.bytes_mut()[0..8].copy_from_slice(b"BADMAGIC");
    seal_sb(&mut d);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Bad CRC.
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    d.bytes_mut()[100] ^= 1;
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Wrong root inode number.
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    d.bytes_mut()[20..24].copy_from_slice(&2u32.to_le_bytes());
    seal_sb(&mut d);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Journal length < 2.
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    d.bytes_mut()[32..40].copy_from_slice(&1u64.to_le_bytes());
    seal_sb(&mut d);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Truncated backing device (sb says 128 blocks, device has 64).
    let mut trunc = MemDisk::new(64);
    trunc
        .bytes_mut()
        .copy_from_slice(&base.bytes()[..64 * BLOCK]);
    assert!(matches!(Fs::mount(trunc), Err(Error::Corrupt)));

    // Oversized blocks > u32::MAX on a sparse device.
    let mut sb = [0u8; BLOCK];
    sb[..8].copy_from_slice(MAGIC_SB);
    let huge = u64::from(u32::MAX) + 1;
    sb[8..16].copy_from_slice(&huge.to_le_bytes());
    sb[16..20].copy_from_slice(&32u32.to_le_bytes());
    sb[20..24].copy_from_slice(&ROOT.to_le_bytes());
    sb[24..32].copy_from_slice(&1u64.to_le_bytes());
    sb[32..40].copy_from_slice(&8u64.to_le_bytes());
    let crc = crc32c(&sb[..BLOCK - 4]);
    sb[BLOCK - 4..BLOCK].copy_from_slice(&crc.to_le_bytes());
    let mut sd = SparseDisk { blocks: huge, sb };
    assert_eq!(fsck(&mut sd, false), Err(Error::Corrupt));
    assert!(matches!(Fs::mount(sd), Err(Error::Corrupt)));
}

#[test]
fn corrupted_inode_and_indirect_bounds_rejected() {
    // Bad root kind.
    let mut d = blank(128, 32, 8);
    write_raw_inode(&mut d, ROOT, KIND_FILE, 1, 0, &[], 0);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Extent pointing into superblock / journal.
    let mut d = blank(128, 32, 8);
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, 4096, &[(0, 1)], 0);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Extent overflowing end of device.
    let mut d = blank(128, 32, 8);
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, 4096 * 10, &[(125, 10)], 0);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));

    // Indirect block with count > 511 and valid CRC.
    let mut d = blank(128, 32, 8);
    let g = sb_geo(&d);
    let ib = g.first_data;
    let at = ib as usize * BLOCK;
    d.bytes_mut()[at + BLOCK - 8..at + BLOCK - 4].copy_from_slice(&512u32.to_le_bytes());
    let crc = crc32c(&d.bytes()[at..at + BLOCK - 4]);
    d.bytes_mut()[at + BLOCK - 4..at + BLOCK].copy_from_slice(&crc.to_le_bytes());
    write_raw_inode(&mut d, ROOT, KIND_DIR, 1, 0, &[], ib as u32);
    assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));
}

#[test]
fn hostile_journal_records_discarded_safely() {
    let base = blank(128, 32, 8);
    let g = sb_geo(&base);

    // Helper to build a journal commit record with valid header CRC.
    let write_jrnl = |d: &mut MemDisk, n: u32, data_crc: u32, homes: &[u64]| {
        let at = g.journal_start as usize * BLOCK;
        let rec = &mut d.bytes_mut()[at..at + BLOCK];
        rec.fill(0);
        rec[0..8].copy_from_slice(MAGIC_JRNL);
        rec[8..12].copy_from_slice(&n.to_le_bytes());
        rec[12..16].copy_from_slice(&data_crc.to_le_bytes());
        for (i, &h) in homes.iter().enumerate() {
            rec[16 + 8 * i..24 + 8 * i].copy_from_slice(&h.to_le_bytes());
        }
        let crc = crc32c(&rec[..BLOCK - 4]);
        rec[BLOCK - 4..BLOCK].copy_from_slice(&crc.to_le_bytes());
    };

    let copy_crc = crc32c(&[0u8; BLOCK]);

    // 1. Hostile n = u32::MAX.
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    write_jrnl(&mut d, u32::MAX, copy_crc, &[]);
    let fs = Fs::mount(d).unwrap();
    let mut d = fs.unmount().unwrap();
    assert!(fsck(&mut d, false).unwrap().errors.is_empty());

    // 2. Hostile home = 0 (attempting to overwrite superblock).
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    write_jrnl(&mut d, 1, copy_crc, &[0]);
    let sb_before = d.bytes()[..BLOCK].to_vec();
    let fs = Fs::mount(d).unwrap();
    let mut d = fs.unmount().unwrap();
    assert_eq!(&d.bytes()[..BLOCK], sb_before.as_slice());
    assert!(fsck(&mut d, false).unwrap().errors.is_empty());

    // 3. Hostile home = blocks (out of range).
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    write_jrnl(&mut d, 1, copy_crc, &[g.blocks]);
    let fs = Fs::mount(d).unwrap();
    let mut d = fs.unmount().unwrap();
    assert!(fsck(&mut d, false).unwrap().errors.is_empty());

    // 4. Hostile home = journal_start (inside journal).
    let mut d = MemDisk::new(128);
    d.bytes_mut().copy_from_slice(base.bytes());
    write_jrnl(&mut d, 1, copy_crc, &[g.journal_start]);
    let fs = Fs::mount(d).unwrap();
    let mut d = fs.unmount().unwrap();
    assert!(fsck(&mut d, false).unwrap().errors.is_empty());
}

#[test]
fn path_traversal_empty_null_and_long_names_rejected() {
    let mut fs = Fs::mount(blank(128, 32, 8)).unwrap();
    fs.mkdir("/a").unwrap();
    fs.create("/a/f").unwrap();

    for bad in [
        "", "a", ".", "..", "/.", "/..", "/a/..", "/a/../a", "/a/./f", "/a//f", "/a/", "/a\0b",
    ] {
        assert_eq!(fs.create(bad), Err(Error::BadPath), "create({bad:?})");
        assert_eq!(fs.stat(bad), Err(Error::BadPath), "stat({bad:?})");
        assert_eq!(fs.remove(bad), Err(Error::BadPath), "remove({bad:?})");
    }

    let long = format!("/{}", "x".repeat(256));
    assert_eq!(fs.create(&long), Err(Error::NameTooLong));

    let max_ok = format!("/{}", "y".repeat(255));
    assert_eq!(fs.create(&max_ok), Ok(()));
    assert_eq!(fs.remove(&max_ok), Ok(()));

    assert_eq!(fs.rmdir("/"), Err(Error::BadPath));
    assert_eq!(fs.remove("/"), Err(Error::IsDir));
    assert_eq!(fs.rename("/", "/x"), Err(Error::BadPath));
    assert_eq!(fs.rename("/a", "/a/sub"), Err(Error::BadPath));
    assert_eq!(fs.write("/f_missing", u64::MAX, b"a"), Err(Error::NotFound));
    assert_eq!(fs.write("/a/f", u64::MAX, b"a"), Err(Error::TooLarge));
}

#[test]
fn deep_directory_tree_no_stack_overflow() {
    let mut fs = Fs::mount(blank(512, 256, 16)).unwrap();
    let mut paths = Vec::new();
    let mut cur = String::new();
    for i in 0..100 {
        cur.push_str(&format!("/d{i}"));
        fs.mkdir(&cur).unwrap();
        paths.push(cur.clone());
    }
    let file_path = format!("{cur}/leaf");
    fs.create(&file_path).unwrap();
    fs.write(&file_path, 0, b"deep").unwrap();
    let mut buf = [0u8; 4];
    assert_eq!(fs.read(&file_path, 0, &mut buf).unwrap(), 4);
    assert_eq!(&buf, b"deep");

    let mut d = fs.unmount().unwrap();
    assert!(fsck(&mut d, false).unwrap().errors.is_empty());

    let mut fs = Fs::mount(d).unwrap();
    fs.remove(&file_path).unwrap();
    for p in paths.into_iter().rev() {
        fs.rmdir(&p).unwrap();
    }
    let mut d = fs.unmount().unwrap();
    assert!(fsck(&mut d, false).unwrap().errors.is_empty());
}

#[test]
fn fsck_on_all_zero_all_ff_and_random_garbage() {
    let start = Instant::now();

    // All-zero and all-0xFF disks.
    for fill in [0x00u8, 0xFFu8] {
        let mut d = MemDisk::new(128);
        d.bytes_mut().fill(fill);
        assert_eq!(fsck(&mut d, false), Err(Error::Corrupt));
        assert!(matches!(Fs::mount(d), Err(Error::Corrupt)));
    }

    // Valid superblock + random garbage everywhere else.
    let base = blank(128, 32, 8);
    let mut rng = Rng::new(0x5EC0_0080);
    for _ in 0..50 {
        let mut d = MemDisk::new(128);
        d.bytes_mut()[..BLOCK].copy_from_slice(&base.bytes()[..BLOCK]);
        rng.fill(&mut d.bytes_mut()[BLOCK..]);
        let _ = fsck(&mut d, false);
        let _ = fsck(&mut d, true);
        let _ = Fs::mount(d);
    }

    assert!(start.elapsed().as_secs_f64() < 1.0);
}
