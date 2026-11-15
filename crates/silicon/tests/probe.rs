//! Tests for the trap, ECALL and device counters exposed by `Machine::probe` and `probe_reset`.
#![allow(clippy::unwrap_used)]

use silicon::inst::{encode, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::probe::{EcallRecord, Probe};
use silicon::Bus;

const FINISHER: u64 = 0x0010_0000;
const CLINT_MTIMECMP: u64 = 0x0200_4000;
const UART_THR: u64 = 0x1000_0000;
const BLOCK: u64 = 0x1000_1000;

fn asm(insts: &[Inst]) -> Vec<u8> {
    insts
        .iter()
        .flat_map(|&i| encode(i).to_le_bytes())
        .collect()
}

/// Two instructions that load `val` into `rd` (lui + addi, fixed length).
fn li32(rd: u8, val: u32) -> [Inst; 2] {
    let lo = (val & 0xFFF) as i32;
    let lo_sext = (lo << 20) >> 20;
    let hi = val.wrapping_sub(lo_sext as u32) as i32 as i64;
    [
        Inst::Lui { rd, imm: hi },
        Inst::Addi {
            rd,
            rs1: rd,
            imm: i64::from(lo_sext),
        },
    ]
}

/// Like `li32` but zero-extends, so addresses at or above 0x8000_0000 (RAM) load correctly.
fn li_u32(rd: u8, val: u32) -> Vec<Inst> {
    let [a, b] = li32(rd, val);
    vec![
        a,
        b,
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

/// Write the test finisher with `Pass`, which ends `Machine::run`.
fn finish() -> Vec<Inst> {
    let mut v = li32(10, FINISHER as u32).to_vec();
    v.extend(li32(11, 0x5555));
    v.push(Inst::Sw {
        rs1: 10,
        rs2: 11,
        imm: 0,
    });
    v
}

/// Handler that skips the trapping instruction (mepc += 4) and returns with mret.
fn skip_handler() -> Vec<Inst> {
    vec![
        Inst::Csrrs {
            rd: 6,
            rs1: 0,
            csr: 0x341,
        },
        Inst::Addi {
            rd: 6,
            rs1: 6,
            imm: 4,
        },
        Inst::Csrrw {
            rd: 0,
            rs1: 6,
            csr: 0x341,
        },
        Inst::Mret,
    ]
}

/// Program: two ECALLs (a7 = 5 and 9), an illegal word, an EBREAK, then finish.
fn ecall_program() -> Vec<u8> {
    let build = |handler: u64| -> Vec<u8> {
        let mut main = li_u32(5, handler as u32);
        main.push(Inst::Csrrw {
            rd: 0,
            rs1: 5,
            csr: 0x305,
        });
        main.push(Inst::Addi {
            rd: 17,
            rs1: 0,
            imm: 5,
        });
        main.push(Inst::Ecall);
        main.push(Inst::Addi {
            rd: 17,
            rs1: 0,
            imm: 9,
        });
        main.push(Inst::Ecall);
        main.push(Inst::Ebreak);
        let mut bytes = asm(&main);
        bytes.extend_from_slice(&0u32.to_le_bytes()); // illegal instruction
        bytes.extend(asm(&finish()));
        bytes.extend(asm(&skip_handler()));
        bytes
    };
    // The handler is the last block; its address is the length of everything before it.
    let handler_len = asm(&skip_handler()).len() as u64;
    let len = build(0).len() as u64 - handler_len;
    build(RAM_BASE + len)
}

#[test]
fn ecall_and_exception_counters_by_cause() {
    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &ecall_program()).unwrap();
    assert_eq!(m.run(10_000), Exit::Pass);
    let p = m.probe();
    assert_eq!(p.exceptions[11], 2, "two M-mode ECALLs");
    assert_eq!(p.exceptions[2], 1, "illegal instruction");
    assert_eq!(p.exceptions[3], 1, "EBREAK");
    let rest: u64 = p
        .exceptions
        .iter()
        .enumerate()
        .filter(|(i, _)| ![2, 3, 11].contains(i))
        .map(|(_, v)| *v)
        .sum();
    assert_eq!(rest, 0);
    assert!(p.interrupts.iter().all(|&v| v == 0));
}

#[test]
fn ecall_ring_records_a7_and_instret() {
    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &ecall_program()).unwrap();
    assert_eq!(m.run(10_000), Exit::Pass);
    let p = m.probe();
    assert_eq!(p.ecall_total(), 2);
    let recs = p.ecalls();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].a7, 5);
    assert_eq!(recs[1].a7, 9);
    assert!(recs[1].instret > recs[0].instret);
    assert_eq!(
        recs[0],
        EcallRecord {
            a7: 5,
            instret: recs[0].instret
        }
    );
}

#[test]
fn timer_interrupt_counter() {
    let build = |handler: u64| -> Vec<u8> {
        let mut main = li_u32(5, handler as u32);
        main.push(Inst::Csrrw {
            rd: 0,
            rs1: 5,
            csr: 0x305,
        });
        main.push(Inst::Addi {
            rd: 5,
            rs1: 0,
            imm: 0x80,
        });
        main.push(Inst::Csrrs {
            rd: 0,
            rs1: 5,
            csr: 0x304,
        });
        main.push(Inst::Addi {
            rd: 5,
            rs1: 0,
            imm: 8,
        });
        main.push(Inst::Csrrs {
            rd: 0,
            rs1: 5,
            csr: 0x300,
        });
        main.extend(li32(6, CLINT_MTIMECMP as u32));
        main.push(Inst::Sd {
            rs1: 6,
            rs2: 0,
            imm: 0,
        });
        main.push(Inst::Jal { rd: 0, imm: 0 });
        let mut bytes = asm(&main);
        bytes.extend(asm(&finish()));
        bytes
    };
    let handler_len = asm(&finish()).len() as u64;
    let len = build(0).len() as u64 - handler_len;
    let prog = build(RAM_BASE + len);
    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &prog).unwrap();
    assert_eq!(m.run(10_000), Exit::Pass);
    let p = m.probe();
    assert_eq!(p.interrupts[7], 1, "one machine timer interrupt");
    assert!(p.exceptions.iter().all(|&v| v == 0));
    assert_eq!(p.ecall_total(), 0);
}

#[test]
fn uart_byte_counters() {
    let mut m = Machine::new(Config::default());
    m.board.uart_input(b"ab");
    for b in b"xyz" {
        m.board.write(UART_THR, 1, u64::from(*b)).unwrap();
    }
    let p = m.probe();
    assert_eq!(p.uart_in, 2);
    assert_eq!(p.uart_out, 3);
    assert_eq!(m.board.take_uart_output(), b"xyz");
}

#[test]
fn block_read_and_write_counters() {
    let mut m = Machine::new(Config {
        disk: vec![0xAB; 1024],
        ..Config::default()
    });
    let set = |m: &mut Machine, off: u64, size: u8, v: u64| {
        m.board.write(BLOCK + off, size, v).unwrap();
    };
    set(&mut m, 0x00, 8, 0);
    set(&mut m, 0x08, 8, RAM_BASE + 0x1000);
    set(&mut m, 0x10, 4, 1);
    set(&mut m, 0x14, 4, 1); // disk -> RAM
    set(&mut m, 0x14, 4, 2); // RAM -> disk
    set(&mut m, 0x14, 4, 7); // unknown command: not counted
    let p = m.probe();
    assert_eq!((p.block_reads, p.block_writes), (1, 1));
}

#[test]
fn probe_reset_zeroes_everything_and_keeps_guest_state() {
    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &ecall_program()).unwrap();
    assert_eq!(m.run(10_000), Exit::Pass);
    m.board.uart_input(b"q");
    m.board.write(UART_THR, 1, 0x41).unwrap();
    assert_ne!(m.probe(), Probe::default());
    let pc = m.cpu.pc;
    let instret = m.cpu.instret;
    m.probe_reset();
    assert_eq!(m.probe(), Probe::default());
    assert_eq!((m.cpu.pc, m.cpu.instret), (pc, instret));
}
