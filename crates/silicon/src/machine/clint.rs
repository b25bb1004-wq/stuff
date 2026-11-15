//! Core-Local Interruptor (CLINT) at `0x0200_0000`, size `0x10000`.
use crate::Fault;

/// CLINT window size.
pub(super) const CLINT_SIZE: u64 = 0x1_0000;

const MSIP_OFF: u64 = 0x0000;
const MTIMECMP_OFF: u64 = 0x4000;
const MTIME_OFF: u64 = 0xBFF8;

/// CLINT state: `msip`, `mtimecmp`, and `mtime`.
#[derive(Debug, Clone)]
pub(super) struct Clint {
    msip: bool,
    mtimecmp: u64,
    mtime: u64,
}

impl Default for Clint {
    fn default() -> Self {
        Self {
            msip: false,
            mtimecmp: u64::MAX,
            mtime: 0,
        }
    }
}

impl Clint {
    /// Advance `mtime` by one tick.
    pub(super) fn tick(&mut self) {
        self.mtime = self.mtime.wrapping_add(1);
    }

    /// Current `mtime` value.
    pub(super) fn mtime(&self) -> u64 {
        self.mtime
    }

    /// Whether the machine software interrupt is pending (`msip` bit 0).
    pub(super) fn msip(&self) -> bool {
        self.msip
    }

    /// Whether the machine timer interrupt is pending (`mtime >= mtimecmp`).
    pub(super) fn mtip(&self) -> bool {
        self.mtime >= self.mtimecmp
    }

    /// Read a register at `off` within the CLINT window.
    pub(super) fn read(&self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > CLINT_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        let val = match (off, size) {
            (MSIP_OFF, 4 | 8) => u64::from(self.msip),
            (MTIMECMP_OFF, 8) => self.mtimecmp,
            (MTIMECMP_OFF, 4) => self.mtimecmp & 0xFFFF_FFFF,
            (o, 4) if o == MTIMECMP_OFF + 4 => self.mtimecmp >> 32,
            (MTIME_OFF, 8) => self.mtime,
            (MTIME_OFF, 4) => self.mtime & 0xFFFF_FFFF,
            (o, 4) if o == MTIME_OFF + 4 => self.mtime >> 32,
            _ => 0,
        };
        Ok(val)
    }

    /// Write `value` to a register at `off` within the CLINT window.
    pub(super) fn write(&mut self, addr: u64, off: u64, size: u8, value: u64) -> Result<(), Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > CLINT_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        match (off, size) {
            (MSIP_OFF, 4 | 8) => self.msip = (value & 1) != 0,
            (MTIMECMP_OFF, 8) => self.mtimecmp = value,
            (MTIMECMP_OFF, 4) => {
                self.mtimecmp = (self.mtimecmp & !0xFFFF_FFFF) | (value & 0xFFFF_FFFF);
            }
            (o, 4) if o == MTIMECMP_OFF + 4 => {
                self.mtimecmp = (self.mtimecmp & 0xFFFF_FFFF) | (value << 32);
            }
            (MTIME_OFF, 8) => self.mtime = value,
            (MTIME_OFF, 4) => {
                self.mtime = (self.mtime & !0xFFFF_FFFF) | (value & 0xFFFF_FFFF);
            }
            (o, 4) if o == MTIME_OFF + 4 => {
                self.mtime = (self.mtime & 0xFFFF_FFFF) | (value << 32);
            }
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
    fn clint_halves_and_interrupts() {
        let mut c = Clint::default();
        assert!(!c.mtip());
        assert!(!c.msip());
        assert_eq!(c.read(0x0200_4000, MTIMECMP_OFF, 8), Ok(u64::MAX));

        // 4-byte writes to both halves of mtimecmp
        c.write(0x0200_4000, MTIMECMP_OFF, 4, 0x0000_0005).unwrap();
        c.write(0x0200_4004, MTIMECMP_OFF + 4, 4, 0x0000_0001)
            .unwrap();
        assert_eq!(
            c.read(0x0200_4000, MTIMECMP_OFF, 8),
            Ok(0x0000_0001_0000_0005)
        );
        assert_eq!(c.read(0x0200_4000, MTIMECMP_OFF, 4), Ok(5));
        assert_eq!(c.read(0x0200_4004, MTIMECMP_OFF + 4, 4), Ok(1));

        // 4-byte writes to both halves of mtime
        c.write(0x0200_BFF8, MTIME_OFF, 4, 0x0000_0004).unwrap();
        c.write(0x0200_BFFC, MTIME_OFF + 4, 4, 0x0000_0001).unwrap();
        assert_eq!(c.read(0x0200_BFF8, MTIME_OFF, 8), Ok(0x0000_0001_0000_0004));
        assert!(!c.mtip());

        c.tick();
        assert!(c.mtip());

        // msip bit 0
        c.write(0x0200_0000, MSIP_OFF, 4, 3).unwrap();
        assert!(c.msip());
        assert_eq!(c.read(0x0200_0000, MSIP_OFF, 4), Ok(1));
        c.write(0x0200_0000, MSIP_OFF, 4, 2).unwrap();
        assert!(!c.msip());

        // undefined offset reads 0 and ignores writes
        c.write(0x0200_0100, 0x100, 4, 0xDEAD_BEEF).unwrap();
        assert_eq!(c.read(0x0200_0100, 0x100, 4), Ok(0));
    }
}
