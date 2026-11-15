//! Per-guest virtual hart state, CSR save/restore, and HS-to-VS/VU context switch assembly.

use core::arch::{asm, global_asm};

use crate::boot::{build_gstage, finish_machine, print_str, BootEntry, LineBuf};

pub const MSTATUS_MIE: u64 = 1 << 3;
pub const MSTATUS_MPIE: u64 = 1 << 7;
pub const MSTATUS_MPP: u64 = 3 << 11;
pub const MSTATUS_MASK: u64 = MSTATUS_MIE | MSTATUS_MPIE | MSTATUS_MPP;

pub const SSTATUS_SIE: u64 = 1 << 1;
pub const SSTATUS_SPIE: u64 = 1 << 5;
pub const SSTATUS_SPP: u64 = 1 << 8;

pub const HSTATUS_SPV: u64 = 1 << 7;
pub const HSTATUS_SPVP: u64 = 1 << 8;
pub const HSTATUS_VTVM: u64 = 1 << 20;
pub const HSTATUS_VTW: u64 = 1 << 21;

pub const S_INT_MASK: u64 = (1 << 1) | (1 << 5) | (1 << 9);
pub const VS_INT_MASK: u64 = (1 << 2) | (1 << 6) | (1 << 10);
pub const HEDELEG_MASK: u64 = 0xb1ff;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum GuestMode {
    U = 0,
    S = 1,
    M = 3,
}

impl GuestMode {
    pub fn from_mpp(mpp: u64) -> Self {
        match mpp & 3 {
            1 => GuestMode::S,
            3 => GuestMode::M,
            _ => GuestMode::U,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GuestStatus {
    Running,
    ExitedOk,
    ExitedFail,
    Killed,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GuestRegs {
    pub x: [u64; 32],
    pub sepc: u64,
    pub host_sp: u64,
    pub host_ra: u64,
    pub host_s: [u64; 12],
    pub host_gp: u64,
    pub host_tp: u64,
}

impl GuestRegs {
    pub const fn zero() -> Self {
        Self {
            x: [0; 32],
            sepc: 0,
            host_sp: 0,
            host_ra: 0,
            host_s: [0; 12],
            host_gp: 0,
            host_tp: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct GuestState {
    pub name: [u8; 16],
    pub name_len: usize,
    pub regs: GuestRegs,
    pub mode: GuestMode,
    pub status: GuestStatus,
    pub idled: bool,
    pub nest_count: u32,
    pub hgatp: u64,
    pub mtimecmp: u64,
    pub mstatus: u64,
    pub mtvec: u64,
    pub mepc: u64,
    pub mcause: u64,
    pub mtval: u64,
    pub mscratch: u64,
    pub mie: u64,
    pub mip_sw: u64,
    pub medeleg: u64,
    pub mideleg: u64,
    pub vsstatus: u64,
    pub vstvec: u64,
    pub vsscratch: u64,
    pub vsepc: u64,
    pub vscause: u64,
    pub vstval: u64,
    pub vsatp: u64,
    pub scounteren: u64,
    pub console: LineBuf,
}

impl GuestState {
    pub const fn empty() -> Self {
        Self {
            name: [0; 16],
            name_len: 0,
            regs: GuestRegs::zero(),
            mode: GuestMode::M,
            status: GuestStatus::Killed,
            idled: false,
            nest_count: 0,
            hgatp: 0,
            mtimecmp: u64::MAX,
            mstatus: 0,
            mtvec: 0,
            mepc: 0,
            mcause: 0,
            mtval: 0,
            mscratch: 0,
            mie: 0,
            mip_sw: 0,
            medeleg: 0,
            mideleg: 0,
            vsstatus: 0,
            vstvec: 0,
            vsscratch: 0,
            vsepc: 0,
            vscause: 0,
            vstval: 0,
            vsatp: 0,
            scounteren: 0,
            console: LineBuf::new(),
        }
    }

    pub fn init(&mut self, idx: usize, entry: &BootEntry) {
        *self = Self::empty();
        self.name = entry.name;
        self.name_len = entry.name_len;
        self.regs.sepc = entry.entry;
        self.regs.x[10] = 0; // a0 = hartid 0
        self.regs.x[11] = entry.a1;
        self.status = GuestStatus::Running;
        self.hgatp = build_gstage(idx, entry.slot, entry.size);
    }

    pub fn kill(&mut self, reason: &str) {
        self.console.flush(&self.name[..self.name_len]);
        print_str("zeus: ");
        crate::boot::uart_write(&self.name[..self.name_len]);
        print_str(" killed: ");
        print_str(reason);
        print_str("\n");
        self.status = GuestStatus::Killed;
        self.idled = false;
    }

    pub fn exit_code(&mut self, code: u32, pass: bool) {
        self.console.flush(&self.name[..self.name_len]);
        print_str("zeus: ");
        crate::boot::uart_write(&self.name[..self.name_len]);
        print_str(" exited ");
        crate::boot::print_u64(u64::from(code));
        print_str("\n");
        self.status = if pass {
            GuestStatus::ExitedOk
        } else {
            GuestStatus::ExitedFail
        };
        self.idled = false;
    }
}

extern "C" {
    fn run_guest_asm(regs: *mut GuestRegs);
    fn hlvx_u16_asm(addr: u64) -> u64;
}

#[no_mangle]
extern "C" fn zeus_internal_trap() -> ! {
    print_str("zeus: internal trap\n");
    finish_machine(false);
}

global_asm!(
    ".section .text",
    ".balign 4",
    ".globl run_guest_asm",
    "run_guest_asm:",
    "    sd sp, 264(a0)",
    "    sd ra, 272(a0)",
    "    sd s0, 280(a0)",
    "    sd s1, 288(a0)",
    "    sd s2, 296(a0)",
    "    sd s3, 304(a0)",
    "    sd s4, 312(a0)",
    "    sd s5, 320(a0)",
    "    sd s6, 328(a0)",
    "    sd s7, 336(a0)",
    "    sd s8, 344(a0)",
    "    sd s9, 352(a0)",
    "    sd s10, 360(a0)",
    "    sd s11, 368(a0)",
    "    sd gp, 376(a0)",
    "    sd tp, 384(a0)",
    "    csrw sscratch, a0",
    "    la t0, hs_trap_vector",
    "    csrw stvec, t0",
    "    ld t0, 256(a0)",
    "    csrw sepc, t0",
    "    ld ra, 8(a0)",
    "    ld sp, 16(a0)",
    "    ld gp, 24(a0)",
    "    ld tp, 32(a0)",
    "    ld t0, 40(a0)",
    "    ld t1, 48(a0)",
    "    ld t2, 56(a0)",
    "    ld s0, 64(a0)",
    "    ld s1, 72(a0)",
    "    ld a1, 88(a0)",
    "    ld a2, 96(a0)",
    "    ld a3, 104(a0)",
    "    ld a4, 112(a0)",
    "    ld a5, 120(a0)",
    "    ld a6, 128(a0)",
    "    ld a7, 136(a0)",
    "    ld s2, 144(a0)",
    "    ld s3, 152(a0)",
    "    ld s4, 160(a0)",
    "    ld s5, 168(a0)",
    "    ld s6, 176(a0)",
    "    ld s7, 184(a0)",
    "    ld s8, 192(a0)",
    "    ld s9, 200(a0)",
    "    ld s10, 208(a0)",
    "    ld s11, 216(a0)",
    "    ld t3, 224(a0)",
    "    ld t4, 232(a0)",
    "    ld t5, 240(a0)",
    "    ld t6, 248(a0)",
    "    ld a0, 80(a0)",
    "    sret",
    ".balign 4",
    ".globl hs_trap_vector",
    "hs_trap_vector:",
    "    csrrw a0, sscratch, a0",
    "    sd ra, 8(a0)",
    "    sd sp, 16(a0)",
    "    sd gp, 24(a0)",
    "    sd tp, 32(a0)",
    "    sd t0, 40(a0)",
    "    sd t1, 48(a0)",
    "    sd t2, 56(a0)",
    "    sd s0, 64(a0)",
    "    sd s1, 72(a0)",
    "    sd a1, 88(a0)",
    "    sd a2, 96(a0)",
    "    sd a3, 104(a0)",
    "    sd a4, 112(a0)",
    "    sd a5, 120(a0)",
    "    sd a6, 128(a0)",
    "    sd a7, 136(a0)",
    "    sd s2, 144(a0)",
    "    sd s3, 152(a0)",
    "    sd s4, 160(a0)",
    "    sd s5, 168(a0)",
    "    sd s6, 176(a0)",
    "    sd s7, 184(a0)",
    "    sd s8, 192(a0)",
    "    sd s9, 200(a0)",
    "    sd s10, 208(a0)",
    "    sd s11, 216(a0)",
    "    sd t3, 224(a0)",
    "    sd t4, 232(a0)",
    "    sd t5, 240(a0)",
    "    sd t6, 248(a0)",
    "    csrr t0, sscratch",
    "    sd t0, 80(a0)",
    "    csrr t0, sepc",
    "    sd t0, 256(a0)",
    "    la t0, hs_host_trap",
    "    csrw stvec, t0",
    "    ld sp, 264(a0)",
    "    ld ra, 272(a0)",
    "    ld s0, 280(a0)",
    "    ld s1, 288(a0)",
    "    ld s2, 296(a0)",
    "    ld s3, 304(a0)",
    "    ld s4, 312(a0)",
    "    ld s5, 320(a0)",
    "    ld s6, 328(a0)",
    "    ld s7, 336(a0)",
    "    ld s8, 344(a0)",
    "    ld s9, 352(a0)",
    "    ld s10, 360(a0)",
    "    ld s11, 368(a0)",
    "    ld gp, 376(a0)",
    "    ld tp, 384(a0)",
    "    ret",
    ".balign 4",
    ".globl hlvx_u16_asm",
    "hlvx_u16_asm:",
    "    li a1, 0",
    "hlvx_insn:",
    "    .word 0x64354573",
    "    slli a1, a1, 32",
    "    or a0, a0, a1",
    "    ret",
    ".balign 4",
    ".globl hs_host_trap",
    "hs_host_trap:",
    "    csrr t0, sepc",
    "    la t1, hlvx_insn",
    "    bne t0, t1, 1f",
    "    li a1, 1",
    "    addi t0, t0, 4",
    "    csrw sepc, t0",
    "    sret",
    "1:  j zeus_internal_trap",
);

pub fn hlvx_u16(addr: u64) -> Option<u16> {
    // SAFETY: `hlvx_u16_asm` executes `hlvx.hu` with `stvec` pointing to `hs_host_trap`,
    // which safely catches any guest/VS translation fault during instruction fetch.
    let res = unsafe { hlvx_u16_asm(addr) };
    if (res >> 32) != 0 {
        None
    } else {
        Some(res as u16)
    }
}

pub fn load_guest_state_and_run(g: &mut GuestState) {
    // SAFETY: Caller is in HS-mode with sstatus.SIE == 0; CSRs and `GuestRegs` are valid.
    unsafe {
        let hstatus = HSTATUS_SPV
            | HSTATUS_VTW
            | if g.mode != GuestMode::U {
                HSTATUS_SPVP
            } else {
                0
            }
            | if g.mode == GuestMode::M {
                HSTATUS_VTVM
            } else {
                0
            };
        asm!("csrw 0x600, {0}", in(reg) hstatus, options(nomem, nostack));

        let mut sstatus: u64;
        asm!("csrr {0}, sstatus", out(reg) sstatus, options(nomem, nostack));
        sstatus &= !(SSTATUS_SIE | SSTATUS_SPIE | SSTATUS_SPP);
        if g.mode != GuestMode::U {
            sstatus |= SSTATUS_SPP;
        }
        asm!("csrw sstatus, {0}", in(reg) sstatus, options(nomem, nostack));

        asm!("csrw 0x200, {0}", in(reg) g.vsstatus, options(nomem, nostack));
        asm!("csrw 0x205, {0}", in(reg) g.vstvec, options(nomem, nostack));
        asm!("csrw 0x240, {0}", in(reg) g.vsscratch, options(nomem, nostack));
        asm!("csrw 0x241, {0}", in(reg) g.vsepc, options(nomem, nostack));
        asm!("csrw 0x242, {0}", in(reg) g.vscause, options(nomem, nostack));
        asm!("csrw 0x243, {0}", in(reg) g.vstval, options(nomem, nostack));
        asm!("csrw 0x106, {0}", in(reg) g.scounteren, options(nomem, nostack));

        if g.mode == GuestMode::M {
            asm!("csrw 0x280, zero", options(nomem, nostack));
            asm!("csrw 0x602, zero", options(nomem, nostack));
            asm!("csrw 0x603, zero", options(nomem, nostack));
            asm!("csrw 0x604, zero", options(nomem, nostack));
            asm!("csrw 0x645, zero", options(nomem, nostack));
        } else {
            let hedeleg = g.medeleg & HEDELEG_MASK;
            let hideleg = (g.mideleg & S_INT_MASK) << 1;
            let hie = (g.mie & g.mideleg & S_INT_MASK) << 1;
            let hvip = (g.mip_sw & S_INT_MASK) << 1;
            asm!("csrw 0x280, {0}", in(reg) g.vsatp, options(nomem, nostack));
            asm!("csrw 0x602, {0}", in(reg) hedeleg, options(nomem, nostack));
            asm!("csrw 0x603, {0}", in(reg) hideleg, options(nomem, nostack));
            asm!("csrw 0x604, {0}", in(reg) hie, options(nomem, nostack));
            asm!("csrw 0x645, {0}", in(reg) hvip, options(nomem, nostack));
        }

        asm!("csrw 0x680, {0}", in(reg) g.hgatp, options(nomem, nostack));
        // hfence.gvma x0, x0
        asm!(".word 0x62000073", options(nomem, nostack));

        run_guest_asm(&raw mut g.regs);

        asm!("csrr {0}, 0x200", out(reg) g.vsstatus, options(nomem, nostack));
        asm!("csrr {0}, 0x205", out(reg) g.vstvec, options(nomem, nostack));
        asm!("csrr {0}, 0x240", out(reg) g.vsscratch, options(nomem, nostack));
        asm!("csrr {0}, 0x241", out(reg) g.vsepc, options(nomem, nostack));
        asm!("csrr {0}, 0x242", out(reg) g.vscause, options(nomem, nostack));
        asm!("csrr {0}, 0x243", out(reg) g.vstval, options(nomem, nostack));
        asm!("csrr {0}, 0x106", out(reg) g.scounteren, options(nomem, nostack));

        if g.mode != GuestMode::M {
            let hstatus: u64;
            asm!("csrr {0}, 0x600", out(reg) hstatus, options(nomem, nostack));
            g.mode = if (hstatus & HSTATUS_SPVP) != 0 {
                GuestMode::S
            } else {
                GuestMode::U
            };
            asm!("csrr {0}, 0x280", out(reg) g.vsatp, options(nomem, nostack));
            let hie: u64;
            let hvip: u64;
            asm!("csrr {0}, 0x604", out(reg) hie, options(nomem, nostack));
            asm!("csrr {0}, 0x645", out(reg) hvip, options(nomem, nostack));
            let del = g.mideleg & S_INT_MASK;
            g.mie = (g.mie & !del) | ((hie >> 1) & del);
            g.mip_sw = (hvip & VS_INT_MASK) >> 1;
        }
    }
}

pub fn csrr_scause() -> u64 {
    let v: u64;
    // SAFETY: Executing in HS-mode.
    unsafe { asm!("csrr {0}, scause", out(reg) v, options(nomem, nostack)) };
    v
}

pub fn csrr_stval() -> u64 {
    let v: u64;
    // SAFETY: Executing in HS-mode.
    unsafe { asm!("csrr {0}, stval", out(reg) v, options(nomem, nostack)) };
    v
}

pub fn csrr_htval() -> u64 {
    let v: u64;
    // SAFETY: Executing in HS-mode.
    unsafe { asm!("csrr {0}, 0x643", out(reg) v, options(nomem, nostack)) };
    v
}

pub fn csrr_sip() -> u64 {
    let v: u64;
    // SAFETY: Executing in HS-mode.
    unsafe { asm!("csrr {0}, sip", out(reg) v, options(nomem, nostack)) };
    v
}

pub fn csrc_sip_ssip() {
    // SAFETY: Executing in HS-mode; clears SSIP (bit 1).
    unsafe { asm!("csrc sip, {0}", in(reg) 2u64, options(nomem, nostack)) };
}
