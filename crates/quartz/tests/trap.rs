//! Trap tests with hand-placed programs: ecall per mode, delegation, mret and sret round trips,
//! vectored mtvec, interrupt priority and masking, WFI and the privileged-instruction traps.
#![allow(clippy::unwrap_used)]
use quartz::inst::{encode, Inst};
use quartz::{Cpu, Mode, Ram, Step, Trap};

const MIE: u64 = 1 << 3;
const MPIE: u64 = 1 << 7;
const SIE: u64 = 1 << 1;
const SPIE: u64 = 1 << 5;
const SPP: u64 = 1 << 8;
const TVM: u64 = 1 << 20;
const TW: u64 = 1 << 21;
const TSR: u64 = 1 << 22;

fn place(ram: &mut Ram, addr: u64, insts: &[Inst]) {
    for (i, inst) in insts.iter().enumerate() {
        let bytes = encode(*inst).to_le_bytes();
        ram.load(addr + 4 * i as u64, &bytes).unwrap();
    }
}

/// A machine-mode hart with `insts` placed at address 0.
fn setup(insts: &[Inst]) -> (Cpu, Ram) {
    let mut ram = Ram::new(0, 0x4000);
    place(&mut ram, 0, insts);
    (Cpu::new(0), ram)
}

fn mstatus(cpu: &Cpu) -> u64 {
    cpu.csr_read(0x300).unwrap()
}

#[test]
fn ecall_from_machine_goes_to_mtvec_with_cause_11() {
    let (mut cpu, mut ram) = setup(&[Inst::Ecall]);
    cpu.csr_write(0x305, 0x800);
    cpu.csr_write(0x300, MIE);
    assert_eq!(cpu.tick(&mut ram), Step::Exception(Trap::Ecall));
    assert_eq!(cpu.pc, 0x800);
    assert_eq!(cpu.csr_read(0x342), Some(11));
    assert_eq!(cpu.csr_read(0x341), Some(0));
    assert_eq!(cpu.csr_read(0x343), Some(0));
    assert_eq!(cpu.mode, Mode::Machine);
    let st = mstatus(&cpu);
    assert_eq!(st & MIE, 0, "MIE cleared");
    assert_eq!(st & MPIE, MPIE, "MPIE takes the old MIE");
    assert_eq!((st >> 11) & 3, 3, "MPP is machine");
}

#[test]
fn ecall_from_user_goes_to_machine_with_cause_8() {
    let (mut cpu, mut ram) = setup(&[Inst::Ecall]);
    cpu.mode = Mode::User;
    cpu.csr_write(0x305, 0x800);
    cpu.tick(&mut ram);
    assert_eq!((cpu.pc, cpu.mode), (0x800, Mode::Machine));
    assert_eq!(cpu.csr_read(0x342), Some(8));
    assert_eq!((mstatus(&cpu) >> 11) & 3, 0, "MPP records user");
}

#[test]
fn delegated_ecall_from_user_goes_to_supervisor() {
    let (mut cpu, mut ram) = setup(&[Inst::Ecall]);
    cpu.mode = Mode::User;
    cpu.csr_write(0x302, 1 << 8);
    cpu.csr_write(0x105, 0x900);
    cpu.tick(&mut ram);
    assert_eq!((cpu.pc, cpu.mode), (0x900, Mode::Supervisor));
    assert_eq!(cpu.csr_read(0x142), Some(8), "scause");
    assert_eq!(cpu.csr_read(0x141), Some(0), "sepc");
    assert_eq!(mstatus(&cpu) & SPP, 0, "SPP is user");
    assert_eq!(cpu.csr_read(0x342), Some(0), "mcause untouched");
}

#[test]
fn delegated_ecall_from_supervisor_sets_spp_and_cause_9() {
    let (mut cpu, mut ram) = setup(&[Inst::Ecall]);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x302, 1 << 9);
    cpu.csr_write(0x105, 0x900);
    cpu.tick(&mut ram);
    assert_eq!((cpu.pc, cpu.mode), (0x900, Mode::Supervisor));
    assert_eq!(cpu.csr_read(0x142), Some(9));
    assert_eq!(mstatus(&cpu) & SPP, SPP);
}

#[test]
fn ecall_from_machine_is_never_delegated() {
    let (mut cpu, mut ram) = setup(&[Inst::Ecall]);
    cpu.csr_write(0x302, u64::MAX);
    cpu.csr_write(0x305, 0x800);
    cpu.tick(&mut ram);
    assert_eq!((cpu.pc, cpu.mode), (0x800, Mode::Machine));
}

#[test]
fn ebreak_stores_its_pc_as_tval() {
    let mut ram = Ram::new(0, 0x4000);
    place(&mut ram, 0x10, &[Inst::Ebreak]);
    let mut cpu = Cpu::new(0x10);
    cpu.csr_write(0x305, 0x800);
    assert_eq!(cpu.tick(&mut ram), Step::Exception(Trap::Ebreak));
    assert_eq!(
        (
            cpu.csr_read(0x342),
            cpu.csr_read(0x343),
            cpu.csr_read(0x341)
        ),
        (Some(3), Some(0x10), Some(0x10))
    );
}

#[test]
fn illegal_csr_access_stores_the_word_as_tval() {
    let read_mstatus = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0x300,
    };
    let (mut cpu, mut ram) = setup(&[read_mstatus]);
    cpu.mode = Mode::User;
    cpu.csr_write(0x305, 0x800);
    let word = encode(read_mstatus);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(word))
    );
    assert_eq!(cpu.csr_read(0x342), Some(2));
    assert_eq!(cpu.csr_read(0x343), Some(u64::from(word)));
}

#[test]
fn vectored_mtvec_sends_exceptions_to_base_and_interrupts_to_base_plus_4_cause() {
    let (mut cpu, mut ram) = setup(&[Inst::Ecall]);
    cpu.csr_write(0x305, 0x1001);
    cpu.tick(&mut ram);
    assert_eq!(cpu.pc, 0x1000, "exceptions use BASE");

    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.csr_write(0x305, 0x1001);
    cpu.csr_write(0x304, 1 << 7);
    cpu.csr_write(0x300, MIE);
    cpu.set_pending(7, true);
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(7));
    assert_eq!(cpu.pc, 0x1000 + 4 * 7);
    assert_eq!(cpu.csr_read(0x342), Some((1 << 63) | 7));
    assert_eq!(
        cpu.csr_read(0x341),
        Some(0),
        "mepc is the pc of the next instruction"
    );
}

#[test]
fn mret_restores_mode_pc_and_mie() {
    let (mut cpu, mut ram) = setup(&[
        Inst::Ecall,
        Inst::Addi {
            rd: 5,
            rs1: 0,
            imm: 9,
        },
    ]);
    cpu.mode = Mode::User;
    cpu.csr_write(0x305, 0x800);
    cpu.csr_write(0x300, MIE);
    place(&mut ram, 0x800, &[Inst::Mret]);
    cpu.tick(&mut ram);
    cpu.csr_write(0x341, 4);
    assert_eq!(cpu.tick(&mut ram), Step::Retired);
    assert_eq!((cpu.pc, cpu.mode), (4, Mode::User));
    assert_eq!(mstatus(&cpu) & MIE, MIE, "MIE restored from MPIE");
    cpu.tick(&mut ram);
    assert_eq!(cpu.x[5], 9);
}

#[test]
fn mret_outside_machine_mode_is_illegal() {
    let (mut cpu, mut ram) = setup(&[Inst::Mret]);
    cpu.mode = Mode::Supervisor;
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(encode(Inst::Mret)))
    );
}

#[test]
fn sret_restores_supervisor_state() {
    let (mut cpu, mut ram) = setup(&[
        Inst::Sret,
        Inst::Addi {
            rd: 5,
            rs1: 0,
            imm: 9,
        },
    ]);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x300, SPIE);
    cpu.csr_write(0x141, 4);
    assert_eq!(cpu.tick(&mut ram), Step::Retired);
    assert_eq!((cpu.pc, cpu.mode), (4, Mode::User), "SPP was user");
    assert_ne!(cpu.csr_read(0x100).unwrap() & SIE, 0, "SIE takes SPIE");
    cpu.tick(&mut ram);
    assert_eq!(cpu.x[5], 9);
}

#[test]
fn sret_from_user_is_illegal_and_tsr_blocks_it_in_supervisor() {
    let (mut cpu, mut ram) = setup(&[Inst::Sret]);
    cpu.mode = Mode::User;
    assert!(matches!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(_))
    ));

    let (mut cpu, mut ram) = setup(&[Inst::Sret]);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x300, TSR);
    assert!(matches!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(_))
    ));
}

#[test]
fn sfence_vma_is_blocked_by_tvm_in_supervisor_and_by_user_mode() {
    let fence = Inst::SfenceVma { rs1: 0, rs2: 0 };
    let (mut cpu, mut ram) = setup(&[fence]);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x300, TVM);
    assert!(matches!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(_))
    ));

    let (mut cpu, mut ram) = setup(&[fence]);
    cpu.mode = Mode::Supervisor;
    assert_eq!(cpu.tick(&mut ram), Step::Retired);

    let (mut cpu, mut ram) = setup(&[fence]);
    cpu.mode = Mode::User;
    assert!(matches!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(_))
    ));
}

#[test]
fn interrupts_are_taken_in_priority_order() {
    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.csr_write(0x305, 0x800);
    cpu.csr_write(0x304, 0xaaa);
    cpu.csr_write(0x300, MIE);
    for bit in [3, 7, 11] {
        cpu.set_pending(bit, true);
    }
    // Trap entry clears MIE, so the handler re-enables it before the next interrupt.
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(11));
    cpu.set_pending(11, false);
    cpu.csr_write(0x300, MIE);
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(3));
    cpu.set_pending(3, false);
    cpu.csr_write(0x300, MIE);
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(7));
}

#[test]
fn machine_interrupts_need_mie_in_machine_mode_but_not_below_it() {
    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.csr_write(0x305, 0x800);
    cpu.csr_write(0x304, 1 << 7);
    cpu.set_pending(7, true);
    assert_eq!(cpu.tick(&mut ram), Step::Retired, "MIE is clear");
    cpu.csr_write(0x300, MIE);
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(7));

    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.mode = Mode::User;
    cpu.csr_write(0x304, 1 << 7);
    cpu.set_pending(7, true);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Interrupt(7),
        "lower mode always takes it"
    );
}

#[test]
fn delegated_supervisor_interrupts_follow_sie_and_never_reach_machine_mode() {
    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.csr_write(0x303, 1 << 1);
    cpu.csr_write(0x304, 1 << 1);
    cpu.csr_write(0x300, SIE);
    cpu.set_pending(1, true);
    cpu.mode = Mode::Supervisor;
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Interrupt(1),
        "S-mode with SIE set"
    );
    assert_eq!(
        (cpu.mode, cpu.pc),
        (Mode::Supervisor, 0),
        "vector is stvec (0)"
    );

    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.csr_write(0x303, 1 << 1);
    cpu.csr_write(0x304, 1 << 1);
    cpu.csr_write(0x300, SIE);
    cpu.set_pending(1, true);
    cpu.mode = Mode::Supervisor;
    assert_eq!(cpu.tick(&mut ram), Step::Interrupt(1));
    assert_eq!(cpu.csr_read(0x142), Some((1 << 63) | 1));

    let (mut cpu, mut ram) = setup(&[Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 1,
    }]);
    cpu.csr_write(0x303, 1 << 1);
    cpu.csr_write(0x304, 1 << 1);
    cpu.csr_write(0x300, SIE);
    cpu.set_pending(1, true);
    cpu.mode = Mode::Machine;
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Retired,
        "M-mode never takes S interrupts"
    );
}

#[test]
fn wfi_returns_wfi_when_nothing_is_pending_and_advances_pc() {
    let (mut cpu, mut ram) = setup(&[Inst::Wfi]);
    assert_eq!(cpu.tick(&mut ram), Step::Wfi);
    assert_eq!((cpu.pc, cpu.instret), (4, 1));
}

#[test]
fn wfi_with_a_pending_but_masked_interrupt_retires() {
    let (mut cpu, mut ram) = setup(&[Inst::Wfi]);
    cpu.csr_write(0x304, 1 << 7);
    cpu.set_pending(7, true);
    assert_eq!(cpu.tick(&mut ram), Step::Retired);
    assert_eq!(cpu.pc, 4);
}

#[test]
fn tw_traps_wfi_in_supervisor_mode() {
    let (mut cpu, mut ram) = setup(&[Inst::Wfi]);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x300, TW);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(encode(Inst::Wfi)))
    );
}

#[test]
fn cause_codes_for_every_trap() {
    let m = Mode::Machine;
    assert_eq!(Trap::IllegalInstruction(5).cause(m), (2, 5));
    assert_eq!(Trap::FetchMisaligned(6).cause(m), (0, 6));
    assert_eq!(Trap::FetchFault(7).cause(m), (1, 7));
    assert_eq!(Trap::LoadMisaligned(8).cause(m), (4, 8));
    assert_eq!(Trap::LoadFault(9).cause(m), (5, 9));
    assert_eq!(Trap::StoreMisaligned(10).cause(m), (6, 10));
    assert_eq!(Trap::StoreFault(11).cause(m), (7, 11));
    assert_eq!(Trap::InstPageFault(12).cause(m), (12, 12));
    assert_eq!(Trap::LoadPageFault(13).cause(m), (13, 13));
    assert_eq!(Trap::StorePageFault(15).cause(m), (15, 15));
    assert_eq!(Trap::Ecall.cause(Mode::User).0, 8);
    assert_eq!(Trap::Ecall.cause(Mode::Supervisor).0, 9);
    assert_eq!(Trap::Ecall.cause(m).0, 11);
}
