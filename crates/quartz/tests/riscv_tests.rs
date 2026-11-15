//! Integration tests for the official riscv-tests suite and T13 behavior.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use assay::forall;
use quartz::inst::Inst;
use quartz::machine::{Config, Machine, RAM_BASE};
use quartz::{Bus, Cpu, Mode, Ram, Step, Trap};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/riscv-tests");

fn encode_i(opcode: u32, rd: u32, funct3: u32, rs1: u32, imm: i32) -> u32 {
    let imm12 = (imm as u32) & 0xfff;
    (imm12 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
}

fn encode_s(opcode: u32, funct3: u32, rs1: u32, rs2: u32, imm: i32) -> u32 {
    let imm12 = (imm as u32) & 0xfff;
    let hi = (imm12 >> 5) & 0x7f;
    let lo = imm12 & 0x1f;
    (hi << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (lo << 7) | opcode
}

fn encode_amo(funct5: u32, funct3: u32, rd: u32, rs1: u32, rs2: u32) -> u32 {
    (funct5 << 27) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | 0x2f
}

fn encode_csr(funct3: u32, rd: u32, rs1_or_uimm: u32, csr: u16) -> u32 {
    (u32::from(csr) << 20) | (rs1_or_uimm << 15) | (funct3 << 12) | (rd << 7) | 0x73
}

fn write_inst(ram: &mut Ram, addr: u64, word: u32) {
    ram.write(addr, 4, u64::from(word)).unwrap();
}

#[test]
fn all_riscv_tests_pass() {
    let index_path = Path::new(FIXTURES).join("index.txt");
    let index = fs::read_to_string(&index_path).unwrap();
    let mut failures = Vec::new();
    let mut count = 0usize;

    for line in index.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        count += 1;
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap();
        let tohost_hex = parts.next().unwrap();
        let size: usize = parts.next().unwrap().parse().unwrap();
        let tohost = u64::from_str_radix(tohost_hex, 16).unwrap();

        let bin_path = Path::new(FIXTURES).join(format!("{name}.bin"));
        let bin = fs::read(&bin_path).unwrap();
        assert_eq!(bin.len(), size, "{name}: binary size mismatch");

        let mut m = Machine::new(Config {
            ram_size: 1024 * 1024,
            ..Config::default()
        });
        m.load(RAM_BASE, &bin).unwrap();

        let mut passed = false;
        let mut detail = String::from("timeout after 5000000 steps");
        for _ in 0..500 {
            for _ in 0..10_000 {
                m.step();
            }
            let val = m.board.read(tohost, 8).unwrap();
            if val != 0 {
                if val == 1 {
                    passed = true;
                } else {
                    detail = format!("failed at test {}", val >> 1);
                }
                break;
            }
        }
        if !passed {
            failures.push(format!("{name}: {detail}"));
        }
    }

    assert!(count > 0, "index.txt must not be empty");
    if !failures.is_empty() {
        let mut msg = format!("{} of {count} riscv-tests failed:\n", failures.len());
        for f in &failures {
            let _ = writeln!(msg, "  {f}");
        }
        panic!("{msg}");
    }
}

#[test]
fn misaligned_loads_and_stores_all_offsets() {
    // Byte-model check for 2, 4, and 8-byte signed/unsigned loads and stores at every offset 0..8.
    forall(64, 0x1357_2468_9abc_def0, |r| {
        let mut model = [0u8; 64];
        r.fill(&mut model);
        let mut ram = Ram::new(0x1000, 0x1000);
        ram.load(0x1020, &model).unwrap();

        for size in [2u8, 4, 8] {
            let load_f3_unsigned = match size {
                2 => 5, // LHU
                4 => 6, // LWU
                8 => 3, // LD
                _ => unreachable!(),
            };
            let load_f3_signed = match size {
                2 => 1, // LH
                4 => 2, // LW
                8 => 3, // LD
                _ => unreachable!(),
            };
            let store_f3 = match size {
                2 => 1, // SH
                4 => 2, // SW
                8 => 3, // SD
                _ => unreachable!(),
            };

            for off in 0u64..8 {
                let addr = 0x1020 + off;
                let idx = usize::try_from(off).unwrap();
                let mut expected_u = 0u64;
                for i in 0..usize::from(size) {
                    expected_u |= u64::from(model[idx + i]) << (8 * i);
                }
                let shift = 64 - 8 * u32::from(size);
                let expected_s = (((expected_u << shift) as i64) >> shift) as u64;

                // Test unsigned and signed load.
                let mut cpu = Cpu::new(0x1000);
                cpu.x[1] = addr;
                write_inst(&mut ram, 0x1000, encode_i(0x03, 2, load_f3_unsigned, 1, 0));
                write_inst(&mut ram, 0x1004, encode_i(0x03, 3, load_f3_signed, 1, 0));
                cpu.step(&mut ram).unwrap();
                cpu.step(&mut ram).unwrap();
                assert_eq!(cpu.x[2], expected_u, "unsigned load size={size} off={off}");
                assert_eq!(cpu.x[3], expected_s, "signed load size={size} off={off}");

                // Test store and verify against byte model.
                let val = r.u64();
                cpu.x[4] = val;
                cpu.pc = 0x1008;
                write_inst(&mut ram, 0x1008, encode_s(0x23, store_f3, 1, 4, 0));
                cpu.step(&mut ram).unwrap();
                for i in 0..usize::from(size) {
                    model[idx + i] = (val >> (8 * i)) as u8;
                    assert_eq!(
                        ram.read(addr + i as u64, 1).unwrap() as u8,
                        model[idx + i],
                        "store byte {i} size={size} off={off}"
                    );
                }
            }
        }
    });
}

#[test]
fn page_crossing_fault_reports_second_page_and_no_partial_store() {
    // Map code page VA 0x0000 -> PA 0x8000_0000 (RX)
    // Map data page 0 VA 0x1000 -> PA 0x8000_1000 (RW)
    // Leave data page 1 VA 0x2000 unmapped.
    let mut ram = Ram::new(RAM_BASE, 0x1_0000);
    let root = RAM_BASE + 0x2000;
    let l1 = RAM_BASE + 0x3000;
    let l0 = RAM_BASE + 0x4000;

    let pte_ptr = |pa: u64| ((pa >> 12) << 10) | 1; // V
    let pte_leaf = |pa: u64, perm: u64| ((pa >> 12) << 10) | perm | 0xc1; // V | A | D | perm

    ram.write(root, 8, pte_ptr(l1)).unwrap();
    ram.write(l1, 8, pte_ptr(l0)).unwrap();
    ram.write(l0, 8, pte_leaf(RAM_BASE, 0x0a)).unwrap(); // VA 0x0000: R | X
    ram.write(l0 + 8, 8, pte_leaf(RAM_BASE + 0x1000, 0x06))
        .unwrap(); // VA 0x1000: R | W

    // Seed last 4 bytes of data page 0 with known pattern.
    ram.write(RAM_BASE + 0x1ffc, 4, 0xdead_beef).unwrap();

    // Place SD x2, 0(x1) at VA 0x0000, LD x3, 0(x1) at VA 0x0004.
    write_inst(&mut ram, RAM_BASE, encode_s(0x23, 3, 1, 2, 0));
    write_inst(&mut ram, RAM_BASE + 4, encode_i(0x03, 3, 3, 1, 0));

    let mut cpu = Cpu::new(0);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x180, (8u64 << 60) | (root >> 12)); // Sv39
    cpu.csr_write(0x305, 0x8000); // mtvec
    cpu.x[1] = 0x1ffc; // 4 bytes in mapped page 0x1000, 4 bytes in unmapped page 0x2000
    cpu.x[2] = 0x1122_3344_5566_7788;

    let step1 = cpu.tick(&mut ram);
    assert_eq!(step1, Step::Exception(Trap::StorePageFault(0x2000)));
    assert_eq!(
        ram.read(RAM_BASE + 0x1ffc, 4).unwrap(),
        0xdead_beef,
        "partial store must not modify first page"
    );

    // Now test page-crossing LD faulting on VA 0x2000.
    cpu.pc = 4;
    cpu.mode = Mode::Supervisor;
    let step2 = cpu.tick(&mut ram);
    assert_eq!(step2, Step::Exception(Trap::LoadPageFault(0x2000)));
}

#[test]
fn misaligned_lr_sc_and_amo_still_trap() {
    let mut ram = Ram::new(0x1000, 0x1000);
    // LR.W x2, (x1) -> funct5=0x02, funct3=2
    write_inst(&mut ram, 0x1000, encode_amo(0x02, 2, 2, 1, 0));
    // SC.W x2, x3, (x1) -> funct5=0x03, funct3=2
    write_inst(&mut ram, 0x1004, encode_amo(0x03, 2, 2, 1, 3));
    // AMOADD.D x2, x3, (x1) -> funct5=0x00, funct3=3
    write_inst(&mut ram, 0x1008, encode_amo(0x00, 3, 2, 1, 3));

    let mut cpu = Cpu::new(0x1000);
    cpu.x[1] = 0x1802; // misaligned for both 4 and 8
    assert_eq!(cpu.step(&mut ram), Err(Trap::LoadMisaligned(0x1802)));
    cpu.pc = 0x1004;
    assert_eq!(cpu.step(&mut ram), Err(Trap::StoreMisaligned(0x1802)));
    cpu.pc = 0x1008;
    assert_eq!(cpu.step(&mut ram), Err(Trap::StoreMisaligned(0x1802)));
}

#[test]
fn counter_write_takes_precedence_over_increment() {
    let mut ram = Ram::new(0x1000, 0x1000);
    // csrwi minstret, 0 -> CSRRWI x0, 0xb02, 0
    write_inst(&mut ram, 0x1000, encode_csr(5, 0, 0, 0xb02));
    // csrr a0, minstret -> CSRRS x10, 0xb02, x0
    write_inst(&mut ram, 0x1004, encode_csr(2, 10, 0, 0xb02));
    // csrw mcycle, x1 -> CSRRW x0, 0xb00, x1
    write_inst(&mut ram, 0x1008, encode_csr(1, 0, 1, 0xb00));
    // csrr a1, mcycle -> CSRRS x11, 0xb00, x0
    write_inst(&mut ram, 0x100c, encode_csr(2, 11, 0, 0xb00));

    let mut cpu = Cpu::new(0x1000);
    cpu.instret = 100;
    cpu.csr_write(0xb00, 200);
    cpu.x[1] = 42;

    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.instret, 0, "csrwi minstret, 0 must not self-increment");
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.x[10], 0, "csrr after csrwi must read 0");
    assert_eq!(cpu.instret, 1, "csrr itself increments instret after read");

    cpu.step(&mut ram).unwrap();
    assert_eq!(
        cpu.csr_read(0xb00),
        Some(42),
        "csrw mcycle must not self-increment"
    );
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.x[11], 42, "csrr after csrw must read written value");
    assert_eq!(
        cpu.csr_read(0xb00),
        Some(43),
        "csrr itself increments mcycle after read"
    );
}

#[test]
fn pmp_csr_masks_and_odd_pmpcfg_illegal() {
    let mut cpu = Cpu::new(0);
    assert!(cpu.csr_write(0x3a0, u64::MAX));
    assert_eq!(cpu.csr_read(0x3a0), Some(0x9f9f_9f9f_9f9f_9f9f));
    assert!(cpu.csr_write(0x3a2, u64::MAX));
    assert_eq!(cpu.csr_read(0x3a2), Some(0x9f9f_9f9f_9f9f_9f9f));

    for i in 0..16u16 {
        let addr = 0x3b0 + i;
        assert!(cpu.csr_write(addr, u64::MAX));
        assert_eq!(cpu.csr_read(addr), Some((1u64 << 54) - 1));
    }

    assert_eq!(cpu.csr_read(0x3a1), None);
    assert_eq!(cpu.csr_read(0x3a3), None);

    let mut ram = Ram::new(0x1000, 0x1000);
    let w1 = encode_csr(2, 10, 0, 0x3a1);
    let w3 = encode_csr(2, 10, 0, 0x3a3);
    write_inst(&mut ram, 0x1000, w1);
    write_inst(&mut ram, 0x1004, w3);
    cpu.pc = 0x1000;
    assert_eq!(cpu.step(&mut ram), Err(Trap::IllegalInstruction(w1)));
    cpu.pc = 0x1004;
    assert_eq!(cpu.step(&mut ram), Err(Trap::IllegalInstruction(w3)));
    let _ = Inst::Ecall;
}
