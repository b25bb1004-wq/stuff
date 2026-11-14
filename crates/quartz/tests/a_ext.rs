#![allow(clippy::unwrap_used)]
//! A extension (RV64A) tests: decode/encode round trips, all 9 AMO ops (W and D, sign cases),
//! MMIO bus call counts, LR/SC reservation sequences, trap clearing, and alignment/bus/page faults.
use assay::forall;
use quartz::inst::{decode, encode, AmoOp, Inst};
use quartz::{Bus, Cpu, Fault, Mode, Ram, Step, Trap};

fn place(ram: &mut Ram, addr: u64, insts: &[Inst]) {
    let bytes: Vec<u8> = insts
        .iter()
        .flat_map(|i| encode(*i).to_le_bytes())
        .collect();
    ram.load(addr, &bytes).unwrap();
}

#[derive(Default)]
struct TraceBus {
    inner: Option<Ram>,
    ops: Vec<(bool, u64, u8, u64)>,
    fail_write: bool,
    fail_read: bool,
}

impl Bus for TraceBus {
    fn read(&mut self, addr: u64, size: u8) -> Result<u64, Fault> {
        self.ops.push((false, addr, size, 0));
        if self.fail_read && addr >= 0x100 {
            return Err(Fault::Unmapped(addr));
        }
        match &mut self.inner {
            Some(r) => r.read(addr, size),
            None => Ok(0x1234_5678),
        }
    }

    fn write(&mut self, addr: u64, size: u8, value: u64) -> Result<(), Fault> {
        self.ops.push((true, addr, size, value));
        if self.fail_write {
            return Err(Fault::Unmapped(addr));
        }
        match &mut self.inner {
            Some(r) => r.write(addr, size, value),
            None => Ok(()),
        }
    }
}

#[test]
fn a_ext_decode_encode_round_trip() {
    let ops = [
        AmoOp::Swap,
        AmoOp::Add,
        AmoOp::Xor,
        AmoOp::And,
        AmoOp::Or,
        AmoOp::Min,
        AmoOp::Max,
        AmoOp::Minu,
        AmoOp::Maxu,
    ];
    forall(1000, 0xA001, |r| {
        let rd = r.below(32) as u8;
        let rs1 = r.below(32) as u8;
        let rs2 = r.below(32) as u8;
        let aq = r.chance(1, 2);
        let rl = r.chance(1, 2);
        let op = *r.pick(&ops);
        for inst in [
            Inst::LrW { rd, rs1, aq, rl },
            Inst::LrD { rd, rs1, aq, rl },
            Inst::ScW {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            },
            Inst::ScD {
                rd,
                rs1,
                rs2,
                aq,
                rl,
            },
            Inst::AmoW {
                op,
                rd,
                rs1,
                rs2,
                aq,
                rl,
            },
            Inst::AmoD {
                op,
                rd,
                rs1,
                rs2,
                aq,
                rl,
            },
        ] {
            let w = encode(inst);
            assert_eq!(decode(w), Some(inst), "{inst:?}");
        }
    });
    // LR with rs2 != 0 must decode to None.
    let lr_w = encode(Inst::LrW {
        rd: 1,
        rs1: 2,
        aq: false,
        rl: false,
    });
    assert_eq!(decode(lr_w | (1 << 20)), None);
    let lr_d = encode(Inst::LrD {
        rd: 1,
        rs1: 2,
        aq: true,
        rl: true,
    });
    assert_eq!(decode(lr_d | (5 << 20)), None);
}

#[test]
fn amo_table_w_and_d_with_sign_cases() {
    let cases_w: &[(AmoOp, u32, u32, u32)] = &[
        (AmoOp::Swap, 0x8000_0001, 0x1234_5678, 0x1234_5678),
        (AmoOp::Add, 0xffff_ffff, 2, 1),
        (AmoOp::Xor, 0xf0f0_00ff, 0x0ff0_ff00, 0xff00_ffff),
        (AmoOp::And, 0xf0f0_00ff, 0x0ff0_ff00, 0x00f0_0000),
        (AmoOp::Or, 0xf0f0_0000, 0x0000_000f, 0xf0f0_000f),
        (AmoOp::Min, 0x8000_0000, 1, 0x8000_0000),
        (AmoOp::Min, 5, 0xffff_fffe, 0xffff_fffe),
        (AmoOp::Max, 0x8000_0000, 1, 1),
        (AmoOp::Max, 0xffff_fffe, 0xffff_ffff, 0xffff_ffff),
        (AmoOp::Minu, 0x8000_0000, 1, 1),
        (AmoOp::Maxu, 0x8000_0000, 1, 0x8000_0000),
    ];
    for &(op, init, rs2, expect_mem) in cases_w {
        let mut ram = Ram::new(0, 0x1000);
        // Put a sentinel upper half to verify AmoW leaves bits 63:32 untouched.
        let full_init = (0xdead_beefu64 << 32) | u64::from(init);
        ram.write(0x100, 8, full_init).unwrap();
        place(
            &mut ram,
            0,
            &[Inst::AmoW {
                op,
                rd: 3,
                rs1: 1,
                rs2: 2,
                aq: false,
                rl: false,
            }],
        );
        let mut cpu = Cpu::new(0);
        cpu.x[1] = 0x100;
        cpu.x[2] = u64::from(rs2) | 0xaaaa_0000_0000_0000;
        cpu.step(&mut ram).unwrap();
        assert_eq!(cpu.x[3], i64::from(init as i32) as u64, "{op:?}");
        assert_eq!(
            ram.read(0x100, 8).unwrap(),
            (0xdead_beefu64 << 32) | u64::from(expect_mem),
            "{op:?}"
        );
    }

    let cases_d: &[(AmoOp, u64, u64, u64)] = &[
        (
            AmoOp::Swap,
            0x1111_2222_3333_4444,
            0x5555_6666_7777_8888,
            0x5555_6666_7777_8888,
        ),
        (AmoOp::Add, u64::MAX, 10, 9),
        (
            AmoOp::Xor,
            0xff00_ff00_00ff_00ff,
            0x0f0f_0f0f_0f0f_0f0f,
            0xf00f_f00f_0ff0_0ff0,
        ),
        (
            AmoOp::And,
            0xff00_ff00_00ff_00ff,
            0x0f0f_0f0f_0f0f_0f0f,
            0x0f00_0f00_000f_000f,
        ),
        (AmoOp::Or, 0xf000_0000_0000_0000, 1, 0xf000_0000_0000_0001),
        (
            AmoOp::Min,
            i64::MIN as u64,
            i64::MAX as u64,
            i64::MIN as u64,
        ),
        (
            AmoOp::Max,
            i64::MIN as u64,
            i64::MAX as u64,
            i64::MAX as u64,
        ),
        (
            AmoOp::Minu,
            i64::MIN as u64,
            i64::MAX as u64,
            i64::MAX as u64,
        ),
        (
            AmoOp::Maxu,
            i64::MIN as u64,
            i64::MAX as u64,
            i64::MIN as u64,
        ),
    ];
    for &(op, init, rs2, expect_mem) in cases_d {
        let mut ram = Ram::new(0, 0x1000);
        ram.write(0x100, 8, init).unwrap();
        place(
            &mut ram,
            0,
            &[Inst::AmoD {
                op,
                rd: 3,
                rs1: 1,
                rs2: 2,
                aq: true,
                rl: true,
            }],
        );
        let mut cpu = Cpu::new(0);
        cpu.x[1] = 0x100;
        cpu.x[2] = rs2;
        cpu.step(&mut ram).unwrap();
        assert_eq!(cpu.x[3], init, "{op:?}");
        assert_eq!(ram.read(0x100, 8).unwrap(), expect_mem, "{op:?}");
    }
}

#[test]
fn amo_performs_one_read_and_one_write_on_mmio() {
    let mut ram = Ram::new(0, 0x100);
    place(
        &mut ram,
        0,
        &[Inst::AmoW {
            op: AmoOp::Add,
            rd: 3,
            rs1: 1,
            rs2: 2,
            aq: false,
            rl: false,
        }],
    );
    let inst_word = ram.read(0, 4).unwrap();
    let mut bus = TraceBus::default();
    let mut code = Ram::new(0, 0x100);
    code.write(0, 4, inst_word).unwrap();
    // Custom bus only for data at 0x2000_0000: override by reading instruction from code via Ram,
    // or simpler: put code inside TraceBus.inner = None? Let's put code in TraceBus.inner = Some(ram)
    // and check ops on 0x2000_0000 if unmapped? Wait: TraceBus with None returns 0x1234_5678 for all reads!
    // Better: put code in inner RAM and have inner = Some(ram), and test MMIO by watching ops at 0x100.
    bus.inner = Some(code);
    let mut cpu = Cpu::new(0);
    cpu.x[1] = 0x80;
    cpu.x[2] = 5;
    bus.inner
        .as_mut()
        .unwrap()
        .write(0x80, 4, 0x8000_0001)
        .unwrap();
    cpu.step(&mut bus).unwrap();
    // Fetch read 2 + 2 bytes at 0 and 2, then 1 data read(4) and 1 data write(4) at 0x80.
    let data_ops: Vec<_> = bus.ops.into_iter().filter(|op| op.1 == 0x80).collect();
    assert_eq!(
        data_ops,
        vec![(false, 0x80, 4, 0), (true, 0x80, 4, 0x8000_0006)]
    );
    assert_eq!(cpu.x[3], 0xffff_ffff_8000_0001);
}

#[test]
fn lr_sc_sequences_and_trap_clears_reservation() {
    let mut ram = Ram::new(0, 0x2000);
    ram.write(0x100, 8, 0x1122_3344_8000_0005).unwrap();
    place(
        &mut ram,
        0,
        &[
            // 0: SC.W without LR -> rd=1, no store
            Inst::ScW {
                rd: 3,
                rs1: 1,
                rs2: 2,
                aq: false,
                rl: false,
            },
            // 4: LR.W -> sign-extended, sets reservation at 0x100
            Inst::LrW {
                rd: 4,
                rs1: 1,
                aq: false,
                rl: false,
            },
            // 8: SC.W at same addr -> rd=0, stores, clears reservation
            Inst::ScW {
                rd: 5,
                rs1: 1,
                rs2: 2,
                aq: false,
                rl: false,
            },
            // 12: second SC.W -> rd=1, no store
            Inst::ScW {
                rd: 6,
                rs1: 1,
                rs2: 7,
                aq: false,
                rl: false,
            },
            // 16: LR.D -> full 64-bit, sets reservation
            Inst::LrD {
                rd: 8,
                rs1: 1,
                aq: false,
                rl: false,
            },
            // 20: SC.D to different addr (x9 = 0x108) -> rd=1, clears reservation
            Inst::ScD {
                rd: 10,
                rs1: 9,
                rs2: 2,
                aq: false,
                rl: false,
            },
            // 24: SC.D to original addr 0x100 -> rd=1 (cleared by previous failed SC)
            Inst::ScD {
                rd: 11,
                rs1: 1,
                rs2: 7,
                aq: false,
                rl: false,
            },
        ],
    );
    let mut cpu = Cpu::new(0);
    cpu.x[1] = 0x100;
    cpu.x[2] = 0x99;
    cpu.x[7] = 0x77;
    cpu.x[9] = 0x108;
    cpu.run(&mut ram, 7).unwrap();
    assert_eq!(cpu.x[3], 1);
    assert_eq!(cpu.x[4], 0xffff_ffff_8000_0005);
    assert_eq!(cpu.x[5], 0);
    assert_eq!(cpu.x[6], 1);
    assert_eq!(cpu.x[8], 0x1122_3344_0000_0099);
    assert_eq!(cpu.x[10], 1);
    assert_eq!(cpu.x[11], 1);
    assert_eq!(ram.read(0x108, 8).unwrap(), 0);

    // Taking a trap clears the reservation.
    let mut ram = Ram::new(0, 0x2000);
    place(
        &mut ram,
        0,
        &[
            Inst::LrD {
                rd: 3,
                rs1: 1,
                aq: false,
                rl: false,
            },
            Inst::Ecall,
            Inst::ScD {
                rd: 4,
                rs1: 1,
                rs2: 2,
                aq: false,
                rl: false,
            },
        ],
    );
    // Handler at 0x800: advance mepc by 4 and mret.
    place(
        &mut ram,
        0x800,
        &[
            Inst::Csrrs {
                rd: 5,
                rs1: 0,
                csr: 0x341,
            },
            Inst::Addi {
                rd: 5,
                rs1: 5,
                imm: 4,
            },
            Inst::Csrrw {
                rd: 0,
                rs1: 5,
                csr: 0x341,
            },
            Inst::Mret,
        ],
    );
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x305, 0x800);
    cpu.x[1] = 0x200;
    cpu.x[2] = 0xabcd;
    assert_eq!(cpu.tick(&mut ram), Step::Retired); // LR.D
    assert!(matches!(cpu.tick(&mut ram), Step::Exception(Trap::Ecall)));
    for _ in 0..4 {
        assert_eq!(cpu.tick(&mut ram), Step::Retired);
    }
    assert_eq!(cpu.tick(&mut ram), Step::Retired); // SC.D
    assert_eq!(cpu.x[4], 1, "trap must clear reservation");
    assert_eq!(ram.read(0x200, 8).unwrap(), 0);
}

#[test]
fn a_ext_misaligned_bus_and_page_faults() {
    let mut ram = Ram::new(0, 0x1000);
    let misaligned_cases = [
        (
            Inst::LrW {
                rd: 2,
                rs1: 1,
                aq: false,
                rl: false,
            },
            0x102u64,
            Trap::LoadMisaligned(0x102),
        ),
        (
            Inst::LrD {
                rd: 2,
                rs1: 1,
                aq: false,
                rl: false,
            },
            0x104,
            Trap::LoadMisaligned(0x104),
        ),
        (
            Inst::ScW {
                rd: 2,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
            0x102,
            Trap::StoreMisaligned(0x102),
        ),
        (
            Inst::ScD {
                rd: 2,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
            0x104,
            Trap::StoreMisaligned(0x104),
        ),
        (
            Inst::AmoW {
                op: AmoOp::Add,
                rd: 2,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
            0x101,
            Trap::StoreMisaligned(0x101),
        ),
        (
            Inst::AmoD {
                op: AmoOp::Swap,
                rd: 2,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
            0x104,
            Trap::StoreMisaligned(0x104),
        ),
    ];
    for (inst, addr, trap) in misaligned_cases {
        place(&mut ram, 0, &[inst]);
        let mut cpu = Cpu::new(0);
        cpu.x[1] = addr;
        assert_eq!(cpu.step(&mut ram), Err(trap), "{inst:?}");
    }

    // Bus faults: LR -> LoadFault, AMO -> StoreFault, SC (with valid reservation) -> StoreFault.
    let mut cpu = Cpu::new(0);
    cpu.x[1] = 0x5000;
    place(
        &mut ram,
        0,
        &[Inst::LrW {
            rd: 2,
            rs1: 1,
            aq: false,
            rl: false,
        }],
    );
    assert_eq!(cpu.step(&mut ram), Err(Trap::LoadFault(0x5000)));
    place(
        &mut ram,
        0,
        &[Inst::AmoW {
            op: AmoOp::Add,
            rd: 2,
            rs1: 1,
            rs2: 3,
            aq: false,
            rl: false,
        }],
    );
    assert_eq!(cpu.step(&mut ram), Err(Trap::StoreFault(0x5000)));

    // SC with valid reservation hitting a write fault -> StoreFault; AMO write fault -> StoreFault.
    let mut inner = Ram::new(0, 0x1000);
    place(
        &mut inner,
        0,
        &[
            Inst::LrW {
                rd: 2,
                rs1: 1,
                aq: false,
                rl: false,
            },
            Inst::ScW {
                rd: 4,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
            Inst::AmoD {
                op: AmoOp::Add,
                rd: 2,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
        ],
    );
    let mut bus = TraceBus {
        inner: Some(inner),
        ops: Vec::new(),
        fail_write: false,
        fail_read: false,
    };
    let mut cpu = Cpu::new(0);
    cpu.x[1] = 0x100;
    cpu.step(&mut bus).unwrap();
    bus.fail_write = true;
    assert_eq!(cpu.step(&mut bus), Err(Trap::StoreFault(0x100)));
    cpu.pc = 8;
    assert_eq!(cpu.step(&mut bus), Err(Trap::StoreFault(0x100)));
    bus.fail_read = true;
    assert_eq!(cpu.step(&mut bus), Err(Trap::StoreFault(0x100)));

    // Sv39 page faults & D bit setting for AMO.
    let mut ram = Ram::new(0, 0x10000);
    let (root, l1, l0) = (0x1000u64, 0x2000u64, 0x3000u64);
    let pte = |pa: u64, flags: u64| ((pa >> 12) << 10) | flags;
    ram.load(root, &pte(l1, 1).to_le_bytes()).unwrap();
    ram.load(l1, &pte(l0, 1).to_le_bytes()).unwrap();
    // VA 0x0000 -> PA 0x4000 (code: V|R|X|A|D)
    ram.load(l0, &pte(0x4000, 1 | 2 | 8 | 64 | 128).to_le_bytes())
        .unwrap();
    // VA 0x1000 -> PA 0x5000 (read-only: V|R|A|D)
    ram.load(l0 + 8, &pte(0x5000, 1 | 2 | 64 | 128).to_le_bytes())
        .unwrap();
    // VA 0x2000 -> PA 0x6000 (read-write without A/D: V|R|W)
    ram.load(l0 + 16, &pte(0x6000, 1 | 2 | 4).to_le_bytes())
        .unwrap();
    // VA 0x3000 -> unmapped

    let mut cpu = Cpu::new(0);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x180, (8 << 60) | (root >> 12));

    // LR on unmapped page -> LoadPageFault
    place(
        &mut ram,
        0x4000,
        &[Inst::LrD {
            rd: 2,
            rs1: 1,
            aq: false,
            rl: false,
        }],
    );
    cpu.x[1] = 0x3000;
    assert_eq!(cpu.step(&mut ram), Err(Trap::LoadPageFault(0x3000)));

    // AMO on read-only page -> StorePageFault
    place(
        &mut ram,
        0x4000,
        &[Inst::AmoD {
            op: AmoOp::Add,
            rd: 2,
            rs1: 1,
            rs2: 3,
            aq: false,
            rl: false,
        }],
    );
    cpu.x[1] = 0x1000;
    assert_eq!(cpu.step(&mut ram), Err(Trap::StorePageFault(0x1000)));

    // SC on read-only page -> StorePageFault (even after LR succeeded on it!)
    place(
        &mut ram,
        0x4000,
        &[
            Inst::LrD {
                rd: 2,
                rs1: 1,
                aq: false,
                rl: false,
            },
            Inst::ScD {
                rd: 4,
                rs1: 1,
                rs2: 3,
                aq: false,
                rl: false,
            },
        ],
    );
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.step(&mut ram), Err(Trap::StorePageFault(0x1000)));

    // AMO on V|R|W page succeeds and sets A and D bits.
    cpu.pc = 0;
    cpu.x[1] = 0x2000;
    cpu.x[3] = 42;
    place(
        &mut ram,
        0x4000,
        &[Inst::AmoD {
            op: AmoOp::Add,
            rd: 2,
            rs1: 1,
            rs2: 3,
            aq: false,
            rl: false,
        }],
    );
    cpu.step(&mut ram).unwrap();
    assert_eq!(ram.read(0x6000, 8).unwrap(), 42);
    assert_eq!(ram.read(l0 + 16, 8).unwrap() & (64 | 128), 64 | 128);
}
