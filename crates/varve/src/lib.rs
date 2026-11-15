//! Transactional 4 KiB pager with redo write-ahead log and crash recovery.
#![forbid(unsafe_code)]

mod crash;
mod crc;
mod format;
mod pager;
mod storage;
mod tree;
mod wal;

use std::fmt;

pub use crash::CrashStorage;
pub use pager::{Pager, Snapshot};
pub use storage::{FileStorage, MemStorage, Storage};
pub use tree::Tree;

/// Page size in bytes (4 KiB).
pub const PAGE: usize = 4096;

/// Number of usable bytes per page before the trailing 4-byte CRC-32.
pub const USABLE: usize = 4092;

/// Fixed-size 4096-byte page buffer.
pub type Page = [u8; PAGE];

/// Error returned by storage, decoding, and pager operations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Underlying storage I/O failure or short read.
    Io,
    /// Checksum mismatch or invalid page/WAL format.
    Corrupt,
    /// Size or count limit exceeded.
    TooLarge,
    /// A write transaction is already active.
    Busy,
    /// Caller violated API preconditions (missing tx, invalid page, released snapshot).
    Misuse,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io => f.write_str("storage I/O error"),
            Error::Corrupt => f.write_str("corrupt page or log"),
            Error::TooLarge => f.write_str("size limit exceeded"),
            Error::Busy => f.write_str("write transaction already active"),
            Error::Misuse => f.write_str("invalid pager operation"),
        }
    }
}

impl std::error::Error for Error {}
