//! HS trap handling, guest M/VS trap injection, `mret`/`wfi` emulation, and virtual timer delivery.

use crate::boot::read_mtime;
use crate::emulate::{emulate_m_csr, emulate_m_satp_or_sfence, emulate_mmio, MmioOutcome};
use crate::guest::{
    csrc_sip_ssip, csrr_htval, csrr_scause, csrr_stval, GuestMode, GuestState, GuestStatus,
    MSTATUS_MIE, MSTATUS_MPIE, MSTATUS_MPP, SSTATUS_SIE, SSTATUS_SPIE, SSTATUS_SPP, S_INT_MASK,
};

pub enum TrapAction {
    Continue,
    Yield,
    QuantumTick,
    Done,
}

pub fn pending_interrupt_mask(g: &GuestState) -> u64 {
    let mtip = if read_mtime() >= g.mtimecmp {
        1 << 7
    } else {
        0
    };
    let mip = (g.mip_sw & S_INT_MASK) | mtip;
    g.mie & mip
}

pub fn deliver_pending_interrupts(g: &mut GuestState) -> bool {
    let pend = pending_interrupt_mask(g);
    if pend == 0 {
        return true;
    }
    g.idled = false;

    let m_enabled = g.mode != GuestMode::M || (g.mstatus & MSTATUS_MIE) != 0;
    if !m_enabled {
        return true;
    }

    if (pend & (1 << 7)) != 0 {
        trap_to_guest_m(g, (1 << 63) | 7, 0);
        return g.status == GuestStatus::Running;
    }

    let non_deleg = pend & !g.mideleg & S_INT_MASK;
    for bit in [9u64, 1, 5] {
        if (non_deleg & (1 << bit)) != 0 {
            trap_to_guest_m(g, (1 << 63) | bit, 0);
            return g.status == GuestStatus::Running;
        }
    }

    true
}

fn trap_to_guest_m(g: &mut GuestState, mcause: u64, mtval: u64) {
    if g.mtvec == 0 {
        g.kill("mtvec is 0");
        return;
    }
    g.nest_count += 1;
    if g.nest_count > 64 {
        g.kill("trap nesting limit");
        return;
    }

    g.mepc = g.regs.sepc;
    g.mcause = mcause;
    g.mtval = mtval;

    let old_mie = (g.mstatus & MSTATUS_MIE) != 0;
    let old_mode = g.mode as u64;
    let mut st = g.mstatus & !(MSTATUS_MIE | MSTATUS_MPIE | MSTATUS_MPP);
    if old_mie {
        st |= MSTATUS_MPIE;
    }
    st |= (old_mode & 3) << 11;
    g.mstatus = st;
    g.mode = GuestMode::M;
    g.regs.sepc = g.mtvec;
}

fn trap_to_guest_vs(g: &mut GuestState, cause: u64, tval: u64) {
    g.vsepc = g.regs.sepc;
    g.vscause = cause;
    g.vstval = tval;

    let old_sie = (g.vsstatus & SSTATUS_SIE) != 0;
    let mut vs = g.vsstatus & !(SSTATUS_SIE | SSTATUS_SPIE | SSTATUS_SPP);
    if old_sie {
        vs |= SSTATUS_SPIE;
    }
    if g.mode == GuestMode::S {
        vs |= SSTATUS_SPP;
    }
    g.vsstatus = vs;
    g.mode = GuestMode::S;
    g.regs.sepc = g.vstvec & !3;
}

fn emulate_mret(g: &mut GuestState) {
    g.nest_count = 0;
    let target = GuestMode::from_mpp((g.mstatus & MSTATUS_MPP) >> 11);
    let mpie = (g.mstatus & MSTATUS_MPIE) != 0;
    let mut st = g.mstatus & !(MSTATUS_MIE | MSTATUS_MPIE | MSTATUS_MPP);
    if mpie {
        st |= MSTATUS_MIE;
    }
    st |= MSTATUS_MPIE;
    g.mstatus = st;
    g.mode = target;
    g.regs.sepc = g.mepc;
}

fn emulate_wfi(g: &mut GuestState) -> TrapAction {
    g.regs.sepc = g.regs.sepc.wrapping_add(4);
    if pending_interrupt_mask(g) == 0 {
        g.idled = true;
        TrapAction::Yield
    } else {
        TrapAction::Continue
    }
}

pub fn handle_trap(g: &mut GuestState) -> TrapAction {
    let scause = csrr_scause();
    let stval = csrr_stval();

    if (scause >> 63) != 0 {
        let code = scause & 0x7fff_ffff_ffff_ffff;
        if code == 1 {
            csrc_sip_ssip();
            return TrapAction::QuantumTick;
        }
        return TrapAction::Continue;
    }

    match scause {
        2 => {
            let inst = stval as u32;
            if g.mode == GuestMode::M {
                if inst == 0x3020_0073 {
                    emulate_mret(g);
                    return TrapAction::Continue;
                }
                if inst == 0x1050_0073 {
                    return emulate_wfi(g);
                }
                if emulate_m_csr(g, inst) {
                    return TrapAction::Continue;
                }
            }
            trap_to_guest_m(g, 2, stval);
            if g.status == GuestStatus::Running {
                TrapAction::Continue
            } else {
                TrapAction::Done
            }
        }
        22 => {
            let inst = stval as u32;
            if g.mode == GuestMode::M {
                if inst == 0x3020_0073 {
                    emulate_mret(g);
                    return TrapAction::Continue;
                }
                if inst == 0x1050_0073 {
                    return emulate_wfi(g);
                }
                if emulate_m_satp_or_sfence(g, inst) {
                    return TrapAction::Continue;
                }
                trap_to_guest_m(g, 2, stval);
            } else if inst == 0x1050_0073 {
                return emulate_wfi(g);
            } else if (g.medeleg & (1 << 2)) != 0 {
                trap_to_guest_vs(g, 2, stval);
            } else {
                trap_to_guest_m(g, 2, stval);
            }
            if g.status == GuestStatus::Running {
                TrapAction::Continue
            } else {
                TrapAction::Done
            }
        }
        10 => {
            let cause = if g.mode == GuestMode::M { 11 } else { 9 };
            trap_to_guest_m(g, cause, 0);
            if g.status == GuestStatus::Running {
                TrapAction::Continue
            } else {
                TrapAction::Done
            }
        }
        8 => {
            trap_to_guest_m(g, 8, 0);
            if g.status == GuestStatus::Running {
                TrapAction::Continue
            } else {
                TrapAction::Done
            }
        }
        20 => {
            if g.mode != GuestMode::M && (g.medeleg & (1 << 1)) != 0 {
                trap_to_guest_vs(g, 1, stval);
            } else {
                trap_to_guest_m(g, 1, stval);
            }
            if g.status == GuestStatus::Running {
                TrapAction::Continue
            } else {
                TrapAction::Done
            }
        }
        21 | 23 => {
            let is_store = scause == 23;
            let gpa = (csrr_htval() << 2) | (stval & 3);
            match emulate_mmio(g, is_store, gpa) {
                MmioOutcome::Done => TrapAction::Continue,
                MmioOutcome::Exited => TrapAction::Done,
                MmioOutcome::AccessFault => {
                    let access_cause = if is_store { 7 } else { 5 };
                    if g.mode != GuestMode::M && (g.medeleg & (1 << access_cause)) != 0 {
                        trap_to_guest_vs(g, access_cause, stval);
                    } else {
                        trap_to_guest_m(g, access_cause, stval);
                    }
                    if g.status == GuestStatus::Running {
                        TrapAction::Continue
                    } else {
                        TrapAction::Done
                    }
                }
            }
        }
        code => {
            trap_to_guest_m(g, code, stval);
            if g.status == GuestStatus::Running {
                TrapAction::Continue
            } else {
                TrapAction::Done
            }
        }
    }
}
