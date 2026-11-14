#![allow(clippy::unwrap_used)]
//! Execution: every instruction against plain Rust arithmetic, plus trap and commit rules.
use assay::{forall, Rng};
use quartz::inst::{encode, Inst};
use quartz::{Bus, Cpu, Fault, Ram, Trap};

/// Sign-extend a 32-bit value to 64 bits.
fn s32(v: u32) -> u64 {
    i64::from(v as i32) as u64
}

/// Write instructions into RAM starting at `at`.
fn place(ram: &mut Ram, at: u64, insts: &[Inst]) {
    let bytes: Vec<u8> = insts
        .iter()
        .flat_map(|i| encode(*i).to_le_bytes())
        .collect();
    ram.load(at, &bytes).unwrap();
}

/// Run one instruction from pc 0 with the given registers set.
fn one(inst: Inst, regs: &[(usize, u64)]) -> Cpu {
    let mut cpu = Cpu::new(0);
    for &(i, v) in regs {
        cpu.x[i] = v;
    }
    let mut ram = Ram::new(0, 0x1000);
    place(&mut ram, 0, &[inst]);
    cpu.step(&mut ram).unwrap();
    cpu
}

/// Edge-heavy random 64-bit value.
fn word(r: &mut Rng) -> u64 {
    match r.below(8) {
        0 => 0,
        1 => 1,
        2 => u64::MAX,
        3 => 1 << 63,
        4 => 0x8000_0000,
        5 => 0xffff_ffff,
        _ => r.u64(),
    }
}

type Rr = fn(u8, u8, u8) -> Inst;
type Ri = fn(u8, u8, i64) -> Inst;
type Bin = fn(u64, u64) -> u64;
type Imm = fn(u64, i64) -> u64;
type Case = (Inst, &'static [(usize, u64)], u64);

#[test]
fn register_ops_match_rust_arithmetic() {
    let cases: &[(Rr, Bin)] = &[
        (
            |rd, rs1, rs2| Inst::Add { rd, rs1, rs2 },
            |a, b| a.wrapping_add(b),
        ),
        (
            |rd, rs1, rs2| Inst::Sub { rd, rs1, rs2 },
            |a, b| a.wrapping_sub(b),
        ),
        (
            |rd, rs1, rs2| Inst::Sll { rd, rs1, rs2 },
            |a, b| a << (b % 64),
        ),
        (
            |rd, rs1, rs2| Inst::Slt { rd, rs1, rs2 },
            |a, b| ((a as i64) < (b as i64)) as u64,
        ),
        (
            |rd, rs1, rs2| Inst::Sltu { rd, rs1, rs2 },
            |a, b| (a < b) as u64,
        ),
        (|rd, rs1, rs2| Inst::Xor { rd, rs1, rs2 }, |a, b| a ^ b),
        (
            |rd, rs1, rs2| Inst::Srl { rd, rs1, rs2 },
            |a, b| a >> (b % 64),
        ),
        (
            |rd, rs1, rs2| Inst::Sra { rd, rs1, rs2 },
            |a, b| ((a as i64) >> (b % 64)) as u64,
        ),
        (|rd, rs1, rs2| Inst::Or { rd, rs1, rs2 }, |a, b| a | b),
        (|rd, rs1, rs2| Inst::And { rd, rs1, rs2 }, |a, b| a & b),
        (
            |rd, rs1, rs2| Inst::Mul { rd, rs1, rs2 },
            |a, b| a.wrapping_mul(b),
        ),
        (
            |rd, rs1, rs2| Inst::Mulh { rd, rs1, rs2 },
            |a, b| (((a as i64 as i128) * (b as i64 as i128)) >> 64) as u64,
        ),
        (
            |rd, rs1, rs2| Inst::Mulhsu { rd, rs1, rs2 },
            |a, b| (((a as i64 as i128) * (b as i128)) >> 64) as u64,
        ),
        (
            |rd, rs1, rs2| Inst::Mulhu { rd, rs1, rs2 },
            |a, b| (((a as u128) * (b as u128)) >> 64) as u64,
        ),
        (
            |rd, rs1, rs2| Inst::Div { rd, rs1, rs2 },
            |a, b| {
                if b == 0 {
                    u64::MAX
                } else {
                    ((a as i64 as i128) / (b as i64 as i128)) as u64
                }
            },
        ),
        (
            |rd, rs1, rs2| Inst::Divu { rd, rs1, rs2 },
            |a, b| a.checked_div(b).unwrap_or(u64::MAX),
        ),
        (
            |rd, rs1, rs2| Inst::Rem { rd, rs1, rs2 },
            |a, b| {
                if b == 0 {
                    a
                } else {
                    ((a as i64 as i128) % (b as i64 as i128)) as u64
                }
            },
        ),
        (
            |rd, rs1, rs2| Inst::Remu { rd, rs1, rs2 },
            |a, b| if b == 0 { a } else { a % b },
        ),
        (
            |rd, rs1, rs2| Inst::Addw { rd, rs1, rs2 },
            |a, b| s32((a as u32).wrapping_add(b as u32)),
        ),
        (
            |rd, rs1, rs2| Inst::Subw { rd, rs1, rs2 },
            |a, b| s32((a as u32).wrapping_sub(b as u32)),
        ),
        (
            |rd, rs1, rs2| Inst::Sllw { rd, rs1, rs2 },
            |a, b| s32((a as u32) << (b % 32)),
        ),
        (
            |rd, rs1, rs2| Inst::Srlw { rd, rs1, rs2 },
            |a, b| s32((a as u32) >> (b % 32)),
        ),
        (
            |rd, rs1, rs2| Inst::Sraw { rd, rs1, rs2 },
            |a, b| s32(((a as u32 as i32) >> (b % 32)) as u32),
        ),
        (
            |rd, rs1, rs2| Inst::Mulw { rd, rs1, rs2 },
            |a, b| s32((a as u32).wrapping_mul(b as u32)),
        ),
        (
            |rd, rs1, rs2| Inst::Divw { rd, rs1, rs2 },
            |a, b| {
                let (x, y) = (a as u32 as i32 as i128, b as u32 as i32 as i128);
                if y == 0 {
                    u64::MAX
                } else {
                    s32(((x / y) as i64) as u32)
                }
            },
        ),
        (
            |rd, rs1, rs2| Inst::Divuw { rd, rs1, rs2 },
            |a, b| (a as u32).checked_div(b as u32).map_or(u64::MAX, s32),
        ),
        (
            |rd, rs1, rs2| Inst::Remw { rd, rs1, rs2 },
            |a, b| {
                let (x, y) = (a as u32 as i32 as i128, b as u32 as i32 as i128);
                if y == 0 {
                    s32(a as u32)
                } else {
                    s32(((x % y) as i64) as u32)
                }
            },
        ),
        (
            |rd, rs1, rs2| Inst::Remuw { rd, rs1, rs2 },
            |a, b| {
                if b as u32 == 0 {
                    s32(a as u32)
                } else {
                    s32((a as u32) % (b as u32))
                }
            },
        ),
    ];
    forall(300, 0xA11, |r| {
        let (a, b) = (word(r), word(r));
        for (make, reference) in cases {
            let got = one(make(3, 1, 2), &[(1, a), (2, b)]).x[3];
            assert_eq!(
                got,
                reference(a, b),
                "{:?} a={a:#x} b={b:#x}",
                make(3, 1, 2)
            );
        }
    });
}

#[test]
fn immediate_ops_match_rust_arithmetic() {
    let cases: &[(Ri, Imm)] = &[
        (
            |rd, rs1, imm| Inst::Addi { rd, rs1, imm },
            |a, i| a.wrapping_add(i as u64),
        ),
        (
            |rd, rs1, imm| Inst::Slti { rd, rs1, imm },
            |a, i| ((a as i64) < i) as u64,
        ),
        (
            |rd, rs1, imm| Inst::Sltiu { rd, rs1, imm },
            |a, i| (a < i as u64) as u64,
        ),
        (
            |rd, rs1, imm| Inst::Xori { rd, rs1, imm },
            |a, i| a ^ i as u64,
        ),
        (
            |rd, rs1, imm| Inst::Ori { rd, rs1, imm },
            |a, i| a | i as u64,
        ),
        (
            |rd, rs1, imm| Inst::Andi { rd, rs1, imm },
            |a, i| a & i as u64,
        ),
        (
            |rd, rs1, imm| Inst::Addiw { rd, rs1, imm },
            |a, i| s32((a as u32).wrapping_add(i as u32)),
        ),
    ];
    let shifts: &[(Ri, Imm)] = &[
        (|rd, rs1, imm| Inst::Slli { rd, rs1, imm }, |a, s| a << s),
        (|rd, rs1, imm| Inst::Srli { rd, rs1, imm }, |a, s| a >> s),
        (
            |rd, rs1, imm| Inst::Srai { rd, rs1, imm },
            |a, s| ((a as i64) >> s) as u64,
        ),
    ];
    let w_shifts: &[(Ri, Imm)] = &[
        (
            |rd, rs1, imm| Inst::Slliw { rd, rs1, imm },
            |a, s| s32((a as u32) << s),
        ),
        (
            |rd, rs1, imm| Inst::Srliw { rd, rs1, imm },
            |a, s| s32((a as u32) >> s),
        ),
        (
            |rd, rs1, imm| Inst::Sraiw { rd, rs1, imm },
            |a, s| s32(((a as u32 as i32) >> s) as u32),
        ),
    ];
    forall(300, 0xB22, |r| {
        let a = word(r);
        let imm = r.range(0, 4096) as i64 - 2048;
        for (make, reference) in cases {
            assert_eq!(one(make(3, 1, imm), &[(1, a)]).x[3], reference(a, imm));
        }
        let sh = r.range(0, 64) as i64;
        for (make, reference) in shifts {
            assert_eq!(one(make(3, 1, sh), &[(1, a)]).x[3], reference(a, sh));
        }
        let sh5 = r.range(0, 32) as i64;
        for (make, reference) in w_shifts {
            assert_eq!(one(make(3, 1, sh5), &[(1, a)]).x[3], reference(a, sh5));
        }
    });
}

#[test]
fn division_edge_cases() {
    let min = 1u64 << 63;
    assert_eq!(
        one(
            Inst::Div {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 5), (2, 0)]
        )
        .x[3],
        u64::MAX
    );
    assert_eq!(
        one(
            Inst::Rem {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 5), (2, 0)]
        )
        .x[3],
        5
    );
    assert_eq!(
        one(
            Inst::Divu {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 5), (2, 0)]
        )
        .x[3],
        u64::MAX
    );
    assert_eq!(
        one(
            Inst::Remu {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 5), (2, 0)]
        )
        .x[3],
        5
    );
    assert_eq!(
        one(
            Inst::Div {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, min), (2, u64::MAX)]
        )
        .x[3],
        min
    );
    assert_eq!(
        one(
            Inst::Rem {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, min), (2, u64::MAX)]
        )
        .x[3],
        0
    );
    assert_eq!(
        one(
            Inst::Divw {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 5), (2, 0)]
        )
        .x[3],
        u64::MAX
    );
    assert_eq!(
        one(
            Inst::Remw {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 0xffff_ffff_9234_5678), (2, 0)]
        )
        .x[3],
        0xffff_ffff_9234_5678
    );
    assert_eq!(
        one(
            Inst::Divw {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 0x8000_0000), (2, u64::MAX)]
        )
        .x[3],
        0xffff_ffff_8000_0000
    );
    assert_eq!(
        one(
            Inst::Remw {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 0x8000_0000), (2, u64::MAX)]
        )
        .x[3],
        0
    );
    assert_eq!(
        one(
            Inst::Divuw {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 7), (2, 0)]
        )
        .x[3],
        u64::MAX
    );
    assert_eq!(
        one(
            Inst::Remuw {
                rd: 3,
                rs1: 1,
                rs2: 2
            },
            &[(1, 0x1_0000_0007), (2, 0)]
        )
        .x[3],
        7
    );
}

#[test]
fn shift_edge_cases() {
    let sll = |a: u64, b: u64| {
        one(
            Inst::Sll {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &[(1, a), (2, b)],
        )
        .x[3]
    };
    assert_eq!(sll(1, 63), 1 << 63);
    assert_eq!(sll(1, 64), 1, "shift amount is taken mod 64");
    assert_eq!(sll(1, 0), 1);
    let srl = |a: u64, b: u64| {
        one(
            Inst::Srl {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &[(1, a), (2, b)],
        )
        .x[3]
    };
    assert_eq!(srl(u64::MAX, 63), 1);
    let sra = |a: u64, b: u64| {
        one(
            Inst::Sra {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &[(1, a), (2, b)],
        )
        .x[3]
    };
    assert_eq!(sra(1 << 63, 63), u64::MAX);
    assert_eq!(sra(1 << 63, 0), 1 << 63);
    let sllw = |a: u64, b: u64| {
        one(
            Inst::Sllw {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &[(1, a), (2, b)],
        )
        .x[3]
    };
    assert_eq!(sllw(1, 31), 0xffff_ffff_8000_0000);
    assert_eq!(sllw(1, 32), 1, "W shift amount is taken mod 32");
    let srlw = |a: u64, b: u64| {
        one(
            Inst::Srlw {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &[(1, a), (2, b)],
        )
        .x[3]
    };
    assert_eq!(srlw(0x8000_0000, 31), 1);
    let sraw = |a: u64, b: u64| {
        one(
            Inst::Sraw {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &[(1, a), (2, b)],
        )
        .x[3]
    };
    assert_eq!(sraw(0x8000_0000, 31), u64::MAX);
    assert_eq!(sraw(0x8000_0000, 0), 0xffff_ffff_8000_0000);
    let imm = |inst: Inst, a: u64| one(inst, &[(1, a)]).x[3];
    assert_eq!(
        imm(
            Inst::Slliw {
                rd: 3,
                rs1: 1,
                imm: 31
            },
            1
        ),
        0xffff_ffff_8000_0000
    );
    assert_eq!(
        imm(
            Inst::Srai {
                rd: 3,
                rs1: 1,
                imm: 63
            },
            1 << 63
        ),
        u64::MAX
    );
    assert_eq!(
        imm(
            Inst::Sraiw {
                rd: 3,
                rs1: 1,
                imm: 31
            },
            0x8000_0000
        ),
        u64::MAX
    );
    assert_eq!(
        imm(
            Inst::Srli {
                rd: 3,
                rs1: 1,
                imm: 63
            },
            u64::MAX
        ),
        1
    );
}

#[test]
fn w_forms_sign_extend_their_results() {
    let add = one(
        Inst::Addw {
            rd: 3,
            rs1: 1,
            rs2: 2,
        },
        &[(1, 0x7fff_ffff), (2, 1)],
    );
    assert_eq!(add.x[3], 0xffff_ffff_8000_0000);
    let addi = one(
        Inst::Addiw {
            rd: 3,
            rs1: 1,
            imm: -1,
        },
        &[(1, 0)],
    );
    assert_eq!(addi.x[3], u64::MAX);
}

#[test]
fn x0_is_hardwired_to_zero() {
    let c = one(
        Inst::Addi {
            rd: 0,
            rs1: 0,
            imm: 5,
        },
        &[],
    );
    assert_eq!(c.x[0], 0);
    let c = one(Inst::Jal { rd: 0, imm: 8 }, &[]);
    assert_eq!(c.x[0], 0);
    assert_eq!(c.pc, 8);
}

#[test]
fn jalr_clears_bit_zero_of_target() {
    let c = one(
        Inst::Jalr {
            rd: 5,
            rs1: 1,
            imm: 0,
        },
        &[(1, 0x101)],
    );
    assert_eq!(c.pc, 0x100);
    assert_eq!(c.x[5], 4);
}

#[test]
fn misaligned_targets_trap_without_changing_state() {
    let cases: [Case; 3] = [
        (Inst::Jal { rd: 1, imm: 2 }, &[], 2),
        (
            Inst::Beq {
                rs1: 0,
                rs2: 0,
                imm: 6,
            },
            &[],
            6,
        ),
        (
            Inst::Jalr {
                rd: 1,
                rs1: 2,
                imm: 0,
            },
            &[(2, 0x102)],
            0x102,
        ),
    ];
    for (inst, regs, target) in cases {
        let mut cpu = Cpu::new(0);
        for &(i, v) in regs {
            cpu.x[i] = v;
        }
        let before = cpu.clone();
        let mut ram = Ram::new(0, 0x1000);
        place(&mut ram, 0, &[inst]);
        assert_eq!(
            cpu.step(&mut ram),
            Err(Trap::FetchMisaligned(target)),
            "{inst:?}"
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn untaken_branch_to_misaligned_offset_does_not_trap() {
    let mut cpu = Cpu::new(0);
    cpu.x[1] = 1;
    let mut ram = Ram::new(0, 0x1000);
    place(
        &mut ram,
        0,
        &[Inst::Beq {
            rs1: 1,
            rs2: 0,
            imm: 2,
        }],
    );
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.pc, 4);
    assert_eq!(cpu.instret, 1);
}

#[test]
fn failed_steps_change_nothing() {
    let mut ram = Ram::new(0, 0x1000);
    let mut cpu = Cpu::new(0x40);
    cpu.x[1] = 0x5000;
    cpu.x[2] = 0x1234;
    cpu.instret = 9;
    let cases = [
        (
            Inst::Ld {
                rd: 4,
                rs1: 1,
                imm: 0,
            },
            Trap::LoadFault(0x5000),
        ),
        (
            Inst::Sd {
                rs1: 1,
                rs2: 2,
                imm: 0,
            },
            Trap::StoreFault(0x5000),
        ),
        (Inst::Ecall, Trap::Ecall),
        (Inst::Ebreak, Trap::Ebreak),
    ];
    for (inst, trap) in cases {
        place(&mut ram, 0x40, &[inst]);
        let before = cpu.clone();
        assert_eq!(cpu.step(&mut ram), Err(trap), "{inst:?}");
        assert_eq!(cpu, before);
    }
    ram.load(0x40, &[0, 0, 0, 0]).unwrap();
    let before = cpu.clone();
    assert_eq!(cpu.step(&mut ram), Err(Trap::IllegalInstruction(0)));
    assert_eq!(cpu, before);
    cpu.pc = 0x2000;
    let before = cpu.clone();
    assert_eq!(cpu.step(&mut ram), Err(Trap::FetchFault(0x2000)));
    assert_eq!(cpu, before);
}

#[test]
fn successful_step_retires_one_instruction() {
    let mut cpu = Cpu::new(0);
    let mut ram = Ram::new(0, 0x1000);
    place(
        &mut ram,
        0,
        &[Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: 7,
        }],
    );
    cpu.step(&mut ram).unwrap();
    assert_eq!((cpu.x[1], cpu.pc, cpu.instret), (7, 4, 1));
}

#[test]
fn loads_sign_and_zero_extend_each_width() {
    let mem = [0x80, 0xff, 0x7f, 0x80, 0x01, 0x02, 0x03, 0x84];
    let load = |inst: Inst| {
        let mut cpu = Cpu::new(0);
        let mut ram = Ram::new(0, 0x1000);
        place(&mut ram, 0, &[inst]);
        ram.load(0x100, &mem).unwrap();
        cpu.x[1] = 0x100;
        cpu.step(&mut ram).unwrap();
        cpu.x[3]
    };
    let (rd, rs1, imm) = (3, 1, 0);
    assert_eq!(load(Inst::Lb { rd, rs1, imm }), 0xffff_ffff_ffff_ff80);
    assert_eq!(load(Inst::Lbu { rd, rs1, imm }), 0x80);
    assert_eq!(load(Inst::Lh { rd, rs1, imm }), 0xffff_ffff_ffff_ff80);
    assert_eq!(load(Inst::Lhu { rd, rs1, imm }), 0xff80);
    assert_eq!(load(Inst::Lw { rd, rs1, imm }), 0xffff_ffff_807f_ff80);
    assert_eq!(load(Inst::Lwu { rd, rs1, imm }), 0x807f_ff80);
    assert_eq!(load(Inst::Ld { rd, rs1, imm }), 0x8403_0201_807f_ff80);
}

#[test]
fn store_then_load_round_trips_at_every_width() {
    forall(400, 0x51, |r| {
        let idx = r.below(4);
        // Store closures take (base register, value register, offset).
        let stores: [fn(u8, u8, i64) -> Inst; 4] = [
            |rs1, rs2, imm| Inst::Sb { rs1, rs2, imm },
            |rs1, rs2, imm| Inst::Sh { rs1, rs2, imm },
            |rs1, rs2, imm| Inst::Sw { rs1, rs2, imm },
            |rs1, rs2, imm| Inst::Sd { rs1, rs2, imm },
        ];
        let signed: [Ri; 4] = [
            |rd, rs1, imm| Inst::Lb { rd, rs1, imm },
            |rd, rs1, imm| Inst::Lh { rd, rs1, imm },
            |rd, rs1, imm| Inst::Lw { rd, rs1, imm },
            |rd, rs1, imm| Inst::Ld { rd, rs1, imm },
        ];
        let unsigned: [Ri; 4] = [
            |rd, rs1, imm| Inst::Lbu { rd, rs1, imm },
            |rd, rs1, imm| Inst::Lhu { rd, rs1, imm },
            |rd, rs1, imm| Inst::Lwu { rd, rs1, imm },
            |rd, rs1, imm| Inst::Ld { rd, rs1, imm },
        ];
        let shift = 64 - 8 * (1u32 << idx);
        let mask = u64::MAX >> shift;
        let addr = 0x1000 + r.range(0, 0xff8);
        let v = r.u64();
        for (load, expect) in [
            (signed[idx], (((v & mask) << shift) as i64 >> shift) as u64),
            (unsigned[idx], v & mask),
        ] {
            let mut ram = Ram::new(0, 0x2000);
            place(&mut ram, 0, &[stores[idx](1, 2, 0), load(3, 1, 0)]);
            let mut cpu = Cpu::new(0);
            cpu.x[1] = addr;
            cpu.x[2] = v;
            cpu.run(&mut ram, 2).unwrap();
            assert_eq!(cpu.x[3], expect, "width index {idx}");
            assert_eq!(ram.read(addr, 1 << idx).unwrap(), v & mask);
        }
    });
}

#[test]
fn bus_faults_map_to_traps() {
    let mut cpu = Cpu::new(0);
    let mut ram = Ram::new(0, 0x1000);
    place(
        &mut ram,
        0,
        &[Inst::Ld {
            rd: 1,
            rs1: 2,
            imm: 0,
        }],
    );
    cpu.x[2] = 0x1000 - 4;
    assert_eq!(cpu.step(&mut ram), Err(Trap::LoadFault(0x1000 - 4)));
    assert_eq!(ram.read(0x2000, 8), Err(Fault::Unmapped(0x2000)));
}
