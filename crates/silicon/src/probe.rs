//! Counters for traps, ECALLs and device traffic. Each update sits on a trap or device path,
//! so the per-instruction loop never touches them.
use alloc::vec::Vec;

/// Number of ECALL records kept.
pub const ECALL_RING: usize = 64;

/// One ECALL trap: the `a7` register and the retired-instruction count at the trap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EcallRecord {
    /// Value of `a7` (the SBI extension or syscall number) when the ECALL trapped.
    pub a7: u64,
    /// Instructions retired (`instret`) when the ECALL trapped.
    pub instret: u64,
}

/// Cumulative counters since the last `Machine::probe_reset` (or since construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Exceptions taken, indexed by mcause code (codes 0 to 23 occur).
    pub exceptions: [u64; 32],
    /// Interrupts taken, indexed by interrupt cause code (codes 1 to 11 occur).
    pub interrupts: [u64; 32],
    /// Bytes the host queued into the UART RX FIFO with `uart_input`.
    pub uart_in: u64,
    /// Bytes the guest wrote to the UART transmit register (THR).
    pub uart_out: u64,
    /// Block commands accepted as disk-to-RAM reads (command 1).
    pub block_reads: u64,
    /// Block commands accepted as RAM-to-disk writes (command 2).
    pub block_writes: u64,
    ecall_ring: [EcallRecord; ECALL_RING],
    ecall_next: usize,
    ecall_total: u64,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            exceptions: [0; 32],
            interrupts: [0; 32],
            uart_in: 0,
            uart_out: 0,
            block_reads: 0,
            block_writes: 0,
            ecall_ring: [EcallRecord::default(); ECALL_RING],
            ecall_next: 0,
            ecall_total: 0,
        }
    }
}

impl Probe {
    /// Number of ECALLs taken since the last reset; the ring keeps only the last 64.
    pub fn ecall_total(&self) -> u64 {
        self.ecall_total
    }

    /// The last 64 ECALL records, oldest first.
    pub fn ecalls(&self) -> Vec<EcallRecord> {
        let n = self.ecall_total.min(ECALL_RING as u64) as usize;
        (0..n)
            .filter_map(|i| {
                let idx = (self.ecall_next + ECALL_RING - n + i) % ECALL_RING;
                self.ecall_ring.get(idx).copied()
            })
            .collect()
    }

    /// Count one trap with mcause or interrupt `code`; codes are masked to five bits.
    pub(crate) fn count_trap(&mut self, code: u64, interrupt: bool) {
        let idx = (code & 31) as usize;
        let table = if interrupt {
            &mut self.interrupts
        } else {
            &mut self.exceptions
        };
        if let Some(slot) = table.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
    }

    /// Append one ECALL record, overwriting the oldest once the ring is full.
    pub(crate) fn record_ecall(&mut self, a7: u64, instret: u64) {
        if let Some(slot) = self.ecall_ring.get_mut(self.ecall_next) {
            *slot = EcallRecord { a7, instret };
        }
        self.ecall_next = (self.ecall_next + 1) % ECALL_RING;
        self.ecall_total = self.ecall_total.saturating_add(1);
    }
}

/// Trap counters kept inside `Cpu`. They are not architectural state, so `Cpu` equality ignores them.
#[derive(Debug, Clone, Default)]
pub(crate) struct HartCounts(pub(crate) Probe);

impl PartialEq for HartCounts {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for HartCounts {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn exception_and_interrupt_counters_are_separate() {
        let mut p = Probe::default();
        p.count_trap(11, false);
        p.count_trap(11, false);
        p.count_trap(7, true);
        assert_eq!(p.exceptions[11], 2);
        assert_eq!(p.interrupts[7], 1);
        assert_eq!(p.exceptions[7], 0);
        assert_eq!(p.interrupts[11], 0);
    }

    #[test]
    fn ecall_ring_keeps_last_64_oldest_first() {
        let mut p = Probe::default();
        assert!(p.ecalls().is_empty());
        for i in 0..70u64 {
            p.record_ecall(i, 1000 + i);
        }
        assert_eq!(p.ecall_total(), 70);
        let recs = p.ecalls();
        assert_eq!(recs.len(), ECALL_RING);
        assert_eq!(
            recs.first(),
            Some(&EcallRecord {
                a7: 6,
                instret: 1006
            })
        );
        assert_eq!(
            recs.last(),
            Some(&EcallRecord {
                a7: 69,
                instret: 1069
            })
        );
    }

    #[test]
    fn hart_counts_do_not_affect_equality() {
        let mut a = HartCounts::default();
        let b = HartCounts::default();
        a.0.count_trap(2, false);
        assert_eq!(a, b);
    }
}
