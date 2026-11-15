//! Polling MMIO block device driver implementing `hades::BlockDevice` via a kernel bounce buffer.

use alloc::boxed::Box;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

use hades::{BlockDevice, Error, BLOCK};

use crate::vm::BLOCK_BASE;

const SECTOR_SIZE: usize = 512;
const SECTORS_PER_BLOCK: u64 = (BLOCK / SECTOR_SIZE) as u64;

const REG_SECTOR: u64 = BLOCK_BASE;
const REG_DMA_ADDR: u64 = BLOCK_BASE + 0x08;
const REG_COUNT: u64 = BLOCK_BASE + 0x10;
const REG_CMD: u64 = BLOCK_BASE + 0x14;
const REG_STATUS: u64 = BLOCK_BASE + 0x18;
const REG_DONE: u64 = BLOCK_BASE + 0x20;
const REG_CAPACITY: u64 = BLOCK_BASE + 0x28;

const CMD_READ: u32 = 1;
const CMD_WRITE: u32 = 2;

#[repr(C, align(8))]
struct BounceBuf([u8; BLOCK]);

/// Read the raw 512-byte sector capacity reported by the Silicon block device.
pub fn probe_sectors() -> u64 {
    // SAFETY: `REG_CAPACITY` (`0x1000_1028`) is the 64-bit MMIO capacity register on Silicon.
    unsafe { read_volatile(REG_CAPACITY as *const u64) }
}

/// Polling driver for the Silicon MMIO block device at `0x1000_1000`.
pub struct Blk {
    blocks: u64,
    bounce: Box<BounceBuf>,
}

impl Blk {
    /// Construct a new block driver instance with an 8-byte aligned DMA bounce buffer.
    pub fn new() -> Self {
        let sectors = probe_sectors();
        Self {
            blocks: sectors / SECTORS_PER_BLOCK,
            bounce: Box::new(BounceBuf([0u8; BLOCK])),
        }
    }

    fn transfer(&mut self, sector: u64, cmd: u32) -> bool {
        let dma_pa = self.bounce.0.as_ptr() as u64;
        // SAFETY: All registers are valid 32/64-bit MMIO registers in the identity-mapped block window,
        // and `dma_pa` points to a 4 KiB heap buffer in identity-mapped RAM (`RAM_BASE..ram_end`).
        unsafe {
            write_volatile(REG_DONE as *mut u32, 0);
            write_volatile(REG_SECTOR as *mut u64, sector);
            write_volatile(REG_DMA_ADDR as *mut u64, dma_pa);
            write_volatile(REG_COUNT as *mut u32, SECTORS_PER_BLOCK as u32);
            compiler_fence(Ordering::SeqCst);
            write_volatile(REG_CMD as *mut u32, cmd);
            while read_volatile(REG_DONE as *const u32) == 0 {
                core::hint::spin_loop();
            }
            compiler_fence(Ordering::SeqCst);
            let status = read_volatile(REG_STATUS as *const u32);
            write_volatile(REG_DONE as *mut u32, 0);
            status == 0
        }
    }
}

impl BlockDevice for Blk {
    fn block_count(&self) -> u64 {
        self.blocks
    }

    fn read(&mut self, block: u64, buf: &mut [u8; BLOCK]) -> Result<(), Error> {
        if block >= self.blocks {
            return Err(Error::Io);
        }
        let sector = block.checked_mul(SECTORS_PER_BLOCK).ok_or(Error::Io)?;
        if !self.transfer(sector, CMD_READ) {
            return Err(Error::Io);
        }
        buf.copy_from_slice(&self.bounce.0);
        Ok(())
    }

    fn write(&mut self, block: u64, buf: &[u8; BLOCK]) -> Result<(), Error> {
        if block >= self.blocks {
            return Err(Error::Io);
        }
        let sector = block.checked_mul(SECTORS_PER_BLOCK).ok_or(Error::Io)?;
        self.bounce.0.copy_from_slice(buf);
        if !self.transfer(sector, CMD_WRITE) {
            return Err(Error::Io);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
