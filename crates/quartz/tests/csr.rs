//! CSR tests: WARL masks, masked views, read-only and privilege rules, counter enables.
#![allow(clippy::unwrap_used)]
use quartz::inst::{encode, Inst};
use quartz::{Cpu, Mode, Ram, Trap};

fn place(ram: &mut Ram, addr: u64, insts: &[Inst]) {
    for (i, inst) in insts.iter().enumerate() {
        let bytes = encode(*inst).to_le_bytes();
        ram.load(addr + 4 * i as u64, &bytes).unwrap();
    }
}

/// Run one instruction at pc 0 from a fresh RAM.
fn step_one(cpu: &mut Cpu, inst: Inst) -> Result<(), Trap> {
    let mut ram = Ram::new(0, 0x1000);
    place(&mut ram, 0, &[inst]);
    cpu.pc = 0;
    cpu.step(&mut ram)
}

#[test]
fn misa_is_rv64_imsu_and_ignores_writes() {
    let mut cpu = Cpu::new(0);
    let misa = cpu.csr_read(0x301).unwrap();
    assert_eq!(misa >> 62, 2);
    assert_eq!(misa & 0x14_1100, 0x14_1100);
    assert!(cpu.csr_write(0x301, 0));
    assert_eq!(cpu.csr_read(0x301), Some(misa));
}

#[test]
fn id_registers_read_zero() {
    let cpu = Cpu::new(0);
    for addr in [0xf11, 0xf12, 0xf13, 0xf14] {
        assert_eq!(cpu.csr_read(addr), Some(0), "{addr:#x}");
    }
    assert_eq!(cpu.csr_read(0x7c0), None);
}

#[test]
fn mstatus_keeps_only_writable_bits_and_fixed_xl() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x300, u64::MAX);
    let m = cpu.csr_read(0x300).unwrap();
    // UXL and SXL read 2; the writable bits are MIE SIE MPIE SPIE SPP MPP MPRV SUM MXR TVM TW TSR.
    assert_eq!(m, (2 << 32) | (2 << 34) | 0x7e_19aa);
}

#[test]
fn mstatus_reserved_mpp_keeps_old_value() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x300, 0x1800);
    cpu.csr_write(0x300, 0x1000);
    assert_eq!(cpu.csr_read(0x300).unwrap() & 0x1800, 0x1800);
    cpu.csr_write(0x300, 0x0800);
    assert_eq!(cpu.csr_read(0x300).unwrap() & 0x1800, 0x0800);
}

#[test]
fn sstatus_is_a_masked_view_of_mstatus() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x300, u64::MAX);
    cpu.csr_write(0x100, 0);
    let m = cpu.csr_read(0x300).unwrap();
    let s = cpu.csr_read(0x100).unwrap();
    assert_eq!(
        m & (1 << 3),
        1 << 3,
        "MIE is not in sstatus, so writing it leaves MIE alone"
    );
    assert_eq!(s & (1 << 3), 0, "MIE is not visible through sstatus");
    assert_eq!((s >> 32) & 3, 2, "UXL reads 2");
    assert_eq!(s & (1 << 1), 0, "SIE cleared by the sstatus write");
}

#[test]
fn sie_and_sip_only_show_delegated_supervisor_bits() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x104, u64::MAX);
    assert_eq!(
        cpu.csr_read(0x304),
        Some(0),
        "nothing delegated, so sie writes do nothing"
    );
    cpu.csr_write(0x303, u64::MAX);
    cpu.csr_write(0x104, u64::MAX);
    assert_eq!(cpu.csr_read(0x304), Some(0x222));
    assert_eq!(cpu.csr_read(0x104), Some(0x222));
    cpu.csr_write(0x144, u64::MAX);
    assert_eq!(cpu.csr_read(0x344), Some(0x2), "sip may only write SSIP");
}

#[test]
fn delegation_and_enable_masks() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x302, u64::MAX);
    assert_eq!(
        cpu.csr_read(0x302),
        Some(0xb3ff),
        "ecall from M (11) is never delegated"
    );
    cpu.csr_write(0x303, u64::MAX);
    assert_eq!(cpu.csr_read(0x303), Some(0x222));
    cpu.csr_write(0x304, u64::MAX);
    assert_eq!(cpu.csr_read(0x304), Some(0xaaa));
}

#[test]
fn mtvec_and_stvec_mode_warl() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x305, 0x1003);
    assert_eq!(
        cpu.csr_read(0x305),
        Some(0x1000),
        "reserved MODE 3 keeps the old MODE 0"
    );
    cpu.csr_write(0x305, 0x2001);
    assert_eq!(cpu.csr_read(0x305), Some(0x2001), "MODE 1 is vectored");
    cpu.csr_write(0x305, 0x2002);
    assert_eq!(
        cpu.csr_read(0x305),
        Some(0x2001),
        "reserved MODE 2 keeps vectored"
    );
    cpu.csr_write(0x105, 0x3000);
    assert_eq!(cpu.csr_read(0x105), Some(0x3000));
}

#[test]
fn epc_bits_are_forced_aligned() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x341, 0x7);
    cpu.csr_write(0x141, 0xff);
    assert_eq!(cpu.csr_read(0x341), Some(0x4));
    assert_eq!(cpu.csr_read(0x141), Some(0xfc));
}

#[test]
fn satp_accepts_only_bare_and_sv39() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x180, (8 << 60) | 5);
    assert_eq!(cpu.csr_read(0x180), Some((8 << 60) | 5));
    cpu.csr_write(0x180, (1 << 60) | 7);
    assert_eq!(
        cpu.csr_read(0x180),
        Some((8 << 60) | 5),
        "Sv48 is not supported"
    );
    cpu.csr_write(0x180, 0);
    assert_eq!(cpu.csr_read(0x180), Some(0));
}

#[test]
fn mip_software_bits_and_hardware_pending() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x344, u64::MAX);
    assert_eq!(
        cpu.csr_read(0x344),
        Some(0x222),
        "MSIP, MTIP, MEIP are hardware only"
    );
    cpu.csr_write(0x344, 0);
    cpu.set_pending(7, true);
    cpu.set_pending(2, true);
    assert_eq!(cpu.csr_read(0x344), Some(0x80));
    cpu.set_pending(7, false);
    assert_eq!(cpu.csr_read(0x344), Some(0));
}

#[test]
fn counters_are_read_only_except_for_the_host_time_and_minstret() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0xc00, 5);
    assert_eq!(cpu.csr_read(0xc00), Some(0), "cycle is read-only");
    assert!(cpu.csr_write(0xc01, 99));
    assert_eq!(cpu.csr_read(0xc01), Some(99), "host sets time");
    assert!(cpu.csr_write(0xb02, 42));
    assert_eq!((cpu.instret, cpu.csr_read(0xb02)), (42, Some(42)));
    assert!(!cpu.csr_write(0x7c0, 1));
}

#[test]
fn pmp_registers_read_zero_and_ignore_writes() {
    let mut cpu = Cpu::new(0);
    assert_eq!(cpu.csr_read(0x3a0), Some(0));
    assert_eq!(
        cpu.csr_read(0x3a1),
        None,
        "odd pmpcfg does not exist on RV64"
    );
    assert_eq!(cpu.csr_read(0x3b5), Some(0));
    cpu.csr_write(0x3b5, 0xff);
    assert_eq!(cpu.csr_read(0x3b5), Some(0));
}

#[test]
fn csrrw_swaps_and_csrrs_csrrc_set_and_clear() {
    let mut cpu = Cpu::new(0);
    cpu.x[2] = 0xf0;
    step_one(
        &mut cpu,
        Inst::Csrrw {
            rd: 1,
            rs1: 2,
            csr: 0x340,
        },
    )
    .unwrap();
    assert_eq!((cpu.x[1], cpu.csr_read(0x340)), (0, Some(0xf0)));
    cpu.x[3] = 0x0f;
    step_one(
        &mut cpu,
        Inst::Csrrs {
            rd: 1,
            rs1: 3,
            csr: 0x340,
        },
    )
    .unwrap();
    assert_eq!((cpu.x[1], cpu.csr_read(0x340)), (0xf0, Some(0xff)));
    cpu.x[3] = 0x3c;
    step_one(
        &mut cpu,
        Inst::Csrrc {
            rd: 1,
            rs1: 3,
            csr: 0x340,
        },
    )
    .unwrap();
    assert_eq!((cpu.x[1], cpu.csr_read(0x340)), (0xff, Some(0xc3)));
}

#[test]
fn set_and_clear_with_x0_do_not_write_a_read_only_csr() {
    let mut cpu = Cpu::new(0);
    let read_only = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0xf11,
    };
    assert_eq!(step_one(&mut cpu, read_only), Ok(()));
    let write = Inst::Csrrw {
        rd: 1,
        rs1: 0,
        csr: 0xf11,
    };
    let word = encode(write);
    assert_eq!(
        step_one(&mut cpu, write),
        Err(Trap::IllegalInstruction(word))
    );
    let imm_clear = Inst::Csrrsi {
        rd: 1,
        uimm: 0,
        csr: 0xc00,
    };
    assert_eq!(
        step_one(&mut cpu, imm_clear),
        Ok(()),
        "uimm 0 does not write"
    );
    let imm_set = Inst::Csrrsi {
        rd: 1,
        uimm: 1,
        csr: 0xc00,
    };
    assert!(step_one(&mut cpu, imm_set).is_err());
}

#[test]
fn user_mode_cannot_touch_machine_csrs() {
    let mut cpu = Cpu::new(0);
    cpu.mode = Mode::User;
    let inst = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0x300,
    };
    let before = cpu.clone();
    assert_eq!(
        step_one(&mut cpu, inst),
        Err(Trap::IllegalInstruction(encode(inst)))
    );
    assert_eq!(cpu, before, "a failed step changes nothing");
}

#[test]
fn supervisor_cannot_touch_machine_csrs_but_reaches_its_own() {
    let mut cpu = Cpu::new(0);
    cpu.mode = Mode::Supervisor;
    let m = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0x341,
    };
    assert!(step_one(&mut cpu, m).is_err());
    let s = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0x141,
    };
    assert_eq!(step_one(&mut cpu, s), Ok(()));
    let unknown = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0x7c0,
    };
    assert!(step_one(&mut cpu, unknown).is_err());
}

#[test]
fn counters_need_mcounteren_and_user_also_needs_scounteren() {
    let mut cpu = Cpu::new(0);
    let cycle = Inst::Csrrs {
        rd: 1,
        rs1: 0,
        csr: 0xc00,
    };
    cpu.mode = Mode::User;
    assert!(step_one(&mut cpu, cycle).is_err(), "mcounteren is zero");
    cpu.csr_write(0x306, 1);
    assert!(step_one(&mut cpu, cycle).is_err(), "scounteren is zero");
    cpu.csr_write(0x106, 1);
    assert_eq!(step_one(&mut cpu, cycle), Ok(()));
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x106, 0);
    assert_eq!(
        step_one(&mut cpu, cycle),
        Ok(()),
        "S-mode needs only mcounteren"
    );
}

#[test]
fn tvm_blocks_satp_in_supervisor_mode() {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x300, 1 << 20);
    cpu.mode = Mode::Supervisor;
    let satp = Inst::Csrrw {
        rd: 1,
        rs1: 2,
        csr: 0x180,
    };
    assert!(step_one(&mut cpu, satp).is_err());
    cpu.mode = Mode::Machine;
    assert_eq!(step_one(&mut cpu, satp), Ok(()));
}
