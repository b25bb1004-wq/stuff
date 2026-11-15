//! hades: the file system of Monolith OS. Version 1 on-disk format, mkfs, mount and path-based
//! file and directory operations. Every operation buffers its block writes in one private
//! transaction and commits them through a write-ahead journal, so a power cut leaves either the
//! state before or the state after the operation. `fsck` checks a device for consistency.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod crc;
mod disk;
mod fs;
mod fsck;
mod journal;
mod layout;
mod ops;
mod txn;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_crash;
#[cfg(test)]
mod tests_fsck;
#[cfg(test)]
mod tests_prop;

use alloc::string::String;

pub use disk::{BlockDevice, CrashDisk, MemDisk};
pub use fs::{mkfs, Fs};
pub use fsck::{fsck, Report};

/// Block size in bytes; every block on disk is this long.
pub const BLOCK: usize = 4096;

/// Why an operation failed. Corrupt means a disk check failed. Nothing is changed on error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The device failed to read or write.
    Io,
    /// On-disk data failed a magic, CRC, range or layout check.
    Corrupt,
    /// No entry or file has that path.
    NotFound,
    /// The name is already in use.
    Exists,
    /// A path component is not a directory.
    NotDir,
    /// The operation needs a file but the path is a directory.
    IsDir,
    /// The directory still has entries.
    NotEmpty,
    /// No free block or inode is left.
    NoSpace,
    /// A name is longer than 255 bytes.
    NameTooLong,
    /// The path is relative, has an empty or dot component, or is otherwise not allowed.
    BadPath,
    /// The file needs more extents than the inode and its indirect block can hold, or the
    /// operation changes more blocks in place than the journal can log. Nothing is written.
    TooLarge,
}

/// What a directory entry or an inode is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
}

/// Metadata of one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    /// File or directory.
    pub kind: Kind,
    /// Size in bytes. For a directory, the size of its entry table.
    pub size: u64,
    /// Inode number.
    pub inode: u32,
    /// Link count.
    pub links: u32,
}

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Entry name.
    pub name: String,
    /// File or directory.
    pub kind: Kind,
    /// Inode number.
    pub inode: u32,
}

/// Free space and free inodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsStat {
    /// Blocks on the device.
    pub total_blocks: u64,
    /// Data blocks not in use.
    pub free_blocks: u64,
    /// Inodes not in use.
    pub free_inodes: u32,
}

/// Options for `mkfs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MkfsOptions {
    /// Number of inodes. 0 picks blocks / 4, at least 32. Values below 2 are raised to 2.
    pub inodes: u32,
    /// Blocks reserved for the journal. Values below 2 are raised to 2.
    pub journal_blocks: u32,
}

impl Default for MkfsOptions {
    fn default() -> Self {
        MkfsOptions {
            inodes: 0,
            journal_blocks: 64,
        }
    }
}
