//! Byte layouts of the on-disk structures (superblock, inode, indirect block) and the region
//! plan of a device. Every decoder checks CRCs and returns Corrupt instead of panicking.
use alloc::vec::Vec;

use crate::crc::crc32c;
use crate::{Error, BLOCK};

pub(crate) const MAGIC: &[u8] = b"SHALE001";
pub(crate) const INODE_SIZE: usize = 128;
pub(crate) const INODES_PER_BLOCK: u32 = 32;
pub(crate) const DIRECT: usize = 6;
pub(crate) const INDIRECT_MAX: usize = 511;
pub(crate) const MAX_EXTENTS: usize = DIRECT + INDIRECT_MAX;
pub(crate) const ROOT: u32 = 1;
pub(crate) const KIND_FREE: u32 = 0;
pub(crate) const KIND_FILE: u32 = 1;
pub(crate) const KIND_DIR: u32 = 2;
pub(crate) const BITS_PER_BLOCK: u64 = BLOCK as u64 * 8;
pub(crate) const BLOCK64: u64 = BLOCK as u64;

const SB_CRC: usize = BLOCK - 4;
const INODE_CRC: usize = INODE_SIZE - 4;
const IND_COUNT: usize = BLOCK - 8;
const IND_CRC: usize = BLOCK - 4;

/// Byte range `at..at + len` of `b`, or Corrupt if it does not fit.
pub(crate) fn sub(b: &[u8], at: usize, len: usize) -> Result<&[u8], Error> {
    let end = at.checked_add(len).ok_or(Error::Corrupt)?;
    b.get(at..end).ok_or(Error::Corrupt)
}

/// Mutable byte range `at..at + len` of `b`, or Corrupt if it does not fit.
pub(crate) fn sub_mut(b: &mut [u8], at: usize, len: usize) -> Result<&mut [u8], Error> {
    let end = at.checked_add(len).ok_or(Error::Corrupt)?;
    b.get_mut(at..end).ok_or(Error::Corrupt)
}

/// Copy `src` into `b` at `at`.
pub(crate) fn put(b: &mut [u8], at: usize, src: &[u8]) -> Result<(), Error> {
    sub_mut(b, at, src.len())?.copy_from_slice(src);
    Ok(())
}

/// Little-endian u32 at `at`.
pub(crate) fn u32_at(b: &[u8], at: usize) -> Result<u32, Error> {
    let raw: [u8; 4] = sub(b, at, 4)?.try_into().map_err(|_| Error::Corrupt)?;
    Ok(u32::from_le_bytes(raw))
}

/// Little-endian u64 at `at`.
pub(crate) fn u64_at(b: &[u8], at: usize) -> Result<u64, Error> {
    let raw: [u8; 8] = sub(b, at, 8)?.try_into().map_err(|_| Error::Corrupt)?;
    Ok(u64::from_le_bytes(raw))
}

fn to_u32(v: u64) -> Result<u32, Error> {
    u32::try_from(v).map_err(|_| Error::Corrupt)
}

/// Region plan of a device: where each part of the layout starts and how long it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Geo {
    pub blocks: u64,
    pub inodes: u32,
    pub journal_start: u64,
    pub journal_len: u64,
    pub bitmap_start: u64,
    pub bitmap_len: u64,
    pub itable_start: u64,
    pub itable_len: u64,
    pub first_data: u64,
}

impl Geo {
    /// The layout for `blocks` blocks, `inodes` inodes and a journal of `journal` blocks.
    /// None if the parts do not fit or leave no data block.
    pub(crate) fn plan(blocks: u64, inodes: u32, journal: u64) -> Option<Geo> {
        if inodes < 2 || blocks > u64::from(u32::MAX) {
            return None;
        }
        let journal_start = 1u64;
        let bitmap_start = journal_start.checked_add(journal)?;
        let bitmap_len = blocks.div_ceil(BITS_PER_BLOCK);
        let itable_start = bitmap_start.checked_add(bitmap_len)?;
        let itable_len = u64::from(inodes).div_ceil(u64::from(INODES_PER_BLOCK));
        let first_data = itable_start.checked_add(itable_len)?;
        if first_data >= blocks {
            return None;
        }
        Some(Geo {
            blocks,
            inodes,
            journal_start,
            journal_len: journal,
            bitmap_start,
            bitmap_len,
            itable_start,
            itable_len,
            first_data,
        })
    }

    /// True when `e` covers data blocks only, inside the device, and is not empty.
    pub(crate) fn valid(&self, e: &Extent) -> bool {
        e.len >= 1
            && e.start >= self.first_data
            && e.start
                .checked_add(e.len)
                .is_some_and(|end| end <= self.blocks)
    }
}

/// A run of consecutive blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub start: u64,
    pub len: u64,
}

/// An inode as held in memory. `extents` holds the direct and the indirect extents in order.
#[derive(Debug)]
pub(crate) struct Inode {
    pub kind: u32,
    pub links: u32,
    pub size: u64,
    pub extents: Vec<Extent>,
    pub indirect: u32,
}

impl Inode {
    /// An inode of `kind` with no data.
    pub(crate) fn new(kind: u32) -> Inode {
        Inode {
            kind,
            links: 1,
            size: 0,
            extents: Vec::new(),
            indirect: 0,
        }
    }
}

/// Write the superblock for `g` into `out` (one block).
pub(crate) fn encode_sb(g: &Geo, root: u32, out: &mut [u8]) -> Result<(), Error> {
    out.fill(0);
    put(out, 0, MAGIC)?;
    put(out, 8, &g.blocks.to_le_bytes())?;
    put(out, 16, &g.inodes.to_le_bytes())?;
    put(out, 20, &root.to_le_bytes())?;
    let fields = [
        g.journal_start,
        g.journal_len,
        g.bitmap_start,
        g.bitmap_len,
        g.itable_start,
        g.itable_len,
        g.first_data,
    ];
    for (i, v) in fields.iter().enumerate() {
        put(out, 24 + 8 * i, &v.to_le_bytes())?;
    }
    let crc = crc32c(sub(out, 0, SB_CRC)?);
    put(out, SB_CRC, &crc.to_le_bytes())
}

/// Read the superblock in `b`: the geometry and the root inode number.
pub(crate) fn decode_sb(b: &[u8]) -> Result<(Geo, u32), Error> {
    if sub(b, 0, 8)? != MAGIC {
        return Err(Error::Corrupt);
    }
    if u32_at(b, SB_CRC)? != crc32c(sub(b, 0, SB_CRC)?) {
        return Err(Error::Corrupt);
    }
    let f = |at: usize| u64_at(b, at);
    let geo = Geo {
        blocks: f(8)?,
        inodes: u32_at(b, 16)?,
        journal_start: f(24)?,
        journal_len: f(32)?,
        bitmap_start: f(40)?,
        bitmap_len: f(48)?,
        itable_start: f(56)?,
        itable_len: f(64)?,
        first_data: f(72)?,
    };
    Ok((geo, u32_at(b, 20)?))
}

/// Read an inode from its 128 bytes. The indirect extents are not read here.
pub(crate) fn decode_inode(raw: &[u8]) -> Result<Inode, Error> {
    if u32_at(raw, INODE_CRC)? != crc32c(sub(raw, 0, INODE_CRC)?) {
        return Err(Error::Corrupt);
    }
    let kind = u32_at(raw, 0)?;
    if kind > KIND_DIR {
        return Err(Error::Corrupt);
    }
    let mut extents = Vec::new();
    for i in 0..DIRECT {
        let start = u32_at(raw, 16 + 8 * i)?;
        let len = u32_at(raw, 20 + 8 * i)?;
        if len != 0 {
            extents.push(Extent {
                start: u64::from(start),
                len: u64::from(len),
            });
        }
    }
    Ok(Inode {
        kind,
        links: u32_at(raw, 4)?,
        size: u64_at(raw, 8)?,
        extents,
        indirect: u32_at(raw, 64)?,
    })
}

/// Write `ino` into its 128 bytes `out`. Extents past the sixth go to the indirect block.
pub(crate) fn encode_inode(ino: &Inode, out: &mut [u8]) -> Result<(), Error> {
    put(out, 0, &ino.kind.to_le_bytes())?;
    put(out, 4, &ino.links.to_le_bytes())?;
    put(out, 8, &ino.size.to_le_bytes())?;
    for i in 0..DIRECT {
        let (start, len) = match ino.extents.get(i) {
            Some(e) => (to_u32(e.start)?, to_u32(e.len)?),
            None => (0, 0),
        };
        put(out, 16 + 8 * i, &start.to_le_bytes())?;
        put(out, 20 + 8 * i, &len.to_le_bytes())?;
    }
    put(out, 64, &ino.indirect.to_le_bytes())?;
    put(out, 68, &[0u8; 56])?;
    let crc = crc32c(sub(out, 0, INODE_CRC)?);
    put(out, INODE_CRC, &crc.to_le_bytes())
}

/// Read the indirect block in `b`: up to 511 extents.
pub(crate) fn decode_indirect(b: &[u8]) -> Result<Vec<Extent>, Error> {
    if u32_at(b, IND_CRC)? != crc32c(sub(b, 0, IND_CRC)?) {
        return Err(Error::Corrupt);
    }
    let count = usize::try_from(u32_at(b, IND_COUNT)?).map_err(|_| Error::Corrupt)?;
    if count > INDIRECT_MAX {
        return Err(Error::Corrupt);
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = u32_at(b, 8 * i)?;
        let len = u32_at(b, 8 * i + 4)?;
        out.push(Extent {
            start: u64::from(start),
            len: u64::from(len),
        });
    }
    Ok(out)
}

/// Write `ext` (at most 511 extents) and its count and CRC into the indirect block `out`.
pub(crate) fn encode_indirect(ext: &[Extent], out: &mut [u8]) -> Result<(), Error> {
    if ext.len() > INDIRECT_MAX {
        return Err(Error::TooLarge);
    }
    out.fill(0);
    for (i, e) in ext.iter().enumerate() {
        put(out, 8 * i, &to_u32(e.start)?.to_le_bytes())?;
        put(out, 8 * i + 4, &to_u32(e.len)?.to_le_bytes())?;
    }
    let count = u32::try_from(ext.len()).map_err(|_| Error::TooLarge)?;
    put(out, IND_COUNT, &count.to_le_bytes())?;
    let crc = crc32c(sub(out, 0, IND_CRC)?);
    put(out, IND_CRC, &crc.to_le_bytes())
}
