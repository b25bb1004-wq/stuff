//! The public handle: mkfs, mount, unmount, and the path operations. Each public call runs one
//! `Txn` and commits it only if the whole operation succeeded.
use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::journal;
use crate::layout::{self, Geo, Inode, KIND_DIR, KIND_FILE, KIND_FREE, ROOT};
use crate::ops;
use crate::txn::Txn;
use crate::{DirEntry, Error, FsStat, MkfsOptions, Stat, BLOCK};

/// Write an empty file system to `dev`. NoSpace if the device is too small for the layout.
pub fn mkfs<D: BlockDevice>(dev: &mut D, opts: MkfsOptions) -> Result<(), Error> {
    let blocks = dev.block_count();
    let inodes = if opts.inodes == 0 {
        (blocks / 4).clamp(32, u64::from(u32::MAX)) as u32
    } else {
        opts.inodes.max(2)
    };
    let jblocks = u64::from(opts.journal_blocks.max(2));
    let geo = Geo::plan(blocks, inodes, jblocks).ok_or(Error::NoSpace)?;
    let mut t = Txn::new(&mut *dev, geo);
    format(&mut t)?;
    t.commit_direct()?;
    dev.flush()
}

fn format<D: BlockDevice>(t: &mut Txn<'_, D>) -> Result<(), Error> {
    let g = t.geo;
    for b in g.journal_start..g.bitmap_start {
        t.fresh(b)?;
    }
    for b in g.bitmap_start..g.itable_start {
        t.fresh(b)?;
    }
    for b in 0..g.first_data {
        t.set_used(b, true)?;
    }
    for b in g.itable_start..g.first_data {
        t.fresh(b)?;
    }
    for n in 1..g.inodes {
        t.write_inode(n, &mut Inode::new(KIND_FREE))?;
    }
    t.write_inode(ROOT, &mut Inode::new(KIND_DIR))?;
    layout::encode_sb(&g, ROOT, t.fresh(0)?)
}

/// Read the superblock of `dev` and check it against the device. Corrupt if it does not fit.
pub(crate) fn read_geo<D: BlockDevice>(dev: &mut D) -> Result<Geo, Error> {
    let blocks = dev.block_count();
    if blocks == 0 {
        return Err(Error::Corrupt);
    }
    let mut sb = Box::new([0u8; BLOCK]);
    dev.read(0, &mut sb)?;
    let (geo, root) = layout::decode_sb(&sb[..])?;
    if geo.blocks != blocks
        || root != ROOT
        || Geo::plan(geo.blocks, geo.inodes, geo.journal_len) != Some(geo)
    {
        return Err(Error::Corrupt);
    }
    Ok(geo)
}

/// A mounted file system on a block device.
pub struct Fs<D: BlockDevice> {
    dev: D,
    geo: Geo,
}

impl<D: BlockDevice> Fs<D> {
    /// Mount the file system on `dev`, replaying a complete journal transaction first.
    /// Corrupt on a bad superblock, CRC, layout or root.
    pub fn mount(mut dev: D) -> Result<Fs<D>, Error> {
        let geo = read_geo(&mut dev)?;
        journal::recover(&mut dev, &geo)?;
        let mut fs = Fs { dev, geo };
        {
            let mut t = Txn::new(&mut fs.dev, fs.geo);
            if t.read_inode(ROOT)?.kind != KIND_DIR {
                return Err(Error::Corrupt);
            }
        }
        Ok(fs)
    }

    /// Flush the device and give it back.
    pub fn unmount(mut self) -> Result<D, Error> {
        self.dev.flush()?;
        Ok(self.dev)
    }

    fn run<R>(&mut self, op: impl FnOnce(&mut Txn<'_, D>) -> Result<R, Error>) -> Result<R, Error> {
        let mut t = Txn::new(&mut self.dev, self.geo);
        let out = op(&mut t)?;
        t.commit()?;
        Ok(out)
    }

    /// Create an empty file at `path`. Exists if the name is taken.
    pub fn create(&mut self, path: &str) -> Result<(), Error> {
        self.run(|t| ops::make(t, path, KIND_FILE))
    }

    /// Create an empty directory at `path`. Exists if the name is taken.
    pub fn mkdir(&mut self, path: &str) -> Result<(), Error> {
        self.run(|t| ops::make(t, path, KIND_DIR))
    }

    /// Write `data` into the file at `path` at `offset`, growing it. Returns the bytes written.
    pub fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, Error> {
        self.run(|t| ops::write(t, path, offset, data))
    }

    /// Read from the file at `path` at `offset` into `buf`. Short at the end of the file.
    pub fn read(&mut self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        self.run(|t| ops::read(t, path, offset, buf))
    }

    /// Set the size of the file at `path`. Shrinking frees blocks; growing zero-fills.
    pub fn truncate(&mut self, path: &str, size: u64) -> Result<(), Error> {
        self.run(|t| ops::truncate(t, path, size))
    }

    /// Remove the file at `path`. IsDir for a directory.
    pub fn remove(&mut self, path: &str) -> Result<(), Error> {
        self.run(|t| ops::remove(t, path))
    }

    /// Remove the empty directory at `path`. NotEmpty unless it is empty; BadPath for the root.
    pub fn rmdir(&mut self, path: &str) -> Result<(), Error> {
        self.run(|t| ops::rmdir(t, path))
    }

    /// Move `from` to `to`. Exists if `to` exists; BadPath if a directory would move into itself.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        self.run(|t| ops::rename(t, from, to))
    }

    /// Entries of the directory at `path`, sorted by name.
    pub fn list(&mut self, path: &str) -> Result<Vec<DirEntry>, Error> {
        self.run(|t| ops::list(t, path))
    }

    /// Metadata of `path`.
    pub fn stat(&mut self, path: &str) -> Result<Stat, Error> {
        self.run(|t| ops::stat(t, path))
    }

    /// Free blocks and free inodes.
    pub fn statfs(&mut self) -> Result<FsStat, Error> {
        self.run(|t| {
            let total_blocks = t.geo.blocks;
            let free_blocks = t.count_free()?;
            let mut free_inodes = 0u32;
            for n in 2..t.geo.inodes {
                if t.read_inode(n)?.kind == KIND_FREE {
                    free_inodes += 1;
                }
            }
            Ok(FsStat {
                total_blocks,
                free_blocks,
                free_inodes,
            })
        })
    }
}
