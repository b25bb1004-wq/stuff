//! 16550 UART subset at `0x1000_0000` (8 byte-wide registers, PLIC source 10).
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::Fault;

/// UART window size (8 byte registers).
pub(super) const UART_SIZE: u64 = 8;

/// 16550 UART subset.
#[derive(Debug, Clone, Default)]
pub(super) struct Uart {
    rx: VecDeque<u8>,
    tx: Vec<u8>,
    ier: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    in_bytes: u64,
    out_bytes: u64,
}

impl Uart {
    /// Queue input bytes for the guest to read from RBR.
    pub(super) fn push_input(&mut self, bytes: &[u8]) {
        self.rx.extend(bytes);
        self.in_bytes = self.in_bytes.saturating_add(bytes.len() as u64);
    }

    /// Host-to-guest and guest-to-host byte totals since the last reset.
    pub(super) fn counts(&self) -> (u64, u64) {
        (self.in_bytes, self.out_bytes)
    }

    /// Zero the byte totals.
    pub(super) fn reset_counts(&mut self) {
        self.in_bytes = 0;
        self.out_bytes = 0;
    }

    /// Drain bytes written by the guest to THR.
    pub(super) fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.tx)
    }

    /// Current UART interrupt line state: `(IER.0 && RX non-empty) || IER.1`.
    pub(super) fn irq_line(&self) -> bool {
        ((self.ier & 1) != 0 && !self.rx.is_empty()) || (self.ier & 2) != 0
    }

    /// Read one of the 8 UART byte registers.
    pub(super) fn read(&mut self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off >= UART_SIZE || size != 1 {
            return Err(Fault::Unmapped(addr));
        }
        let val = match off {
            0 => self.rx.pop_front().unwrap_or(0),
            1 => self.ier,
            2 => {
                if (self.ier & 1) != 0 && !self.rx.is_empty() {
                    0xC4
                } else if (self.ier & 2) != 0 {
                    0xC2
                } else {
                    0xC1
                }
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => 0x60 | u8::from(!self.rx.is_empty()),
            6 => 0,
            7 => self.scr,
            _ => 0,
        };
        Ok(u64::from(val))
    }

    /// Write one of the 8 UART byte registers.
    pub(super) fn write(&mut self, addr: u64, off: u64, size: u8, value: u64) -> Result<(), Fault> {
        if off >= UART_SIZE || size != 1 {
            return Err(Fault::Unmapped(addr));
        }
        let b = value as u8;
        match off {
            0 => {
                self.tx.push(b);
                self.out_bytes = self.out_bytes.saturating_add(1);
            }
            1 => self.ier = b,
            3 => self.lcr = b,
            4 => self.mcr = b,
            7 => self.scr = b,
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
    fn uart_lsr_iir_and_sizes() {
        let mut u = Uart::default();
        assert_eq!(u.read(0x1000_0000, 0, 4), Err(Fault::Unmapped(0x1000_0000)));
        assert_eq!(
            u.write(0x1000_0001, 1, 2, 1),
            Err(Fault::Unmapped(0x1000_0001))
        );

        assert_eq!(u.read(0x1000_0005, 5, 1), Ok(0x60));
        assert_eq!(u.read(0x1000_0002, 2, 1), Ok(0xC1));
        assert!(!u.irq_line());

        // Enable TX-empty interrupt
        u.write(0x1000_0001, 1, 1, 2).unwrap();
        assert_eq!(u.read(0x1000_0002, 2, 1), Ok(0xC2));
        assert!(u.irq_line());

        // Enable RX interrupt + push byte -> RX takes precedence in IIR
        u.write(0x1000_0001, 1, 1, 3).unwrap();
        u.push_input(b"AB");
        assert_eq!(u.read(0x1000_0005, 5, 1), Ok(0x61));
        assert_eq!(u.read(0x1000_0002, 2, 1), Ok(0xC4));
        assert_eq!(u.read(0x1000_0000, 0, 1), Ok(u64::from(b'A')));
        assert_eq!(u.read(0x1000_0000, 0, 1), Ok(u64::from(b'B')));
        assert_eq!(u.read(0x1000_0000, 0, 1), Ok(0));
        assert_eq!(u.read(0x1000_0005, 5, 1), Ok(0x60));
        assert_eq!(u.read(0x1000_0002, 2, 1), Ok(0xC2));

        // THR / LCR / MCR / SCR / MSR
        u.write(0x1000_0000, 0, 1, u64::from(b'Z')).unwrap();
        assert_eq!(u.take_output(), b"Z");
        u.write(0x1000_0003, 3, 1, 0x03).unwrap();
        u.write(0x1000_0004, 4, 1, 0x0B).unwrap();
        u.write(0x1000_0007, 7, 1, 0x5A).unwrap();
        assert_eq!(u.read(0x1000_0003, 3, 1), Ok(0x03));
        assert_eq!(u.read(0x1000_0004, 4, 1), Ok(0x0B));
        assert_eq!(u.read(0x1000_0006, 6, 1), Ok(0));
        assert_eq!(u.read(0x1000_0007, 7, 1), Ok(0x5A));
    }
}
