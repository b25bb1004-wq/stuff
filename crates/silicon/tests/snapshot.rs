#![allow(clippy::unwrap_used)]
//! Tests for `Machine::snapshot` and `Machine::restore`: replay after restore, delta size,
//! idle snapshot size and chain validation.
use assay::{forall, Rng};
use silicon::inst::{encode, Inst};
use silicon::machine::{Config, Machine, Region, Snapshot, SnapshotError, RAM_BASE};
use silicon::Bus;

const RAM_SIZE: usize = 64 * 1024;
const UART_THR: u64 = 0x1000_0000;
const REGS: [u8; 10] = [1, 2, 3, 4, 7, 8, 9, 10, 11, 12];

fn cfg() -> Config {
    Config {
        ram_size: RAM_SIZE,
        ..Config::default()
    }
}

fn asm(insts: &[Inst]) -> Vec<u8> {
    insts
        .iter()
        .flat_map(|&i| encode(i).to_le_bytes())
        .collect()
}

/// Load a 32-bit constant into `rd` without sign extension (lui, addi, slli, srli).
fn li_u32(rd: u8, val: u32) -> Vec<Inst> {
    let lo = (val & 0xFFF) as i32;
    let lo_sext = (lo << 20) >> 20;
    let hi = val.wrapping_sub(lo_sext as u32) as i32 as i64;
    vec![
        Inst::Lui { rd, imm: hi },
        Inst::Addi {
            rd,
            rs1: rd,
            imm: i64::from(lo_sext),
        },
        Inst::Slli {
            rd,
            rs1: rd,
            imm: 32,
        },
        Inst::Srli {
            rd,
            rs1: rd,
            imm: 32,
        },
    ]
}

/// One random instruction: ALU, RAM window loads and stores (x5 base), UART writes (x6 base),
/// and forward branches that skip one instruction.
fn random_inst(rng: &mut Rng) -> Inst {
    let rd = *rng.pick(&REGS);
    let rs1 = *rng.pick(&REGS);
    let rs2 = *rng.pick(&REGS);
    match rng.below(10) {
        0 => Inst::Addi {
            rd,
            rs1,
            imm: rng.below(4096) as i64 - 2048,
        },
        1 => Inst::Add { rd, rs1, rs2 },
        2 => Inst::Xor { rd, rs1, rs2 },
        3 => Inst::Mul { rd, rs1, rs2 },
        4 => Inst::Sd {
            rs1: 5,
            rs2,
            imm: 8 * rng.below(128) as i64,
        },
        5 => Inst::Sb {
            rs1: 5,
            rs2,
            imm: rng.below(2048) as i64,
        },
        6 => Inst::Ld {
            rd,
            rs1: 5,
            imm: 8 * rng.below(128) as i64,
        },
        7 => {
            if rng.chance(1, 2) {
                Inst::Beq { rs1, rs2, imm: 8 }
            } else {
                Inst::Bne { rs1, rs2, imm: 8 }
            }
        }
        8 => Inst::Sb {
            rs1: 6,
            rs2,
            imm: 0,
        },
        _ => Inst::Sltu { rd, rs1, rs2 },
    }
}

/// Prologue setting x5 to the RAM window and x6 to the UART, then 120 random instructions.
fn random_program(rng: &mut Rng) -> Vec<u8> {
    let mut v = li_u32(5, (RAM_BASE + 0x1000) as u32);
    v.extend(li_u32(6, UART_THR as u32));
    for _ in 0..120 {
        v.push(random_inst(rng));
    }
    asm(&v)
}

fn run(m: &mut Machine, steps: u64) {
    for _ in 0..steps {
        m.step();
    }
}

/// Registers, pc, instret and the UART bytes written since the last call.
fn observe(m: &mut Machine) -> ([u64; 32], u64, u64, Vec<u8>) {
    (m.cpu.x, m.cpu.pc, m.cpu.instret, m.board.take_uart_output())
}

/// Same captured state, ignoring the id and base link.
fn same_state(a: &Snapshot, b: &Snapshot) -> bool {
    a.full == b.full
        && (a.ram_size, a.disk_size, a.fb_size) == (b.ram_size, b.disk_size, b.fb_size)
        && a.cpu == b.cpu
        && a.devices == b.devices
        && a.pages == b.pages
}

#[test]
fn restore_replays_identically_on_random_programs() {
    forall(40, 0x5EED_0092, |rng| {
        let mut m = Machine::new(cfg());
        m.load(RAM_BASE, &random_program(rng)).unwrap();
        run(&mut m, rng.below(100) as u64);
        let _ = m.board.take_uart_output();
        let base = m.snapshot(None);

        let steps = 100 + rng.below(100) as u64;
        run(&mut m, steps);
        let first = observe(&mut m);
        let end = m.snapshot(None);

        m.restore(&[&base]).unwrap();
        run(&mut m, steps);
        assert_eq!(observe(&mut m), first);
        assert!(same_state(&m.snapshot(None), &end));

        // A delta against `base` restores the same state on top of `base`.
        m.restore(&[&base]).unwrap();
        run(&mut m, steps);
        let before = m.snapshot(None);
        let delta = m.snapshot(Some(&base));
        assert!(!delta.full);
        assert_eq!(delta.base_id, Some(base.id));
        m.restore(&[&base]).unwrap();
        run(&mut m, 50);
        m.restore(&[&base, &delta]).unwrap();
        assert!(same_state(&m.snapshot(None), &before));
    });
}

#[test]
fn delta_after_one_ram_word_stores_one_page() {
    let mut m = Machine::new(cfg());
    m.load(RAM_BASE, &asm(&li_u32(7, 1))).unwrap();
    run(&mut m, 10);
    let full = m.snapshot(None);
    assert!(full.full);
    m.board.write(RAM_BASE + 0x3008, 8, 0xDEAD_BEEF).unwrap();
    let delta = m.snapshot(Some(&full));
    assert!(!delta.full);
    assert_eq!(delta.pages.len(), 1);
    assert_eq!(delta.pages[0].region, Region::Ram);
    assert_eq!(delta.pages[0].index, 3);
}

#[test]
fn idle_machine_snapshot_is_under_one_mib() {
    let mut m = Machine::new(Config {
        ram_size: 16 * 1024 * 1024,
        ..Config::default()
    });
    m.load(RAM_BASE, &asm(&li_u32(7, 1))).unwrap();
    run(&mut m, 1000);
    let snap = m.snapshot(None);
    assert!(snap.bytes() < 1024 * 1024, "stored {} bytes", snap.bytes());
}

#[test]
fn delta_only_chain_and_size_mismatch_are_errors() {
    let mut m = Machine::new(cfg());
    let full = m.snapshot(None);
    m.board.write(RAM_BASE, 8, 1).unwrap();
    let delta = m.snapshot(Some(&full));
    let before = m.snapshot(None);

    assert_eq!(m.restore(&[&delta]), Err(SnapshotError::NotFull));
    assert_eq!(m.restore(&[]), Err(SnapshotError::NotFull));
    assert!(
        same_state(&m.snapshot(None), &before),
        "failed restore changed the machine"
    );

    let mut other = Machine::new(Config {
        ram_size: 4096,
        ..Config::default()
    });
    assert_eq!(other.restore(&[&full]), Err(SnapshotError::SizeMismatch));
}

#[test]
fn snapshot_ids_are_unique_and_deltas_link_to_their_full_base() {
    let m = Machine::new(cfg());
    let full = m.snapshot(None);
    let other = m.snapshot(None);
    assert_ne!(full.id, other.id);
    assert_eq!(full.base_id, None);
    let delta = m.snapshot(Some(&full));
    assert_eq!(delta.base_id, Some(full.id));
    // A delta against a delta is not produced: the result is a full snapshot.
    let again = m.snapshot(Some(&delta));
    assert!(again.full);
    assert_eq!(again.base_id, None);
}

#[test]
fn chains_must_link_each_delta_to_the_one_before() {
    let mut m = Machine::new(cfg());
    let full = m.snapshot(None);
    // Page 3 changes, then reverts to its base value (zero).
    m.board.write(RAM_BASE + 0x3000, 8, 0x1234).unwrap();
    let d1 = m.snapshot(Some(&full));
    m.board.write(RAM_BASE + 0x3000, 8, 0).unwrap();
    let d2 = m.snapshot(Some(&full));
    let now = m.snapshot(None);

    // d2 was taken against the full snapshot, so [full, d1, d2] is a broken chain.
    assert_eq!(
        m.restore(&[&full, &d1, &d2]),
        Err(SnapshotError::BrokenChain)
    );
    assert!(
        same_state(&m.snapshot(None), &now),
        "broken chain changed the machine"
    );
    // Each delta restores exactly on top of the full snapshot, including the reverted page.
    m.restore(&[&full, &d1]).unwrap();
    assert_eq!(m.board.read(RAM_BASE + 0x3000, 8), Ok(0x1234));
    m.restore(&[&full, &d2]).unwrap();
    assert_eq!(m.board.read(RAM_BASE + 0x3000, 8), Ok(0));
    // A delta-of-delta is not produced; a full snapshot of d1 stands alone.
    let f1 = m.snapshot(Some(&d1));
    assert!(f1.full);
    m.restore(&[&f1]).unwrap();
    // f1 captures the machine as it is now, so the page reads back the current value.
    assert_eq!(m.board.read(RAM_BASE + 0x3000, 8), Ok(0));
    // A full snapshot cannot sit in the middle of a chain.
    assert_eq!(m.restore(&[&full, &f1]), Err(SnapshotError::BrokenChain));
}
