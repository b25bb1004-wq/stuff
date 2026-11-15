//! Trap entry, interrupt selection and the full-hart `tick`.
use crate::bus::Bus;
use crate::csr::{HSTATUS_GVA, H_INT, MIE, MPIE, MPP, MPV, MSTATUS_GVA, SIE, SPIE, SPP, SPV, SPVP};
use crate::{Cpu, Mode, Step, Trap};

/// Interrupt cause codes in priority order: MEI, MSI, MTI, SEI, SSI, STI, SGEI, VSEI, VSSI, VSTI.
const PRIORITY: [u64; 10] = [11, 3, 7, 9, 1, 5, 12, 10, 2, 6];

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
            Trap::InstGuestPageFault { gva, .. } => (20, gva),
            Trap::LoadGuestPageFault { gva, .. } => (21, gva),
            Trap::VirtualInstruction(w) => (22, u64::from(w)),
            Trap::StoreGuestPageFault { gva, .. } => (23, gva),
        }
    }

    /// Guest physical address shifted right by 2 for guest-page faults, or 0 otherwise.
    fn gpa_val(&self) -> u64 {
        match *self {
            Trap::InstGuestPageFault { gpa, .. }
            | Trap::LoadGuestPageFault { gpa, .. }
            | Trap::StoreGuestPageFault { gpa, .. } => gpa >> 2,
            _ => 0,
        }
    }
}

impl Cpu {
    /// One step of the whole hart. It takes a pending enabled interrupt, or else executes one
    /// instruction and takes its trap. A trap or interrupt goes to the handler, so the pc
    /// changes; `Step` reports what happened.
    pub fn tick(&mut self, bus: &mut impl Bus) -> Step {
        self.tick_with(bus, |cpu, bus, pc| cpu.fetch(bus, pc)).0
    }

    /// Like `tick`, using `fetch_fn` and returning `(Step, sfence_executed)`.
    #[inline(always)]
    pub(crate) fn tick_with<B: Bus>(
        &mut self,
        bus: &mut B,
        fetch_fn: impl FnOnce(&Self, &mut B, u64) -> Result<(crate::inst::Inst, u32, u64), Trap>,
    ) -> (Step, bool) {
        if let Some(code) = self.pending_interrupt() {
            self.enter(code, true, 0, 0, false);
            return (Step::Interrupt(code), false);
        }
        match self.step_inner_with(bus, fetch_fn) {
            Ok(flags) => {
                let step = if flags.wfi && self.csr.mip & self.csr.mie == 0 {
                    Step::Wfi
                } else {
                    Step::Retired
                };
                (step, flags.sfence)
            }
            Err((trap, from_hlv_hsv)) => {
                let (mut code, tval) = trap.cause(self.mode);
                if trap == Trap::Ecall && self.virt && self.mode == Mode::Supervisor {
                    code = 10;
                }
                let tval = if trap == Trap::Ebreak { self.pc } else { tval };
                let is_gva = (self.virt || from_hlv_hsv)
                    && matches!(code, 0 | 1 | 4 | 5 | 6 | 7 | 12 | 13 | 15 | 20 | 21 | 23);
                self.enter(code, false, tval, trap.gpa_val(), is_gva);
                (Step::Exception(trap), false)
            }
        }
    }

    /// The interrupt to take now, if any. M-destined interrupts come before HS-destined ones,
    /// which come before VS-destined ones.
    fn pending_interrupt(&self) -> Option<u64> {
        let pend = self.csr.mip & self.csr.mie;
        if pend == 0 {
            return None;
        }
        let st = self.csr.mstatus;
        let m_on = self.mode != Mode::Machine || (st & MIE) != 0;
        let hs_on = self.virt
            || self.mode == Mode::User
            || (self.mode == Mode::Supervisor && (st & SIE) != 0);
        let vs_on = self.virt && (self.mode == Mode::User || (self.csr.vsstatus & SIE) != 0);
        let mdeleg = self.csr.mideleg | H_INT;
        let hdeleg = self.csr.hideleg;
        let to_m = PRIORITY
            .iter()
            .copied()
            .find(|&c| m_on && bit(pend, c) && !bit(mdeleg, c));
        let to_hs = PRIORITY
            .iter()
            .copied()
            .find(|&c| hs_on && bit(pend, c) && bit(mdeleg, c) && !bit(hdeleg, c));
        let to_vs = PRIORITY
            .iter()
            .copied()
            .find(|&c| vs_on && bit(pend, c) && bit(mdeleg, c) && bit(hdeleg, c));
        to_m.or(to_hs).or(to_vs)
    }

    /// Take a trap (or interrupt, with `interrupt` set) with cause `code` into its handler.
    fn enter(&mut self, code: u64, interrupt: bool, tval: u64, gpa2: u64, is_gva: bool) {
        self.reservation = None;
        let mdeleg = if interrupt {
            self.csr.mideleg | H_INT
        } else {
            self.csr.medeleg
        };
        let hdeleg = if interrupt {
            self.csr.hideleg
        } else {
            self.csr.hedeleg
        };
        let epc = self.pc;
        let old = self.mode;
        let old_virt = self.virt;
        if old != Mode::Machine && bit(mdeleg, code) {
            if old_virt && bit(hdeleg, code) {
                let vs_code = if interrupt { code - 1 } else { code };
                let vs_cause = if interrupt {
                    (1 << 63) | vs_code
                } else {
                    vs_code
                };
                self.csr.vsepc = epc;
                self.csr.vscause = vs_cause;
                self.csr.vstval = tval;
                let vs = self.csr.vsstatus;
                let mut s = (vs & !(SPIE | SPP | SIE)) | ((vs & SIE) << 4);
                if old == Mode::Supervisor {
                    s |= SPP;
                }
                self.csr.vsstatus = s;
                self.mode = Mode::Supervisor;
                self.virt = true;
                self.pc = vector(self.csr.vstvec, vs_code, interrupt);
                return;
            }
            let cause = if interrupt { (1 << 63) | code } else { code };
            self.csr.sepc = epc;
            self.csr.scause = cause;
            self.csr.stval = tval;
            self.csr.htval = gpa2;
            self.csr.htinst = 0;
            let st = self.csr.mstatus;
            let mut s = (st & !(SPIE | SPP | SIE)) | ((st & SIE) << 4);
            if old == Mode::Supervisor {
                s |= SPP;
            }
            self.csr.mstatus = s;
            let mut hs = self.csr.hstatus & !(SPV | HSTATUS_GVA);
            if old_virt {
                hs |= SPV;
                if old == Mode::Supervisor {
                    hs |= SPVP;
                } else {
                    hs &= !SPVP;
                }
            }
            if is_gva {
                hs |= HSTATUS_GVA;
            }
            self.csr.hstatus = hs;
            self.mode = Mode::Supervisor;
            self.virt = false;
            self.pc = vector(self.csr.stvec, code, interrupt);
        } else {
            let cause = if interrupt { (1 << 63) | code } else { code };
            self.csr.mepc = epc;
            self.csr.mcause = cause;
            self.csr.mtval = tval;
            self.csr.mtval2 = gpa2;
            self.csr.mtinst = 0;
            let st = self.csr.mstatus;
            let mut s = (st & !(MPIE | MPP | MIE | MPV | MSTATUS_GVA))
                | ((st & MIE) << 4)
                | (old.level() << 11);
            if old_virt {
                s |= MPV;
            }
            if is_gva {
                s |= MSTATUS_GVA;
            }
            self.csr.mstatus = s;
            self.mode = Mode::Machine;
            self.virt = false;
            self.pc = vector(self.csr.mtvec, code, interrupt);
        }
    }
}
