//! Platform-Level Interrupt Controller (PLIC) at `0x0C00_0000`, size `0x400_0000`.
use crate::Fault;

/// PLIC window size.
pub(super) const PLIC_SIZE: u64 = 0x0400_0000;

const PENDING_OFF: u64 = 0x1000;
const ENABLE_CTX0_OFF: u64 = 0x2000;
const ENABLE_CTX1_OFF: u64 = 0x2080;
const THRESHOLD_CTX0_OFF: u64 = 0x20_0000;
const CLAIM_CTX0_OFF: u64 = 0x20_0004;
const THRESHOLD_CTX1_OFF: u64 = 0x20_1000;
const CLAIM_CTX1_OFF: u64 = 0x20_1004;

/// PLIC with sources 1..=31 and two contexts (0 = M, 1 = S).
#[derive(Debug, Clone, Default)]
pub(super) struct Plic {
    pub(super) priority: [u8; 32],
    pub(super) pending: u32,
    pub(super) in_flight: u32,
    pub(super) enable: [u32; 2],
    pub(super) threshold: [u32; 2],
}

impl Plic {
    /// Sample active interrupt lines (bit `id` = source `id` line high) via the level gateway.
    pub(super) fn sample_lines(&mut self, lines: u32) {
        let newly_pending = lines & !1 & !self.in_flight;
        self.pending |= newly_pending;
    }

    /// Return the highest-priority eligible pending source for `ctx`, or `None`.
    fn best_eligible(&self, ctx: usize) -> Option<u32> {
        let ready = self.pending & self.enable[ctx] & !1;
        if ready == 0 {
            return None;
        }
        let thresh = self.threshold[ctx];
        let mut best_id = 0u32;
        let mut best_prio = 0u32;
        for id in 1..32u32 {
            if (ready & (1 << id)) == 0 {
                continue;
            }
            let prio = u32::from(self.priority[id as usize]);
            if prio > thresh && prio > best_prio {
                best_prio = prio;
                best_id = id;
            }
        }
        (best_id != 0).then_some(best_id)
    }

    /// Whether `ctx` (0 = M, 1 = S) currently has an eligible pending source.
    pub(super) fn has_eligible(&self, ctx: usize) -> bool {
        self.best_eligible(ctx).is_some()
    }

    /// Claim the highest-priority eligible source for `ctx`, or return 0.
    fn claim(&mut self, ctx: usize) -> u32 {
        let Some(id) = self.best_eligible(ctx) else {
            return 0;
        };
        let mask = 1u32 << id;
        self.pending &= !mask;
        self.in_flight |= mask;
        id
    }

    /// Complete `id` if it is in flight and enabled in `ctx`; other writes are ignored.
    fn complete(&mut self, ctx: usize, id: u32) {
        if (1..=31).contains(&id) {
            let mask = 1u32 << id;
            if (self.in_flight & mask) != 0 && (self.enable[ctx] & mask) != 0 {
                self.in_flight &= !mask;
            }
        }
    }

    /// Read a 32-bit PLIC register at `off`.
    pub(super) fn read(&mut self, addr: u64, off: u64, size: u8) -> Result<u64, Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > PLIC_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if size != 4 {
            return Err(Fault::Unmapped(addr));
        }
        if off & 3 != 0 {
            return Err(Fault::Misaligned(addr));
        }
        let val = match off {
            0x0000 => 0,
            0x0004..=0x007C => u32::from(self.priority[(off / 4) as usize]),
            PENDING_OFF => self.pending,
            ENABLE_CTX0_OFF => self.enable[0],
            ENABLE_CTX1_OFF => self.enable[1],
            THRESHOLD_CTX0_OFF => self.threshold[0],
            CLAIM_CTX0_OFF => self.claim(0),
            THRESHOLD_CTX1_OFF => self.threshold[1],
            CLAIM_CTX1_OFF => self.claim(1),
            _ => 0,
        };
        Ok(u64::from(val))
    }

    /// Write a 32-bit PLIC register at `off`.
    pub(super) fn write(&mut self, addr: u64, off: u64, size: u8, value: u64) -> Result<(), Fault> {
        if off
            .checked_add(u64::from(size))
            .is_none_or(|e| e > PLIC_SIZE)
        {
            return Err(Fault::Unmapped(addr));
        }
        if size != 4 {
            return Err(Fault::Unmapped(addr));
        }
        if off & 3 != 0 {
            return Err(Fault::Misaligned(addr));
        }
        let v = value as u32;
        match off {
            0x0004..=0x007C => self.priority[(off / 4) as usize] = (v & 7) as u8,
            ENABLE_CTX0_OFF => self.enable[0] = v & !1,
            ENABLE_CTX1_OFF => self.enable[1] = v & !1,
            THRESHOLD_CTX0_OFF => self.threshold[0] = v,
            CLAIM_CTX0_OFF => self.complete(0, v),
            THRESHOLD_CTX1_OFF => self.threshold[1] = v,
            CLAIM_CTX1_OFF => self.complete(1, v),
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
    fn plic_priority_ties_threshold_and_inflight() {
        let mut p = Plic::default();
        // Other sizes -> Err
        assert_eq!(p.read(0x0C00_0004, 4, 1), Err(Fault::Unmapped(0x0C00_0004)));
        assert_eq!(
            p.write(0x0C00_0004, 4, 8, 1),
            Err(Fault::Unmapped(0x0C00_0004))
        );

        // Source 0 reads 0 and ignores writes
        p.write(0x0C00_0000, 0, 4, 7).unwrap();
        assert_eq!(p.read(0x0C00_0000, 0, 4), Ok(0));

        // Priority is 3 bits
        p.write(0x0C00_0004, 4, 4, 0xFF).unwrap();
        assert_eq!(p.read(0x0C00_0004, 4, 4), Ok(7));

        // Sources 5 and 3 both priority 4; source 8 priority 5
        p.write(0x0C00_0000 + 4 * 5, 4 * 5, 4, 4).unwrap();
        p.write(0x0C00_0000 + 4 * 3, 4 * 3, 4, 4).unwrap();
        p.write(0x0C00_0000 + 4 * 8, 4 * 8, 4, 5).unwrap();
        p.write(
            0x0C00_2000,
            ENABLE_CTX0_OFF,
            4,
            (1 << 3) | (1 << 5) | (1 << 8),
        )
        .unwrap();
        p.write(0x0C00_2080, ENABLE_CTX1_OFF, 4, 1 << 5).unwrap();

        p.sample_lines((1 << 3) | (1 << 5) | (1 << 8));
        assert!(p.has_eligible(0));
        assert!(p.has_eligible(1));

        // Highest priority first: 8
        assert_eq!(p.read(0x0C20_0004, CLAIM_CTX0_OFF, 4), Ok(8));
        // Tie between 3 and 5 (both prio 4): lowest id (3) wins
        assert_eq!(p.read(0x0C20_0004, CLAIM_CTX0_OFF, 4), Ok(3));

        // Threshold = 4 blocks source 5 (prio 4 is not > 4)
        p.write(0x0C20_0000, THRESHOLD_CTX0_OFF, 4, 4).unwrap();
        assert!(!p.has_eligible(0));
        assert_eq!(p.read(0x0C20_0004, CLAIM_CTX0_OFF, 4), Ok(0));
        p.write(0x0C20_0000, THRESHOLD_CTX0_OFF, 4, 3).unwrap();
        assert!(p.has_eligible(0));
        assert_eq!(p.read(0x0C20_0004, CLAIM_CTX0_OFF, 4), Ok(5));
        assert_eq!(p.read(0x0C00_1000, PENDING_OFF, 4), Ok(0));

        // While in flight, sampling line high again does not re-pend
        p.sample_lines(1 << 8);
        assert_eq!(p.read(0x0C00_1000, PENDING_OFF, 4), Ok(0));

        // Completing from wrong context (ctx 1 doesn't have 8 enabled) is ignored
        p.write(0x0C20_1004, CLAIM_CTX1_OFF, 4, 8).unwrap();
        p.sample_lines(1 << 8);
        assert_eq!(p.read(0x0C00_1000, PENDING_OFF, 4), Ok(0));

        // Completing from ctx 0 clears in-flight so next sample re-pends
        p.write(0x0C20_0004, CLAIM_CTX0_OFF, 4, 8).unwrap();
        p.sample_lines(1 << 8);
        assert_eq!(p.read(0x0C00_1000, PENDING_OFF, 4), Ok(1 << 8));
        assert_eq!(p.read(0x0C20_0004, CLAIM_CTX0_OFF, 4), Ok(8));
    }
}
