//! VirtIO-style simplified block device at `0x1000_1000`, size `0x1000`, PLIC source 8.
use alloc::vec::Vec;

use crate::{Bus, Fault, Ram};

use super::RAM_BASE;

/// Block device MMIO window size.
pub(super) const BLOCK_SIZE: u64 = 0x1000;
/// Bytes per sector.
const SECTOR_SIZE: u64 = 512;

/// Block device with DMA against guest RAM.
#[derive(Debug, Clone, Default)]
pub(super) struct Block {
    disk: Vec<u8>,
    sector: u64,
    dma_addr: u64,
    count: u32,
    status: u32,
    irq_enable: u32,
    done: bool,
}

impl Block {
    /// Create a block device backed by `disk`.
    pub(super) fn new(disk: Vec<u8>) -> Self {
        Self {
            disk,
            ..Self::default()
        }
    }

    /// Borrow the raw disk image.
    pub(super) fn disk(&self) -> &[u8] {
        &self.disk
    }

    /// Interrupt line state: `irq_enable && done`.
    pub(super) fn irq_line(&self) -> bool {
        self.irq_enable != 0 && self.done
    }

    /// Total capacity in whole 512-byte sectors.
    fn capacity(&self) -> u64 {
        (self.disk.len() as u64) / SECTOR_SIZE
    }

    /// Execute a block command (`1` = disk -> RAM, `2` = RAM -> disk).
    fn execute(&mut self, cmd: u32, ram: &mut Ram, ram_size: usize) {
        if cmd != 1 && cmd != 2 {
            return;
        }
        let Some((disk_off, disk_end, byte_len)) = self.valid_ranges(ram_size) else {
            self.status = 1;
            self.done = true;
            return;
        };
        if cmd == 1 {
            if let Some(src) = self.disk.get(disk_off..disk_end) {
                self.status = u32::from(ram.load(self.dma_addr, src).is_err());
            } else {
                self.status = 1;
            }
        } else {
            let mut ok = true;
            if let Some(dst) = self.disk.get_mut(disk_off..disk_end) {
                for (i, chunk) in dst.as_chunks_mut::<8>().0.iter_mut().enumerate() {
                    let Some(addr) = (i as u64)
                        .checked_mul(8)
                        .and_then(|o| self.dma_addr.checked_add(o))
                    else {
                        ok = false;
                        break;
                    };
                    match ram.read(addr, 8) {
                        Ok(word) => *chunk = word.to_le_bytes(),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
            } else {
                ok = false;
            }
            let _ = byte_len;
            self.status = u32::from(!ok);
        }
        self.done = true;
    }

    /// Validate that both sector range and RAM DMA range are in bounds.
    fn valid_ranges(&self, ram_size: usize) -> Option<(usize, usize, usize)> {
        let end_sec = self.sector.checked_add(u64::from(self.count))?;
        if end_sec > self.capacity() {
            return None;
        }
        let byte_len = usize::try_from(u64::from(self.count).checked_mul(SECTOR_SIZE)?).ok()?;
        let disk_off = usize::try_from(self.sector.checked_mul(SECTOR_SIZE)?).ok()?;
        let disk_end = disk_off.checked_add(byte_len)?;
        if disk_end > self.disk.len() {
            return None;
        }
        let ram_off = usize::try_from(self.dma_addr.checked_sub(RAM_BASE)?).ok()?;
        let ram_end = ram_off.checked_add(byte_len)?;
        if ram_end > ram_size {
            return None;
        }
        Some((disk_off, disk_end, byte_len))
    }

    /// Read a block register at `off`.
    pub(super) fn read(&self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > BLOCK_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        let cap = self.capacity();
        let val = match (off, size) {
            (0x00, 8) => self.sector,
            (0x00, 4) => self.sector & 0xFFFF_FFFF,
            (0x04, 4) => self.sector >> 32,
            (0x08, 8) => self.dma_addr,
            (0x08, 4) => self.dma_addr & 0xFFFF_FFFF,
            (0x0C, 4) => self.dma_addr >> 32,
            (0x10, 4) => u64::from(self.count),
            (0x18, 4) => u64::from(self.status),
            (0x1C, 4) => u64::from(self.irq_enable),
            (0x20, 4) => u64::from(self.done),
            (0x28, 8) => cap,
            (0x28, 4) => cap & 0xFFFF_FFFF,
            (0x2C, 4) => cap >> 32,
            _ => 0,
        };
        Ok(val)
    }

    /// Write a block register at `off`.
    pub(super) fn write(
        &mut self,
        addr: u64,
        off: u64,
        size: u8,
        value: u64,
        ram: &mut Ram,
        ram_size: usize,
    ) -> Result<(), Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > BLOCK_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        match (off, size) {
            (0x00, 8) => self.sector = value,
            (0x00, 4) => self.sector = (self.sector & !0xFFFF_FFFF) | (value & 0xFFFF_FFFF),
            (0x04, 4) => self.sector = (self.sector & 0xFFFF_FFFF) | (value << 32),
            (0x08, 8) => self.dma_addr = value,
            (0x08, 4) => self.dma_addr = (self.dma_addr & !0xFFFF_FFFF) | (value & 0xFFFF_FFFF),
            (0x0C, 4) => self.dma_addr = (self.dma_addr & 0xFFFF_FFFF) | (value << 32),
            (0x10, 4) => self.count = value as u32,
            (0x14, 4) => self.execute(value as u32, ram, ram_size),
            (0x1C, 4) => self.irq_enable = value as u32,
            (0x20, 4) => self.done = false,
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn block_dma_read_write_and_errors() {
        let mut disk = alloc::vec![0u8; 1024];
        disk[512..520].copy_from_slice(b"MONOLITH");
        let mut blk = Block::new(disk);
        let ram_size = 4096;
        let mut ram = Ram::new(RAM_BASE, ram_size);

        assert_eq!(blk.read(0x1000_1028, 0x28, 8), Ok(2));
        assert!(!blk.irq_line());

        // Read sector 1 into RAM_BASE + 256
        blk.write(0x1000_101C, 0x1C, 4, 1, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1000, 0x00, 8, 1, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1008, 0x08, 8, RAM_BASE + 256, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1010, 0x10, 4, 1, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1014, 0x14, 4, 1, &mut ram, ram_size)
            .unwrap();

        assert_eq!(blk.read(0x1000_1018, 0x18, 4), Ok(0));
        assert_eq!(blk.read(0x1000_1020, 0x20, 4), Ok(1));
        assert!(blk.irq_line());
        assert_eq!(
            ram.read(RAM_BASE + 256, 8),
            Ok(u64::from_le_bytes(*b"MONOLITH"))
        );

        // Clear done
        blk.write(0x1000_1020, 0x20, 4, 0, &mut ram, ram_size)
            .unwrap();
        assert!(!blk.irq_line());
        assert_eq!(blk.read(0x1000_1020, 0x20, 4), Ok(0));

        // Write from RAM_BASE + 256 to sector 0
        blk.write(0x1000_1000, 0x00, 8, 0, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1014, 0x14, 4, 2, &mut ram, ram_size)
            .unwrap();
        assert_eq!(blk.read(0x1000_1018, 0x18, 4), Ok(0));
        assert_eq!(&blk.disk()[0..8], b"MONOLITH");

        // Out-of-range sector error leaves RAM untouched
        ram.write(RAM_BASE, 8, 0x1122_3344_5566_7788).unwrap();
        blk.write(0x1000_1000, 0x00, 8, 2, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1008, 0x08, 8, RAM_BASE, &mut ram, ram_size)
            .unwrap();
        blk.write(0x1000_1014, 0x14, 4, 1, &mut ram, ram_size)
            .unwrap();
        assert_eq!(blk.read(0x1000_1018, 0x18, 4), Ok(1));
        assert_eq!(blk.read(0x1000_1020, 0x20, 4), Ok(1));
        assert_eq!(ram.read(RAM_BASE, 8), Ok(0x1122_3344_5566_7788));

        // DMA past RAM end error leaves disk untouched
        blk.write(0x1000_1000, 0x00, 8, 0, &mut ram, ram_size)
            .unwrap();
        blk.write(
            0x1000_1008,
            0x08,
            8,
            RAM_BASE + 4096 - 256,
            &mut ram,
            ram_size,
        )
        .unwrap();
        blk.write(0x1000_1014, 0x14, 4, 2, &mut ram, ram_size)
            .unwrap();
        assert_eq!(blk.read(0x1000_1018, 0x18, 4), Ok(1));
        assert_eq!(&blk.disk()[0..8], b"MONOLITH");
    }
}
