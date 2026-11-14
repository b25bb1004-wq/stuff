//! The transaction buffer. Every block that one operation reads or writes goes through a `Txn`;
//! `commit` is the only place where blocks reach the device. Dropping a `Txn` changes nothing.
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::journal::{self, Page};
use crate::layout::{
    self, Geo, Inode, BITS_PER_BLOCK, BLOCK64, DIRECT, INODES_PER_BLOCK, INODE_SIZE, KIND_FREE,
    MAX_EXTENTS,
};
use crate::{Error, BLOCK};

struct Slot {
    data: Box<[u8; BLOCK]>,
    dirty: bool,
}

/// A private write buffer over a device. Reads see its own copies; `commit` writes the dirty ones.
pub(crate) struct Txn<'a, D: BlockDevice> {
    dev: &'a mut D,
    pub(crate) geo: Geo,
    slots: BTreeMap<u64, Slot>,
}

impl<'a, D: BlockDevice> Txn<'a, D> {
    pub(crate) fn new(dev: &'a mut D, geo: Geo) -> Self {
        Txn {
            dev,
            geo,
            slots: BTreeMap::new(),
        }
    }

    fn slot(&mut self, b: u64, fresh: bool) -> Result<&mut Slot, Error> {
        if b >= self.geo.blocks {
            return Err(Error::Corrupt);
        }
        if !self.slots.contains_key(&b) {
            let mut data = Box::new([0u8; BLOCK]);
            if !fresh {
                self.dev.read(b, &mut data)?;
            }
            self.slots.insert(b, Slot { data, dirty: false });
        }
        self.slots.get_mut(&b).ok_or(Error::Corrupt)
    }

    /// Read-only view of block `b`.
    pub(crate) fn get(&mut self, b: u64) -> Result<&[u8; BLOCK], Error> {
        Ok(&*self.slot(b, false)?.data)
    }

    /// Mutable view of block `b`. The block is written at commit.
    pub(crate) fn get_mut(&mut self, b: u64) -> Result<&mut [u8; BLOCK], Error> {
        let s = self.slot(b, false)?;
        s.dirty = true;
        Ok(&mut *s.data)
    }

    /// Zeroed view of block `b`, without reading the device. The block is written at commit.
    pub(crate) fn fresh(&mut self, b: u64) -> Result<&mut [u8; BLOCK], Error> {
        let s = self.slot(b, true)?;
        s.data.fill(0);
        s.dirty = true;
        Ok(&mut *s.data)
    }

    /// Commit the dirty blocks through the journal. This is the only device write of an operation.
    /// Data blocks that were free on the device are written home first; the rest is logged.
    pub(crate) fn commit(self) -> Result<(), Error> {
        let Txn { dev, geo, slots } = self;
        let mut bitmaps: BTreeMap<u64, Page> = BTreeMap::new();
        let mut direct: Vec<(u64, &[u8; BLOCK])> = Vec::new();
        let mut logged: Vec<(u64, &[u8; BLOCK])> = Vec::new();
        for (&b, s) in slots.iter() {
            if !s.dirty {
                continue;
            }
            if geo.in_data(b) && !was_used(dev, &geo, b, &mut bitmaps)? {
                direct.push((b, &*s.data));
            } else {
                logged.push((b, &*s.data));
            }
        }
        journal::commit(dev, &geo, &direct, &logged)
    }

    /// Write every dirty block straight to the device, superblock (block 0) last. Used by mkfs,
    /// which has no earlier state to protect.
    pub(crate) fn commit_direct(self) -> Result<(), Error> {
        let Txn { dev, slots, .. } = self;
        for (b, s) in slots.iter().rev() {
            if s.dirty {
                dev.write(*b, &s.data)?;
            }
        }
        Ok(())
    }

    fn bit_pos(&self, b: u64) -> Result<(u64, usize, u8), Error> {
        if b >= self.geo.blocks {
            return Err(Error::Corrupt);
        }
        let blk = self
            .geo
            .bitmap_start
            .checked_add(b / BITS_PER_BLOCK)
            .ok_or(Error::Corrupt)?;
        let idx = b % BITS_PER_BLOCK;
        Ok((blk, (idx / 8) as usize, (idx % 8) as u8))
    }

    /// True when block `b` is marked used in the bitmap of this transaction.
    pub(crate) fn is_used(&mut self, b: u64) -> Result<bool, Error> {
        let (blk, byte, bit) = self.bit_pos(b)?;
        let v = *self.get(blk)?.get(byte).ok_or(Error::Corrupt)?;
        Ok(v & (1 << bit) != 0)
    }

    /// Mark block `b` used or free in the bitmap.
    pub(crate) fn set_used(&mut self, b: u64, used: bool) -> Result<(), Error> {
        let (blk, byte, bit) = self.bit_pos(b)?;
        let page = self.get_mut(blk)?;
        let v = page.get_mut(byte).ok_or(Error::Corrupt)?;
        if used {
            *v |= 1 << bit;
        } else {
            *v &= !(1 << bit);
        }
        Ok(())
    }

    /// Take one free data block, searching from `hint`. The block comes back zeroed.
    pub(crate) fn alloc(&mut self, hint: u64) -> Result<u64, Error> {
        let (first, n) = (self.geo.first_data, self.geo.blocks);
        if first >= n {
            return Err(Error::NoSpace);
        }
        let span = n - first;
        let start = if (first..n).contains(&hint) {
            hint - first
        } else {
            0
        };
        for i in 0..span {
            let cand = first + (start + i) % span;
            if !self.is_used(cand)? {
                self.set_used(cand, true)?;
                self.fresh(cand)?;
                return Ok(cand);
            }
        }
        Err(Error::NoSpace)
    }

    /// Return data block `b` to the free pool.
    pub(crate) fn free(&mut self, b: u64) -> Result<(), Error> {
        self.set_used(b, false)
    }

    /// Number of free data blocks.
    pub(crate) fn count_free(&mut self) -> Result<u64, Error> {
        let mut free = 0;
        for b in self.geo.first_data..self.geo.blocks {
            if !self.is_used(b)? {
                free += 1;
            }
        }
        Ok(free)
    }

    /// Block and byte offset of inode `n` in the inode table.
    pub(crate) fn inode_pos(&self, n: u32) -> Result<(u64, usize), Error> {
        if n == 0 || n >= self.geo.inodes {
            return Err(Error::Corrupt);
        }
        let blk = self
            .geo
            .itable_start
            .checked_add(u64::from(n / INODES_PER_BLOCK))
            .ok_or(Error::Corrupt)?;
        Ok((blk, (n % INODES_PER_BLOCK) as usize * INODE_SIZE))
    }

    /// Read inode `n`, with its indirect extents, and check its extents against the layout.
    pub(crate) fn read_inode(&mut self, n: u32) -> Result<Inode, Error> {
        let (blk, off) = self.inode_pos(n)?;
        let mut ino = {
            let page = self.get(blk)?;
            layout::decode_inode(layout::sub(page, off, INODE_SIZE)?)?
        };
        if ino.indirect != 0 {
            let ib = u64::from(ino.indirect);
            if !(self.geo.first_data..self.geo.blocks).contains(&ib) {
                return Err(Error::Corrupt);
            }
            let more = layout::decode_indirect(self.get(ib)?)?;
            ino.extents.extend(more);
        }
        let mut sum: u64 = 0;
        for e in &ino.extents {
            if !self.geo.valid(e) {
                return Err(Error::Corrupt);
            }
            sum = sum.checked_add(e.len).ok_or(Error::Corrupt)?;
        }
        if sum != ino.size.div_ceil(BLOCK64) {
            return Err(Error::Corrupt);
        }
        Ok(ino)
    }

    /// Write inode `n`. Frees or allocates its indirect block as the extent count requires.
    pub(crate) fn write_inode(&mut self, n: u32, ino: &mut Inode) -> Result<(), Error> {
        let (blk, off) = self.inode_pos(n)?;
        if ino.extents.len() > MAX_EXTENTS {
            return Err(Error::TooLarge);
        }
        let need = ino.extents.len() > DIRECT;
        if need && ino.indirect == 0 {
            let hint = self.geo.first_data;
            let b = self.alloc(hint)?;
            ino.indirect = u32::try_from(b).map_err(|_| Error::NoSpace)?;
        } else if !need && ino.indirect != 0 {
            let old = u64::from(ino.indirect);
            ino.indirect = 0;
            self.free(old)?;
        }
        if need {
            let ib = u64::from(ino.indirect);
            let page = self.get_mut(ib)?;
            layout::encode_indirect(ino.extents.get(DIRECT..).unwrap_or_default(), page)?;
        }
        let page = self.get_mut(blk)?;
        layout::encode_inode(ino, layout::sub_mut(page, off, INODE_SIZE)?)
    }

    /// Find a free inode number (never 0 or the root).
    pub(crate) fn alloc_inode(&mut self) -> Result<u32, Error> {
        for n in 2..self.geo.inodes {
            if self.read_inode(n)?.kind == KIND_FREE {
                return Ok(n);
            }
        }
        Err(Error::NoSpace)
    }
}

/// Whether data block `b` is marked used in the bitmap on the device, which is the committed state.
fn was_used<D: BlockDevice>(
    dev: &mut D,
    geo: &Geo,
    b: u64,
    cache: &mut BTreeMap<u64, Page>,
) -> Result<bool, Error> {
    let blk = geo
        .bitmap_start
        .checked_add(b / BITS_PER_BLOCK)
        .ok_or(Error::Corrupt)?;
    let page = match cache.entry(blk) {
        alloc::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
        alloc::collections::btree_map::Entry::Vacant(e) => {
            let mut page = Box::new([0u8; BLOCK]);
            dev.read(blk, &mut page)?;
            e.insert(page)
        }
    };
    let idx = b % BITS_PER_BLOCK;
    let byte = *page.get((idx / 8) as usize).ok_or(Error::Corrupt)?;
    Ok(byte & (1 << ((idx % 8) as u32)) != 0)
}
