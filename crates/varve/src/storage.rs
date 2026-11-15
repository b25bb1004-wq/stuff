//! Byte storage abstraction and memory/file implementations.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::Error;

/// Random-access byte store underlying a database file or write-ahead log.
pub trait Storage {
    /// Return the current byte length of the store.
    fn len(&mut self) -> Result<u64, Error>;

    /// Return whether the store has zero bytes.
    fn is_empty(&mut self) -> Result<bool, Error> {
        Ok(self.len()? == 0)
    }

    /// Read `buf.len()` bytes starting at `off`; returns `Error::Io` if past the end.
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), Error>;

    /// Write `data` at `off`, growing the store with zero bytes for any gap.
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), Error>;

    /// Truncate or zero-extend the store to `n` bytes.
    fn set_len(&mut self, n: u64) -> Result<(), Error>;

    /// Flush all prior writes to durable storage.
    fn sync(&mut self) -> Result<(), Error>;
}

/// In-memory byte store backed by a `Vec<u8>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemStorage {
    data: Vec<u8>,
}

impl MemStorage {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Wrap an existing byte buffer as a memory store.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { data: bytes }
    }

    /// Borrow the full byte contents of the store.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}

impl Storage for MemStorage {
    fn len(&mut self) -> Result<u64, Error> {
        Ok(self.data.len() as u64)
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), Error> {
        let end = off.checked_add(buf.len() as u64).ok_or(Error::Io)?;
        let start = usize::try_from(off).map_err(|_| Error::Io)?;
        let end = usize::try_from(end).map_err(|_| Error::Io)?;
        let slice = self.data.get(start..end).ok_or(Error::Io)?;
        buf.copy_from_slice(slice);
        Ok(())
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), Error> {
        let end = off.checked_add(data.len() as u64).ok_or(Error::Io)?;
        let start = usize::try_from(off).map_err(|_| Error::Io)?;
        let end = usize::try_from(end).map_err(|_| Error::Io)?;
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        let dst = self.data.get_mut(start..end).ok_or(Error::Io)?;
        dst.copy_from_slice(data);
        Ok(())
    }

    fn set_len(&mut self, n: u64) -> Result<(), Error> {
        let len = usize::try_from(n).map_err(|_| Error::Io)?;
        self.data.resize(len, 0);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// File-backed byte store using `std::fs::File`.
#[derive(Debug)]
pub struct FileStorage {
    file: File,
}

impl FileStorage {
    /// Open or create a file at `path` for read/write storage.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| Error::Io)?;
        Ok(Self { file })
    }
}

impl Storage for FileStorage {
    fn len(&mut self) -> Result<u64, Error> {
        self.file.metadata().map(|m| m.len()).map_err(|_| Error::Io)
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), Error> {
        let end = off.checked_add(buf.len() as u64).ok_or(Error::Io)?;
        if buf.is_empty() {
            if end > self.len()? {
                return Err(Error::Io);
            }
            return Ok(());
        }
        self.file
            .seek(SeekFrom::Start(off))
            .map_err(|_| Error::Io)?;
        self.file.read_exact(buf).map_err(|_| Error::Io)
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), Error> {
        let end = off.checked_add(data.len() as u64).ok_or(Error::Io)?;
        if data.is_empty() {
            if end > self.len()? {
                self.set_len(end)?;
            }
            return Ok(());
        }
        self.file
            .seek(SeekFrom::Start(off))
            .map_err(|_| Error::Io)?;
        self.file.write_all(data).map_err(|_| Error::Io)
    }

    fn set_len(&mut self, n: u64) -> Result<(), Error> {
        self.file.set_len(n).map_err(|_| Error::Io)
    }

    fn sync(&mut self) -> Result<(), Error> {
        self.file.sync_all().map_err(|_| Error::Io)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn mem_storage_gap_and_bounds() {
        let mut s = MemStorage::new();
        assert!(s.is_empty().unwrap());
        s.write_at(10, b"hi").unwrap();
        assert_eq!(s.len().unwrap(), 12);
        let mut buf = [0u8; 12];
        s.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf[..10], &[0u8; 10]);
        assert_eq!(&buf[10..], b"hi");
        assert_eq!(s.read_at(11, &mut [0u8; 2]), Err(Error::Io));
        assert_eq!(s.read_at(u64::MAX, &mut [0u8; 1]), Err(Error::Io));
        s.set_len(5).unwrap();
        assert_eq!(s.len().unwrap(), 5);
    }
}
