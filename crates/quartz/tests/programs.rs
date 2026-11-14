#![allow(clippy::unwrap_used)]
//! Whole programs built with `encode` and run on RAM until EBREAK.
use assay::{forall, Rng};
use quartz::inst::{encode, Inst};
use quartz::{Bus, Cpu, Ram, Trap};

/// Byte offset from instruction `from` to instruction `to` (both are indices).
fn off(from: usize, to: usize) -> i64 {
    (to as i64 - from as i64) * 4
}

/// Write a program at address 0.
fn load_program(ram: &mut Ram, prog: &[Inst]) {
    let bytes: Vec<u8> = prog.iter().flat_map(|i| encode(*i).to_le_bytes()).collect();
    ram.load(0, &bytes).unwrap();
}

#[test]
fn fibonacci_of_fifty() {
    // x5 = remaining count, x1 = F(k), x2 = F(k+1), x3 = scratch.
    let prog = [
        Inst::Addi {
            rd: 5,
            rs1: 0,
            imm: 50,
        },
        Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: 0,
        },
        Inst::Addi {
            rd: 2,
            rs1: 0,
            imm: 1,
        },
        // loop (index 3):
        Inst::Beq {
            rs1: 5,
            rs2: 0,
            imm: off(3, 9),
        },
        Inst::Add {
            rd: 3,
            rs1: 1,
            rs2: 2,
        },
        Inst::Addi {
            rd: 1,
            rs1: 2,
            imm: 0,
        },
        Inst::Addi {
            rd: 2,
            rs1: 3,
            imm: 0,
        },
        Inst::Addi {
            rd: 5,
            rs1: 5,
            imm: -1,
        },
        Inst::Jal {
            rd: 0,
            imm: off(8, 3),
        },
        // end (index 9):
        Inst::Ebreak,
    ];
    let mut ram = Ram::new(0, 0x1000);
    load_program(&mut ram, &prog);
    let mut cpu = Cpu::new(0);
    assert_eq!(cpu.run(&mut ram, 10_000), Err(Trap::Ebreak));
    assert_eq!(cpu.x[1], 12_586_269_025, "F(50)");
    assert_eq!(cpu.pc, 36);
}

#[test]
fn bubble_sort_sixteen_random_values() {
    let base = 0x800u64;
    let n = 16u64;
    // x10 = array base, x11 = n, x12 = passes left, x13 = j, x14 = p, x5/x6 = pair.
    let prog = [
        Inst::Addi {
            rd: 12,
            rs1: 11,
            imm: -1,
        },
        // outer (1):
        Inst::Beq {
            rs1: 12,
            rs2: 0,
            imm: off(1, 18),
        },
        Inst::Addi {
            rd: 13,
            rs1: 0,
            imm: 0,
        },
        Inst::Addi {
            rd: 14,
            rs1: 10,
            imm: 0,
        },
        // inner (4):
        Inst::Beq {
            rs1: 13,
            rs2: 12,
            imm: off(4, 16),
        },
        Inst::Ld {
            rd: 5,
            rs1: 14,
            imm: 0,
        },
        Inst::Ld {
            rd: 6,
            rs1: 14,
            imm: 8,
        },
        Inst::Bltu {
            rs1: 6,
            rs2: 5,
            imm: off(7, 11),
        },
        Inst::Addi {
            rd: 13,
            rs1: 13,
            imm: 1,
        },
        Inst::Addi {
            rd: 14,
            rs1: 14,
            imm: 8,
        },
        Inst::Jal {
            rd: 0,
            imm: off(10, 4),
        },
        // swap (11):
        Inst::Sd {
            rs1: 14,
            rs2: 6,
            imm: 0,
        },
        Inst::Sd {
            rs1: 14,
            rs2: 5,
            imm: 8,
        },
        Inst::Addi {
            rd: 13,
            rs1: 13,
            imm: 1,
        },
        Inst::Addi {
            rd: 14,
            rs1: 14,
            imm: 8,
        },
        Inst::Jal {
            rd: 0,
            imm: off(15, 4),
        },
        // outer_end (16):
        Inst::Addi {
            rd: 12,
            rs1: 12,
            imm: -1,
        },
        Inst::Jal {
            rd: 0,
            imm: off(17, 1),
        },
        // done (18):
        Inst::Ebreak,
    ];
    forall(4, 0x5027, |r: &mut Rng| {
        let mut ram = Ram::new(0, 0x1000);
        load_program(&mut ram, &prog);
        let mut values: Vec<u64> = (0..n).map(|_| r.u64()).collect();
        for (i, v) in values.iter().enumerate() {
            ram.write(base + 8 * i as u64, 8, *v).unwrap();
        }
        let mut cpu = Cpu::new(0);
        cpu.x[10] = base;
        cpu.x[11] = n;
        assert_eq!(cpu.run(&mut ram, 100_000), Err(Trap::Ebreak));
        values.sort_unstable();
        for (i, v) in values.iter().enumerate() {
            assert_eq!(ram.read(base + 8 * i as u64, 8), Ok(*v));
        }
    });
}
