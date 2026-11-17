//! Door device at `0x1000_4000`, size `0x1000`, PLIC source 12: a frame channel between host and guest.
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::{Fault, Ram};

use super::RAM_BASE;

/// Door MMIO window size.
pub(super) const DOOR_SIZE: u64 = 0x1000;
/// Frames the host may queue for the guest; further pushes are dropped and counted.
pub(super) const INBOX_CAP: usize = 64;
/// Frames the guest may leave undrained; TX_GO beyond that sets TX_ERR.
pub(super) const OUTBOX_CAP: usize = 64;
/// Largest frame accepted in either direction, in bytes.
pub(super) const MAX_FRAME: u64 = 1 << 20;

/// One channel-tagged frame.
pub(super) type Frame = (u8, Vec<u8>);

/// Frame door: guest TX copies RAM into the outbox, host frames wait in the inbox for RX.
#[derive(Debug, Clone, Default)]
pub(super) struct Door {
    pub(super) tx_addr: u64,
    pub(super) tx_len: u64,
    pub(super) tx_chan: u64,
    pub(super) tx_err: bool,
    pub(super) rx_addr: u64,
    pub(super) rx_cap: u64,
    /// Set by a failed RX_TAKE; cleared by a good take or a write to RX_ADDR or RX_CAP.
    pub(super) rx_fail: bool,
    pub(super) irq_enable: u64,
    pub(super) inbox: VecDeque<Frame>,
    pub(super) outbox: Vec<Frame>,
    pub(super) dropped: u64,
}

/// Keep the low 32 bits for a 4-byte access, the whole register for an 8-byte one.
fn word(reg: u64, size: u8) -> u64 {
    if size == 8 {
        reg
    } else {
        reg & 0xFFFF_FFFF
    }
}

/// Store `value` the way a `size`-byte write lands on a 64-bit register.
fn store(reg: &mut u64, size: u8, value: u64) {
    *reg = if size == 8 {
        value
    } else {
        (*reg & !0xFFFF_FFFF) | (value & 0xFFFF_FFFF)
    };
}

/// Reject accesses that leave the window or are misaligned for their size.
fn check_access(addr: u64, off: u64, size: u8) -> Result<(), Fault> {
    if off
        .checked_add(u64::from(size))
        .is_none_or(|e| e > DOOR_SIZE)
    {
        return Err(Fault::Unmapped(addr));
    }
    if off & u64::from(size).saturating_sub(1) != 0 {
        return Err(Fault::Misaligned(addr));
    }
    Ok(())
}

/// `(start, end)` of guest RAM bytes `[addr, addr + len)`, or `None` if they are not all in RAM.
fn ram_span(ram: &Ram, addr: u64, len: u64) -> Option<(usize, usize)> {
    let start = usize::try_from(addr.checked_sub(RAM_BASE)?).ok()?;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    (end <= ram.bytes().len()).then_some((start, end))
}

impl Door {
    /// Queue a host frame for the guest. Empty or oversize frames and a full inbox are dropped and counted.
    pub(super) fn push_to_guest(&mut self, channel: u8, bytes: &[u8]) {
        let len = bytes.len() as u64;
        if self.inbox.len() >= INBOX_CAP || !(1..=MAX_FRAME).contains(&len) {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.inbox.push_back((channel, bytes.to_vec()));
        }
    }

    /// Drain the guest-to-host frames in order.
    pub(super) fn take_outbox(&mut self) -> Vec<Frame> {
        core::mem::take(&mut self.outbox)
    }

    /// Interrupt line for PLIC source 12: `irq_enable != 0 && inbox not empty`.
    pub(super) fn irq_line(&self) -> bool {
        self.irq_enable != 0 && !self.inbox.is_empty()
    }

    /// STATUS: bit 0 RX_READY, bit 1 TX_ERR, bit 2 RX_TOO_BIG.
    fn status(&self) -> u64 {
        let too_big = self.rx_fail
            || self
                .inbox
                .front()
                .is_some_and(|(_, b)| b.len() as u64 > self.rx_cap);
        u64::from(!self.inbox.is_empty())
            | (u64::from(self.tx_err) << 1)
            | (u64::from(too_big) << 2)
    }

    /// Read a door register at `off`.
    pub(super) fn read(&self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        check_access(addr, off, size)?;
        let front = self.inbox.front();
        let val = match (off, size) {
            (0x00, 4 | 8) => word(self.tx_addr, size),
            (0x08, 4 | 8) => word(self.tx_len, size),
            (0x10, 4 | 8) => word(self.tx_chan, size),
            (0x20, 4 | 8) => word(self.rx_addr, size),
            (0x28, 4 | 8) => word(self.rx_cap, size),
            (0x30, 4 | 8) => self.status(),
            (0x38, 4 | 8) => word(front.map_or(0, |(_, b)| b.len() as u64), size),
            (0x40, 4 | 8) => word(front.map_or(0, |(c, _)| u64::from(*c)), size),
            (0x50, 4 | 8) => word(self.irq_enable, size),
            _ => 0,
        };
        Ok(val)
    }

    /// Write a door register at `off`. Bad guest pointers and lengths set flags; they never panic.
    pub(super) fn write(
        &mut self,
        addr: u64,
        off: u64,
        size: u8,
        value: u64,
        ram: &mut Ram,
    ) -> Result<(), Fault> {
        check_access(addr, off, size)?;
        match (off, size) {
            (0x00, 4 | 8) => store(&mut self.tx_addr, size, value),
            (0x08, 4 | 8) => store(&mut self.tx_len, size, value),
            (0x10, 4 | 8) => store(&mut self.tx_chan, size, value),
            (0x18, 4 | 8) => self.tx_go(ram),
            (0x20, 4 | 8) => {
                store(&mut self.rx_addr, size, value);
                self.rx_fail = false;
            }
            (0x28, 4 | 8) => {
                store(&mut self.rx_cap, size, value);
                self.rx_fail = false;
            }
            (0x48, 4 | 8) => self.rx_take(ram),
            (0x50, 4 | 8) => store(&mut self.irq_enable, size, value),
            _ => {}
        }
        Ok(())
    }

    /// TX_GO: copy TX_LEN bytes at TX_ADDR into the outbox. Bad channel, length or range sets TX_ERR.
    fn tx_go(&mut self, ram: &Ram) {
        let chan = u8::try_from(self.tx_chan).ok();
        let data = if (1..=MAX_FRAME).contains(&self.tx_len) {
            ram_span(ram, self.tx_addr, self.tx_len)
                .and_then(|(s, e)| ram.bytes().get(s..e))
                .map(<[u8]>::to_vec)
        } else {
            None
        };
        match (chan, data) {
            (Some(c), Some(d)) if self.outbox.len() < OUTBOX_CAP => {
                self.outbox.push((c, d));
                self.tx_err = false;
            }
            _ => self.tx_err = true,
        }
    }

    /// RX_TAKE: copy the front frame to RX_ADDR and pop it. A frame over RX_CAP or outside RAM stays queued.
    fn rx_take(&mut self, ram: &mut Ram) {
        let Some((_, frame)) = self.inbox.front() else {
            return;
        };
        let len = frame.len() as u64;
        let span = if len <= self.rx_cap {
            ram_span(ram, self.rx_addr, len)
        } else {
            None
        };
        let Some((s, e)) = span else {
            self.rx_fail = true;
            return;
        };
        let Some(dst) = ram.bytes_mut().get_mut(s..e) else {
            self.rx_fail = true;
            return;
        };
        dst.copy_from_slice(frame);
        self.inbox.pop_front();
        self.rx_fail = false;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::super::{Board, Config};
    use super::*;
    use crate::Bus;

    const RAM: u64 = 0x8000_0000;
    const DOOR: u64 = 0x1000_4000;

    fn board() -> Board {
        Board::new(Config {
            ram_size: 64 * 1024,
            ..Config::default()
        })
    }

    #[test]
    fn tx_round_trip_and_bad_range() {
        let mut b = board();
        b.ram.load(RAM + 0x100, b"hello").unwrap();
        b.write(DOOR, 8, RAM + 0x100).unwrap();
        b.write(DOOR + 0x08, 8, 5).unwrap();
        b.write(DOOR + 0x10, 8, 7).unwrap();
        b.write(DOOR + 0x18, 8, 1).unwrap();
        assert_eq!(b.read(DOOR + 0x30, 4), Ok(0));
        assert_eq!(b.door.take_outbox(), alloc::vec![(7u8, b"hello".to_vec())]);

        // Range runs past RAM end, then zero and oversize lengths: TX_ERR, nothing queued.
        for (addr, len) in [(RAM + 64 * 1024 - 2, 5), (RAM, 0), (RAM, MAX_FRAME + 1)] {
            b.write(DOOR, 8, addr).unwrap();
            b.write(DOOR + 0x08, 8, len).unwrap();
            b.write(DOOR + 0x18, 8, 1).unwrap();
            assert_eq!(b.read(DOOR + 0x30, 4), Ok(2));
            assert!(b.door.take_outbox().is_empty());
        }
    }

    #[test]
    fn rx_take_cap_and_range() {
        let mut d = Door::default();
        let mut ram = Ram::new(RAM, 4096);
        d.push_to_guest(3, b"abc");
        d.push_to_guest(4, b"defg");
        // RX_CAP starts at 0, so the pending frame is too big until a cap is set.
        assert_eq!(d.read(DOOR, 0x30, 8), Ok(5));
        d.write(DOOR, 0x28, 8, 8, &mut ram).unwrap();
        assert_eq!(d.read(DOOR, 0x30, 8), Ok(1));
        assert_eq!(d.read(DOOR, 0x38, 8), Ok(3));
        assert_eq!(d.read(DOOR, 0x40, 8), Ok(3));

        // Cap 2 is too small for "abc": bit 2 set, frame stays queued.
        d.write(DOOR, 0x28, 8, 2, &mut ram).unwrap();
        d.write(DOOR, 0x20, 8, RAM + 0x200, &mut ram).unwrap();
        d.write(DOOR, 0x48, 8, 1, &mut ram).unwrap();
        assert_eq!(d.read(DOOR, 0x30, 8), Ok(5));
        assert_eq!(d.read(DOOR, 0x38, 8), Ok(3));

        // Cap 3 fits: frame lands in RAM and pops, then the second frame shows.
        d.write(DOOR, 0x28, 4, 3, &mut ram).unwrap();
        assert_eq!(d.read(DOOR, 0x30, 8), Ok(1));
        d.write(DOOR, 0x48, 8, 1, &mut ram).unwrap();
        assert_eq!(ram.read(RAM + 0x200, 1), Ok(u64::from(b'a')));
        assert_eq!(d.read(DOOR, 0x38, 8), Ok(4));

        // Outside RAM: bit 2 set, nothing popped.
        d.write(DOOR, 0x20, 8, RAM + 4096, &mut ram).unwrap();
        d.write(DOOR, 0x48, 8, 1, &mut ram).unwrap();
        assert_eq!(d.read(DOOR, 0x30, 8), Ok(5));
        assert_eq!(d.read(DOOR, 0x38, 8), Ok(4));
    }

    #[test]
    fn outbox_cap_sets_tx_err() {
        let mut ram = Ram::new(RAM, 4096);
        let mut d = Door::default();
        d.write(DOOR, 0x00, 8, RAM, &mut ram).unwrap();
        d.write(DOOR, 0x08, 8, 1, &mut ram).unwrap();
        for _ in 0..OUTBOX_CAP {
            d.write(DOOR, 0x18, 8, 1, &mut ram).unwrap();
            assert!(!d.tx_err);
        }
        d.write(DOOR, 0x18, 8, 1, &mut ram).unwrap();
        assert!(d.tx_err);
        assert_eq!(d.take_outbox().len(), OUTBOX_CAP);
    }

    #[test]
    fn inbox_cap_drops_and_counts() {
        let mut d = Door::default();
        for i in 0..65u8 {
            d.push_to_guest(0, &[i]);
        }
        d.push_to_guest(0, &[]);
        assert_eq!(d.inbox.len(), INBOX_CAP);
        assert_eq!(d.dropped, 2);
    }

    #[test]
    fn irq_line_follows_enable_and_inbox() {
        let mut b = board();
        assert_eq!((b.device_lines() >> 12) & 1, 0);
        b.write(DOOR + 0x50, 8, 1).unwrap();
        assert_eq!((b.device_lines() >> 12) & 1, 0);
        b.door.push_to_guest(1, b"x");
        assert_eq!((b.device_lines() >> 12) & 1, 1);
    }
}
