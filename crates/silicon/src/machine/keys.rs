//! Keys input FIFO device at `0x1000_2000`, size `0x1000`, PLIC source 11.
use alloc::collections::VecDeque;

use crate::Fault;

/// Keys MMIO window size.
pub(super) const KEYS_SIZE: u64 = 0x1000;
const MAX_KEYS: usize = 64;

/// 64-deep key FIFO.
#[derive(Debug, Clone, Default)]
pub(super) struct Keys {
    pub(super) fifo: VecDeque<u32>,
    pub(super) irq_enable: u32,
}

impl Keys {
    /// Push a key code if fewer than 64 are queued; extra keys are dropped.
    pub(super) fn push(&mut self, code: u32) {
        if self.fifo.len() < MAX_KEYS {
            self.fifo.push_back(code);
        }
    }

    /// Interrupt line state: `irq_enable && count > 0`.
    pub(super) fn irq_line(&self) -> bool {
        self.irq_enable != 0 && !self.fifo.is_empty()
    }

    /// Read a keys register at `off`.
    pub(super) fn read(&mut self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > KEYS_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        if size != 4 {
            return Ok(0);
        }
        let val = match off {
            0x00 => self.fifo.len() as u64,
            0x04 => u64::from(self.fifo.pop_front().unwrap_or(0)),
            0x08 => u64::from(self.irq_enable),
            _ => 0,
        };
        Ok(val)
    }

    /// Write a keys register at `off`.
    pub(super) fn write(&mut self, addr: u64, off: u64, size: u8, value: u64) -> Result<(), Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > KEYS_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        if size == 4 && off == 0x08 {
            self.irq_enable = value as u32;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn keys_fifo_capacity_and_irq() {
        let mut k = Keys::default();
        assert!(!k.irq_line());
        assert_eq!(k.read(0x1000_2000, 0, 4), Ok(0));
        assert_eq!(k.read(0x1000_2004, 4, 4), Ok(0));

        for i in 1..=70u32 {
            k.push(i);
        }
        assert_eq!(k.read(0x1000_2000, 0, 4), Ok(64));

        k.write(0x1000_2008, 8, 4, 1).unwrap();
        assert!(k.irq_line());

        for i in 1..=64u32 {
            assert_eq!(k.read(0x1000_2004, 4, 4), Ok(u64::from(i)));
        }
        assert_eq!(k.read(0x1000_2000, 0, 4), Ok(0));
        assert_eq!(k.read(0x1000_2004, 4, 4), Ok(0));
        assert!(!k.irq_line());
    }
}
