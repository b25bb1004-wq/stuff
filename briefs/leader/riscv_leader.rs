//! Leader check for T13 (official riscv-tests, misaligned access, counter writes, PMP storage).
//! Copied to crates/silicon/tests/riscv_leader.rs.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use silicon::inst::{encode, AmoOp, Inst};
use silicon::machine::{Config, Machine, RAM_BASE};
use silicon::{Bus, Cpu, Mode, Ram, Trap};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/riscv-tests");

#[test]
fn leader_official_riscv_tests_pass() {
    let idx = std::fs::read_to_string(format!("{DIR}/index.txt")).unwrap();
    let mut failed = Vec::new();
    let mut n = 0;
    for l in idx.lines() {
        let w: Vec<&str> = l.split(' ').collect();
        let (name, tohost, size) = (w[0], u64::from_str_radix(w[1], 16).unwrap(), w[2]);
        let bin = std::fs::read(format!("{DIR}/{name}.bin")).unwrap();
        assert_eq!(bin.len().to_string(), size, "{name}: size");
        let mut m = Machine::new(Config {
            ram_size: 1 << 20,
            ..Config::default()
        });
        m.load(RAM_BASE, &bin).unwrap();
        let mut v = 0;
        for _ in 0..500 {
            let _ = m.run(10_000);
            v = m.board.read(tohost, 8).unwrap();
            if v != 0 {
                break;
            }
        }
        if v != 1 {
            failed.push(if v == 0 {
                format!("{name}: no result")
            } else {
                format!("{name}: test {}", v >> 1)
            });
        }
        n += 1;
    }
    assert_eq!(n, 110, "index.txt should list 110 tests");
    assert!(failed.is_empty(), "failing: {failed:?}");
    let skipped = std::fs::read_to_string(format!("{DIR}/SKIPPED")).unwrap();
    assert_eq!(skipped.lines().count(), 1);
    assert!(skipped.starts_with("rv64mi-p-breakpoint:"));
}

/// Machine-mode hart and RAM at 0; the instruction goes at address 0.
fn one(inst: Inst, ram: &mut Ram) -> Cpu {
    ram.load(0, &encode(inst).to_le_bytes()).unwrap();
    Cpu::new(0)
}

#[test]
fn leader_misaligned_loads_and_stores_match_bytes() {
    let mut r = Rng::new(0x13_3a11_0001);
    type Ld = fn(u8, u8, i64) -> Inst;
    let loads: [(Ld, usize, bool); 7] = [
        (|rd, rs1, imm| Inst::Lb { rd, rs1, imm }, 1, true),
        (|rd, rs1, imm| Inst::Lbu { rd, rs1, imm }, 1, false),
        (|rd, rs1, imm| Inst::Lh { rd, rs1, imm }, 2, true),
        (|rd, rs1, imm| Inst::Lhu { rd, rs1, imm }, 2, false),
        (|rd, rs1, imm| Inst::Lw { rd, rs1, imm }, 4, true),
        (|rd, rs1, imm| Inst::Lwu { rd, rs1, imm }, 4, false),
        (|rd, rs1, imm| Inst::Ld { rd, rs1, imm }, 8, true),
    ];
    type St = fn(u8, u8, i64) -> Inst;
    let stores: [(St, usize); 4] = [
        (|rs1, rs2, imm| Inst::Sb { rs1, rs2, imm }, 1),
        (|rs1, rs2, imm| Inst::Sh { rs1, rs2, imm }, 2),
        (|rs1, rs2, imm| Inst::Sw { rs1, rs2, imm }, 4),
        (|rs1, rs2, imm| Inst::Sd { rs1, rs2, imm }, 8),
    ];
    for round in 0..40 {
        let mut data = [0u8; 64];
        r.fill(&mut data);
        for off in 0..8u64 {
            for &(mk, size, signed) in &loads {
                let mut ram = Ram::new(0, 0x10000);
                ram.load(0x8000, &data).unwrap();
                let imm = r.below(16) as i64 - 8;
                let mut cpu = one(mk(10, 11, imm), &mut ram);
                cpu.x[11] = (0x8010 + off).wrapping_sub(imm as u64);
                assert_eq!(
                    cpu.step(&mut ram),
                    Ok(()),
                    "round {round} load size {size} off {off}"
                );
                let at = 0x10 + off as usize;
                let mut v = 0u64;
                for i in (0..size).rev() {
                    v = (v << 8) | u64::from(data[at + i]);
                }
                if signed && size < 8 {
                    let sh = 64 - 8 * size as u32;
                    v = (((v << sh) as i64) >> sh) as u64;
                }
                assert_eq!(
                    cpu.x[10], v,
                    "round {round} load size {size} signed {signed} off {off}"
                );
                assert_eq!(cpu.pc, 4);
            }
            for &(mk, size) in &stores {
                let mut ram = Ram::new(0, 0x10000);
                ram.load(0x8000, &data).unwrap();
                let mut cpu = one(mk(11, 12, 3), &mut ram);
                let val = r.u64();
                cpu.x[11] = 0x8010 + off - 3;
                cpu.x[12] = val;
                assert_eq!(
                    cpu.step(&mut ram),
                    Ok(()),
                    "round {round} store size {size} off {off}"
                );
                let mut want = data;
                let at = 0x10 + off as usize;
                want[at..at + size].copy_from_slice(&val.to_le_bytes()[..size]);
                let got: Vec<u8> = (0..64)
                    .map(|i| ram.read(0x8000 + i, 1).unwrap() as u8)
                    .collect();
                assert_eq!(got, want, "round {round} store size {size} off {off}");
            }
        }
    }
}

#[test]
fn leader_misaligned_atomics_still_trap() {
    let mut ram = Ram::new(0, 0x10000);
    let amo = Inst::AmoW {
        op: AmoOp::Add,
        rd: 10,
        rs1: 11,
        rs2: 12,
        aq: false,
        rl: false,
    };
    let mut cpu = one(amo, &mut ram);
    cpu.x[11] = 0x8002;
    assert_eq!(cpu.step(&mut ram), Err(Trap::StoreMisaligned(0x8002)));
    let mut cpu = one(
        Inst::LrD {
            rd: 10,
            rs1: 11,
            aq: false,
            rl: false,
        },
        &mut ram,
    );
    cpu.x[11] = 0x8004;
    assert_eq!(cpu.step(&mut ram), Err(Trap::LoadMisaligned(0x8004)));
    let mut cpu = one(
        Inst::ScW {
            rd: 10,
            rs1: 11,
            rs2: 12,
            aq: false,
            rl: false,
        },
        &mut ram,
    );
    cpu.x[11] = 0x8001;
    assert_eq!(cpu.step(&mut ram), Err(Trap::StoreMisaligned(0x8001)));
}

const V: u64 = 1;
const R: u64 = 2;
const W: u64 = 4;
const X: u64 = 8;
const A: u64 = 64;
const D: u64 = 128;
const ROOT: u64 = 0x1000;
const L1: u64 = 0x2000;
const L0: u64 = 0x3000;
const CODE_PA: u64 = 0x2_0000;
const PAGE0_PA: u64 = 0x10_0000;
const PAGE1_PA: u64 = 0x30_0000;

fn put(ram: &mut Ram, addr: u64, value: u64) {
    ram.load(addr, &value.to_le_bytes()).unwrap();
}
fn pte(pa: u64, flags: u64) -> u64 {
    ((pa >> 12) << 10) | flags
}
fn va(v0: u64, off: u64) -> u64 {
    (1 << 30) | (2 << 21) | (v0 << 12) | off
}

/// Supervisor hart under Sv39: code page at VPN0 5, data page 0 mapped, page 1 mapped only when
/// `second` is true (to a physical page far from page 0).
fn paged(inst: Inst, second: bool) -> (Cpu, Ram) {
    let mut ram = Ram::new(0, 0x40_0000);
    put(&mut ram, ROOT + 8, pte(L1, V));
    put(&mut ram, L1 + 16, pte(L0, V));
    put(&mut ram, L0 + 8 * 5, pte(CODE_PA, V | R | X | A));
    put(&mut ram, L0, pte(PAGE0_PA, V | R | W | A | D));
    if second {
        put(&mut ram, L0 + 8, pte(PAGE1_PA, V | R | W | A | D));
    }
    ram.load(CODE_PA, &encode(inst).to_le_bytes()).unwrap();
    for i in 0..8 {
        ram.load(PAGE0_PA + 0xff8 + i, &[0x10 + i as u8]).unwrap();
        ram.load(PAGE1_PA + i, &[0x80 + i as u8]).unwrap();
    }
    let mut cpu = Cpu::new(va(5, 0));
    cpu.csr_write(0x180, (8 << 60) | (ROOT >> 12));
    cpu.mode = Mode::Supervisor;
    (cpu, ram)
}

#[test]
fn leader_page_crossing_accesses() {
    for off in 0xff9..=0xfffu64 {
        let first = va(0, off);
        let crossing_at = va(1, 0);
        // Load across two pages mapped to unrelated physical pages.
        let (mut cpu, mut ram) = paged(
            Inst::Ld {
                rd: 10,
                rs1: 11,
                imm: 0,
            },
            true,
        );
        cpu.x[11] = first;
        assert_eq!(cpu.step(&mut ram), Ok(()), "load off {off:#x}");
        let mut want = [0u8; 8];
        for (i, b) in want.iter_mut().enumerate() {
            let a = off + i as u64;
            *b = if a < 0x1000 {
                0x10 + (a - 0xff8) as u8
            } else {
                0x80 + (a - 0x1000) as u8
            };
        }
        assert_eq!(cpu.x[10], u64::from_le_bytes(want), "load off {off:#x}");
        // Second page unmapped: page fault at its first byte, nothing loaded or stored.
        let (mut cpu, mut ram) = paged(
            Inst::Ld {
                rd: 10,
                rs1: 11,
                imm: 0,
            },
            false,
        );
        cpu.x[11] = first;
        cpu.x[10] = 0x5a5a;
        assert_eq!(
            cpu.step(&mut ram),
            Err(Trap::LoadPageFault(crossing_at)),
            "load fault off {off:#x}"
        );
        assert_eq!(cpu.x[10], 0x5a5a);
        let (mut cpu, mut ram) = paged(
            Inst::Sd {
                rs1: 11,
                rs2: 12,
                imm: 0,
            },
            false,
        );
        cpu.x[11] = first;
        cpu.x[12] = u64::MAX;
        let before: Vec<u64> = (0..8)
            .map(|i| ram.read(PAGE0_PA + 0xff8 + i, 1).unwrap())
            .collect();
        assert_eq!(
            cpu.step(&mut ram),
            Err(Trap::StorePageFault(crossing_at)),
            "store fault off {off:#x}"
        );
        let after: Vec<u64> = (0..8)
            .map(|i| ram.read(PAGE0_PA + 0xff8 + i, 1).unwrap())
            .collect();
        assert_eq!(before, after, "partial store at off {off:#x}");
    }
}

#[test]
fn leader_counter_write_wins() {
    let mut ram = Ram::new(0, 0x10000);
    let prog = [
        encode(Inst::Csrrwi {
            rd: 0,
            uimm: 0,
            csr: 0xb02,
        }),
        encode(Inst::Csrrs {
            rd: 10,
            rs1: 0,
            csr: 0xb02,
        }),
        encode(Inst::Addi {
            rd: 0,
            rs1: 0,
            imm: 0,
        }),
        encode(Inst::Csrrs {
            rd: 11,
            rs1: 0,
            csr: 0xb02,
        }),
    ];
    for (i, w) in prog.iter().enumerate() {
        ram.load(4 * i as u64, &w.to_le_bytes()).unwrap();
    }
    let mut cpu = Cpu::new(0);
    for _ in 0..4 {
        cpu.step(&mut ram).unwrap();
    }
    assert_eq!(
        (cpu.x[10], cpu.x[11]),
        (0, 2),
        "minstret after write, then two more"
    );
    assert_eq!(cpu.csr_read(0xb02), Some(3));
}

#[test]
fn leader_pmp_registers() {
    let mut cpu = Cpu::new(0);
    for a in 0x3b0..=0x3bfu16 {
        assert!(cpu.csr_write(a, u64::MAX), "pmpaddr {a:#x}");
        assert_eq!(cpu.csr_read(a), Some((1 << 54) - 1), "pmpaddr {a:#x}");
        assert!(cpu.csr_write(a, 0x1234_5678));
        assert_eq!(cpu.csr_read(a), Some(0x1234_5678));
    }
    for a in [0x3a0u16, 0x3a2] {
        assert!(cpu.csr_write(a, u64::MAX));
        assert_eq!(
            cpu.csr_read(a),
            Some(0x9f9f_9f9f_9f9f_9f9f),
            "pmpcfg {a:#x}"
        );
        assert!(cpu.csr_write(a, 0x0102_0304_0506_0718));
        assert_eq!(cpu.csr_read(a), Some(0x0102_0304_0506_0718));
    }
    for a in [0x3a1u16, 0x3a3] {
        assert_eq!(cpu.csr_read(a), None, "{a:#x} does not exist on RV64");
        assert!(!cpu.csr_write(a, 1));
    }
}
