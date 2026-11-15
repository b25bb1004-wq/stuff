//! Zeus hypervisor binary (`riscv64gc-unknown-none-elf` M-mode shim + HS hypervisor).
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
#[path = "zeus/boot.rs"]
mod boot;

#[cfg(target_os = "none")]
#[path = "zeus/guest.rs"]
mod guest;

#[cfg(target_os = "none")]
#[path = "zeus/emulate.rs"]
mod emulate;

#[cfg(target_os = "none")]
#[path = "zeus/trap.rs"]
mod trap;

#[cfg(target_os = "none")]
mod hypervisor {
    use core::arch::{asm, global_asm};
    use core::panic::PanicInfo;

    use crate::boot::{finish_machine, parse_boot_table, print_str};
    use crate::guest::{
        csrc_sip_ssip, csrr_sip, load_guest_state_and_run, GuestState, GuestStatus,
    };
    use crate::trap::{
        deliver_pending_interrupts, handle_trap, pending_interrupt_mask, TrapAction,
    };

    #[no_mangle]
    static mut M_SCRATCH: [u64; 4] = [0; 4];

    global_asm!(
        ".section .text.entry",
        ".globl _start",
        "_start:",
        "    la sp, _stack_top",
        "    la t0, _sbss",
        "    la t1, _ebss",
        "1:  bgeu t0, t1, 2f",
        "    sd zero, 0(t0)",
        "    addi t0, t0, 8",
        "    j 1b",
        "2:  la t0, M_SCRATCH",
        "    csrw mscratch, t0",
        "    la t0, m_trap_vector",
        "    csrw mtvec, t0",
        "    li t0, 0xf0b7ff",
        "    csrw medeleg, t0",
        "    li t0, 0x222",
        "    csrw mideleg, t0",
        "    li t0, 2",
        "    csrw mcounteren, t0",
        "    csrw 0x606, t0",
        "    li t0, 0x0200bff8",
        "    ld t1, 0(t0)",
        "    li t2, 10000",
        "    add t1, t1, t2",
        "    li t0, 0x02004000",
        "    sd t1, 0(t0)",
        "    li t0, 0x80",
        "    csrw mie, t0",
        "    li t0, (1 << 11) | (1 << 7)",
        "    csrw mstatus, t0",
        "    la t0, zeus_main",
        "    csrw mepc, t0",
        "    mret",
        ".section .text",
        ".balign 4",
        "m_trap_vector:",
        "    csrrw tp, mscratch, tp",
        "    sd t0, 0(tp)",
        "    sd t1, 8(tp)",
        "    sd t2, 16(tp)",
        "    li t0, 0x0200bff8",
        "    ld t1, 0(t0)",
        "    li t2, 10000",
        "    add t1, t1, t2",
        "    li t0, 0x02004000",
        "    sd t1, 0(t0)",
        "    li t0, 2",
        "    csrs mip, t0",
        "    ld t0, 0(tp)",
        "    ld t1, 8(tp)",
        "    ld t2, 16(tp)",
        "    csrrw tp, mscratch, tp",
        "    mret",
    );

    #[no_mangle]
    extern "C" fn zeus_main() -> ! {
        // SAFETY: Entered in HS-mode from `_start` with sstatus.SIE == 0 and valid stack.
        unsafe {
            asm!(
                "la t0, hs_host_trap",
                "csrw stvec, t0",
                "li t0, 2",
                "csrs sie, t0",
                out("t0") _,
                options(nomem, nostack),
            );
        }

        let Some((count, entries)) = parse_boot_table() else {
            print_str("zeus: bad boot table\n");
            finish_machine(false);
        };

        let mut guests = [
            GuestState::empty(),
            GuestState::empty(),
            GuestState::empty(),
        ];
        for i in 0..count {
            guests[i].init(i, &entries[i]);
        }

        let mut current = 0usize;
        loop {
            let mut any_running = false;
            for g in guests.iter_mut().take(count) {
                if g.status == GuestStatus::Running {
                    any_running = true;
                    if g.idled && pending_interrupt_mask(g) != 0 {
                        g.idled = false;
                    }
                }
            }

            if !any_running {
                let all_ok = guests
                    .iter()
                    .take(count)
                    .all(|g| g.status == GuestStatus::ExitedOk);
                print_str("zeus: all guests done\n");
                finish_machine(all_ok);
            }

            let mut chosen = None;
            for offset in 0..count {
                let idx = (current + offset) % count;
                if guests[idx].status == GuestStatus::Running && !guests[idx].idled {
                    chosen = Some(idx);
                    break;
                }
            }

            let Some(idx) = chosen else {
                loop {
                    if (csrr_sip() & 2) != 0 {
                        csrc_sip_ssip();
                        break;
                    }
                    // SAFETY: HS-mode wfi waits until SSIP becomes pending in `sip`.
                    unsafe { asm!("wfi", options(nomem, nostack)) };
                }
                continue;
            };

            current = idx;
            if !deliver_pending_interrupts(&mut guests[current]) {
                current = (current + 1) % count;
                continue;
            }

            loop {
                if (csrr_sip() & 2) != 0 {
                    csrc_sip_ssip();
                    current = (current + 1) % count;
                    break;
                }

                load_guest_state_and_run(&mut guests[current]);

                match handle_trap(&mut guests[current]) {
                    TrapAction::Continue => {
                        if !deliver_pending_interrupts(&mut guests[current]) {
                            current = (current + 1) % count;
                            break;
                        }
                    }
                    TrapAction::Yield | TrapAction::QuantumTick | TrapAction::Done => {
                        current = (current + 1) % count;
                        break;
                    }
                }
            }
        }
    }

    #[panic_handler]
    fn panic(_info: &PanicInfo) -> ! {
        print_str("zeus: panic\n");
        finish_machine(false);
    }
}
