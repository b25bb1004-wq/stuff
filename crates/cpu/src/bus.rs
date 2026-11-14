//! The memory bus seen by the cpu, and the RAM device that backs it.
use alloc::vec;
use alloc::vec::Vec;

/// A memory access the bus refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// No device maps the address, or the access runs past the end of the device.
    Unmapped(u64),
    /// The address is not aligned as the device requires.
    Misaligned(u64),
}

/// Memory as the cpu sees it. Sizes are 1, 2, 4 or 8 bytes, little endian.
pub trait Bus {
    /// Read `size` bytes at `addr`, zero-extended into the result.
    fn read(&mut self, addr: u64, size: u8) -> Result<u64, Fault>;
    /// Write the low `size` bytes of `value` at `addr`.
    fn write(&mut self, addr: u64, size: u8, value: u64) -> Result<(), Fault>;
}

/// Byte-addressed RAM mapped at `base`.
#[derive(Debug, Clone)]
pub struct Ram {
    base: u64,
    mem: Vec<u8>,
}

impl Ram {
    /// Zeroed RAM of `size` bytes mapped at `base`.
    pub fn new(base: u64, size: usize) -> Ram {
        Ram {
            base,
            mem: vec![0; size],
        }
    }

    /// Copy `data` into memory at `addr`; fails unless all of it is inside the RAM.
    pub fn load(&mut self, addr: u64, data: &[u8]) -> Result<(), Fault> {
        let off = self.offset(addr).ok_or(Fault::Unmapped(addr))?;
        let dst = self
            .mem
            .get_mut(off..)
            .and_then(|s| s.get_mut(..data.len()))
            .ok_or(Fault::Unmapped(addr))?;
        dst.copy_from_slice(data);
        Ok(())
    }

    /// Index of `addr` in the backing store, if it lies at or above `base`.
    fn offset(&self, addr: u64) -> Option<usize> {
        usize::try_from(addr.checked_sub(self.base)?).ok()
    }
}

/// Number of bytes for a legal access size.
fn access_len(size: u8) -> Option<usize> {
    match size {
        1 | 2 | 4 | 8 => Some(usize::from(size)),
        _ => None,
    }
}

impl Bus for Ram {
    fn read(&mut self, addr: u64, size: u8) -> Result<u64, Fault> {
        let n = access_len(size).ok_or(Fault::Unmapped(addr))?;
        let off = self.offset(addr).ok_or(Fault::Unmapped(addr))?;
        let bytes = self
            .mem
            .get(off..)
            .and_then(|s| s.get(..n))
            .ok_or(Fault::Unmapped(addr))?;
        Ok(bytes
            .iter()
            .rev()
            .fold(0, |acc, &b| (acc << 8) | u64::from(b)))
    }

    fn write(&mut self, addr: u64, size: u8, value: u64) -> Result<(), Fault> {
        let n = access_len(size).ok_or(Fault::Unmapped(addr))?;
        let off = self.offset(addr).ok_or(Fault::Unmapped(addr))?;
        let dst = self
            .mem
            .get_mut(off..)
            .and_then(|s| s.get_mut(..n))
            .ok_or(Fault::Unmapped(addr))?;
        for (i, b) in dst.iter_mut().enumerate() {
            *b = (value >> (8 * i)) as u8;
        }
        Ok(())
    }
}
