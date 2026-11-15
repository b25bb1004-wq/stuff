//! Display info registers (`0x1000_3000`, size `0x1000`) and RGBA framebuffer (`0x3000_0000`).
use alloc::vec;
use alloc::vec::Vec;

use crate::Fault;

/// Display control MMIO window size.
pub(super) const DISPLAY_SIZE: u64 = 0x1000;

/// Display metadata and linear RGBA framebuffer.
#[derive(Debug, Clone)]
pub(super) struct Display {
    width: u32,
    height: u32,
    fb: Vec<u8>,
}

impl Display {
    /// Allocate a zeroed `width * height * 4` RGBA framebuffer.
    pub(super) fn new(width: u32, height: u32) -> Self {
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|px| px.checked_mul(4))
            .and_then(|b| usize::try_from(b).ok())
            .unwrap_or(0);
        Self {
            width,
            height,
            fb: vec![0; bytes],
        }
    }

    /// Borrow the RGBA pixel buffer.
    pub(super) fn framebuffer(&self) -> &[u8] {
        &self.fb
    }

    /// Size of the framebuffer in bytes.
    pub(super) fn fb_len(&self) -> u64 {
        self.fb.len() as u64
    }

    /// Read a display register at `off`.
    pub(super) fn read_reg(&self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > DISPLAY_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        let val = match (off, size) {
            (0x00, 4) => u64::from(self.width),
            (0x04, 4) => u64::from(self.height),
            _ => 0,
        };
        Ok(val)
    }

    /// Write a display register at `off` (all defined registers are read-only).
    pub(super) fn write_reg(&self, addr: u64, off: u64, size: u8) -> Result<(), Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > DISPLAY_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        Ok(())
    }

    /// Read `size` little-endian bytes from the framebuffer at `off`.
    pub(super) fn read_fb(&self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        let n = usize::from(size);
        let idx = usize::try_from(off).map_err(|_| Fault::Unmapped(addr))?;
        let slice = self
            .fb
            .get(idx..)
            .and_then(|s| s.get(..n))
            .ok_or(Fault::Unmapped(addr))?;
        Ok(slice
            .iter()
            .rev()
            .fold(0, |acc, &b| (acc << 8) | u64::from(b)))
    }

    /// Write `size` little-endian bytes into the framebuffer at `off`.
    pub(super) fn write_fb(
        &mut self,
        addr: u64,
        off: u64,
        size: u8,
        value: u64,
    ) -> Result<(), Fault> {
        let n = usize::from(size);
        let idx = usize::try_from(off).map_err(|_| Fault::Unmapped(addr))?;
        let slice = self
            .fb
            .get_mut(idx..)
            .and_then(|s| s.get_mut(..n))
            .ok_or(Fault::Unmapped(addr))?;
        for (i, b) in slice.iter_mut().enumerate() {
            *b = (value >> (8 * i)) as u8;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn display_regs_and_rgba_pixels() {
        let mut d = Display::new(4, 2);
        assert_eq!(d.read_reg(0x1000_3000, 0, 4), Ok(4));
        assert_eq!(d.read_reg(0x1000_3004, 4, 4), Ok(2));
        d.write_reg(0x1000_3000, 0, 4).unwrap();
        assert_eq!(d.read_reg(0x1000_3000, 0, 4), Ok(4));

        // Pixel (x=2, y=1) -> offset 4 * (1 * 4 + 2) = 24
        let off = 4 * (4 + 2);
        // Little-endian RGBA: R=0x11, G=0x22, B=0x33, A=0xFF
        d.write_fb(0x3000_0000 + off, off, 4, 0xFF33_2211).unwrap();
        assert_eq!(&d.framebuffer()[24..28], &[0x11, 0x22, 0x33, 0xFF]);
        assert_eq!(d.read_fb(0x3000_0000 + off, off, 4), Ok(0xFF33_2211));
        assert_eq!(
            d.read_fb(0x3000_0000 + 30, 30, 4),
            Err(Fault::Unmapped(0x3000_001E))
        );
    }
}
