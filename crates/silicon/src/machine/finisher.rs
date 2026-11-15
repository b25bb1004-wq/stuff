//! SiFive test finisher device (`0x0010_0000`, 4 bytes).
use crate::{Bus, Fault};

use super::Exit;

/// Test finisher MMIO device.
#[derive(Debug, Clone, Default)]
pub(super) struct Finisher {
    exit: Option<Exit>,
}

impl Finisher {
    /// Return the recorded exit status, if the guest has written pass or fail.
    pub(super) fn exit(&self) -> Option<Exit> {
        self.exit
    }

    /// Read from `offset` within the 4-byte finisher window.
    pub(super) fn read(&self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off.checked_add(u64::from(size)).is_none_or(|e| e > 4) {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        Ok(0)
    }

    /// Write to `offset` within the 4-byte finisher window.
    pub(super) fn write(&mut self, addr: u64, off: u64, size: u8, value: u64) -> Result<(), Fault> {
        if off.checked_add(u64::from(size)).is_none_or(|e| e > 4) {
            return Err(Fault::Unmapped(addr));
        }
        if off & (u64::from(size) - 1) != 0 {
            return Err(Fault::Misaligned(addr));
        }
        if off == 0 && size == 4 {
            let v = value as u32;
            if v == 0x5555 {
                self.exit = Some(Exit::Pass);
            } else if v & 0xFFFF == 0x3333 {
                self.exit = Some(Exit::Fail(v >> 16));
            }
        }
        Ok(())
    }
}

impl Bus for Finisher {
    fn read(&mut self, _addr: u64, _size: u8) -> Result<u64, Fault> {
        Ok(0)
    }

    fn write(&mut self, _addr: u64, _size: u8, _value: u64) -> Result<(), Fault> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn finisher_pass_fail_and_bounds() {
        let mut f = Finisher::default();
        assert_eq!(f.exit(), None);
        f.write(0x0010_0000, 0, 4, 0x1234).unwrap();
        assert_eq!(f.exit(), None);
        f.write(0x0010_0000, 0, 4, 0x5555).unwrap();
        assert_eq!(f.exit(), Some(Exit::Pass));
        f.write(0x0010_0000, 0, 4, (42 << 16) | 0x3333).unwrap();
        assert_eq!(f.exit(), Some(Exit::Fail(42)));
        assert_eq!(f.read(0x0010_0000, 0, 4), Ok(0));
        assert_eq!(f.read(0x0010_0002, 2, 4), Err(Fault::Unmapped(0x0010_0002)));
    }
}
