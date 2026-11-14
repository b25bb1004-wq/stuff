//! Trap entry, interrupt selection and the full-hart `tick`.
use crate::bus::Bus;
use crate::csr::{MIE, MPIE, MPP, SIE, SPIE, SPP};
use crate::{Cpu, Mode, Step, Trap};

/// Interrupt cause codes in priority order: MEI, MSI, MTI, SEI, SSI, STI.
const PRIORITY: [u64; 6] = [11, 3, 7, 9, 1, 5];

/// Bit `n` of `v`; false for `n` out of range.
fn bit(v: u64, n: u64) -> bool {
    v.checked_shr(n as u32).is_some_and(|x| x & 1 == 1)
}

/// Trap handler address for a vector base: vectored mode adds 4 * cause for interrupts.
fn vector(tvec: u64, code: u64, interrupt: bool) -> u64 {
    let base = tvec & !3;
    if interrupt && tvec & 3 == 1 {
        base.wrapping_add(code.wrapping_mul(4))
    } else {
        base
    }
}

impl Trap {
    /// The `(mcause code, mtval)` pair for this trap taken in `mode`. Ebreak gives tval 0 here;
    /// the hart stores its pc in mtval or stval.
    pub fn cause(&self, mode: Mode) -> (u64, u64) {
        match *self {
            Trap::IllegalInstruction(w) => (2, u64::from(w)),
            Trap::Ecall => (
                match mode {
                    Mode::User => 8,
                    Mode::Supervisor => 9,
                    Mode::Machine => 11,
                },
                0,
            ),
            Trap::Ebreak => (3, 0),
            Trap::FetchMisaligned(a) => (0, a),
            Trap::FetchFault(a) => (1, a),
            Trap::LoadMisaligned(a) => (4, a),
            Trap::LoadFault(a) => (5, a),
            Trap::StoreMisaligned(a) => (6, a),
            Trap::StoreFault(a) => (7, a),
            Trap::InstPageFault(a) => (12, a),
            Trap::LoadPageFault(a) => (13, a),
            Trap::StorePageFault(a) => (15, a),
        }
    }
}

impl Cpu {
    /// One step of the whole hart. It takes a pending enabled interrupt, or else executes one
    /// instruction and takes its trap. A trap or interrupt goes to the handler, so the pc
    /// changes; `Step` reports what happened.
    pub fn tick(&mut self, bus: &mut impl Bus) -> Step {
        if let Some(code) = self.pending_interrupt() {
            self.enter(code, true, 0);
            return Step::Interrupt(code);
        }
        match self.step_inner(bus) {
            Ok(true) if self.csr.mip & self.csr.mie == 0 => Step::Wfi,
            Ok(_) => Step::Retired,
            Err(trap) => {
                let (code, tval) = trap.cause(self.mode);
                let tval = if trap == Trap::Ebreak { self.pc } else { tval };
                self.enter(code, false, tval);
                Step::Exception(trap)
            }
        }
    }

    /// The interrupt to take now, if any. M-destined interrupts come before S-destined ones.
    fn pending_interrupt(&self) -> Option<u64> {
        let pend = self.csr.mip & self.csr.mie;
        if pend == 0 {
            return None;
        }
        let st = self.csr.mstatus;
        let m_on = self.mode != Mode::Machine || (st & MIE) != 0;
        let s_on = self.mode == Mode::User || (self.mode == Mode::Supervisor && (st & SIE) != 0);
        let deleg = self.csr.mideleg;
        let to_m = PRIORITY
            .iter()
            .copied()
            .find(|&c| m_on && bit(pend, c) && !bit(deleg, c));
        let to_s = PRIORITY
            .iter()
            .copied()
            .find(|&c| s_on && bit(pend, c) && bit(deleg, c));
        to_m.or(to_s)
    }

    /// Take a trap (or interrupt, with `interrupt` set) with cause `code` into its handler.
    /// Delegated traps from S or U go to S-mode; everything else goes to M-mode.
    fn enter(&mut self, code: u64, interrupt: bool, tval: u64) {
        self.reservation = None;
        let deleg = if interrupt {
            self.csr.mideleg
        } else {
            self.csr.medeleg
        };
        let cause = if interrupt { (1 << 63) | code } else { code };
        let epc = self.pc;
        let old = self.mode;
        let st = self.csr.mstatus;
        if bit(deleg, code) && old != Mode::Machine {
            self.csr.sepc = epc;
            self.csr.scause = cause;
            self.csr.stval = tval;
            // SPIE takes SIE (bit 1 moves to bit 5), SIE is cleared, SPP records the mode.
            let mut s = (st & !(SPIE | SPP | SIE)) | ((st & SIE) << 4);
            if old == Mode::Supervisor {
                s |= SPP;
            }
            self.csr.mstatus = s;
            self.mode = Mode::Supervisor;
            self.pc = vector(self.csr.stvec, code, interrupt);
        } else {
            self.csr.mepc = epc;
            self.csr.mcause = cause;
            self.csr.mtval = tval;
            // MPIE takes MIE (bit 3 moves to bit 7), MIE is cleared, MPP records the mode.
            let s = (st & !(MPIE | MPP | MIE)) | ((st & MIE) << 4) | (old.level() << 11);
            self.csr.mstatus = s;
            self.mode = Mode::Machine;
            self.pc = vector(self.csr.mtvec, code, interrupt);
        }
    }
}
