//! Block devices: the trait the file system runs on, and an in-memory device.
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use crate::{Error, BLOCK};

/// A device of fixed 4096-byte blocks.
pub trait BlockDevice {
    /// Number of blocks on the device.
    fn block_count(&self) -> u64;
    /// Read block `block` into `buf`. Io if the block does not exist.
    fn read(&mut self, block: u64, buf: &mut [u8; BLOCK]) -> Result<(), Error>;
    /// Write `buf` to block `block`.
    fn write(&mut self, block: u64, buf: &[u8; BLOCK]) -> Result<(), Error>;
    /// Make all earlier writes durable.
    fn flush(&mut self) -> Result<(), Error>;
}

/// A RAM-backed device, zeroed on creation.
pub struct MemDisk {
    data: Vec<u8>,
}

impl MemDisk {
    /// A zeroed device of `blocks` blocks.
    pub fn new(blocks: u64) -> MemDisk {
        let len = usize::try_from(blocks)
            .ok()
            .and_then(|b| b.checked_mul(BLOCK))
            .unwrap_or(0);
        MemDisk { data: vec![0; len] }
    }

    /// The raw bytes of the device.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// The raw bytes of the device, mutable (for corruption tests and image tools).
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    fn span(&self, block: u64) -> Option<Range<usize>> {
        let start = usize::try_from(block).ok()?.checked_mul(BLOCK)?;
        let end = start.checked_add(BLOCK)?;
        (end <= self.data.len()).then_some(start..end)
    }
}

impl BlockDevice for MemDisk {
    fn block_count(&self) -> u64 {
        (self.data.len() / BLOCK) as u64
    }

    fn read(&mut self, block: u64, buf: &mut [u8; BLOCK]) -> Result<(), Error> {
        let range = self.span(block).ok_or(Error::Io)?;
        let src = self.data.get(range).ok_or(Error::Io)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write(&mut self, block: u64, buf: &[u8; BLOCK]) -> Result<(), Error> {
        let range = self.span(block).ok_or(Error::Io)?;
        let dst = self.data.get_mut(range).ok_or(Error::Io)?;
        dst.copy_from_slice(buf);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
