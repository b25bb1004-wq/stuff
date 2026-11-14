//! Integration tests for UART RX, timer/software interrupts, and S-mode keys via PLIC ctx 1.
#![allow(clippy::unwrap_used)]

use quartz::inst::{encode, Inst};
use quartz::machine::{Board, Config, Exit, Machine, RAM_BASE};
use quartz::{Bus, Fault};

fn asm(insts: &[Inst]) -> Vec<u8> {
    let mut out = Vec::with_capacity(insts.len() * 4);
    for &inst in insts {
        out.extend_from_slice(&encode(inst).to_le_bytes());
    }
    out
}

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

fn li_u32(rd: u8, val: u32) -> [Inst; 4] {
    let [a, b] = li32(rd, val);
    [
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

#[test]
fn uart_rx_interrupt_echo() {
    let mut main = Vec::new();
    let mtvec_patch = main.len();
    main.extend_from_slice(&li_u32(5, 0));
    main.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x305,
    });
    main.extend_from_slice(&li32(5, 0x0C00_0000 + 4 * 10));
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 1,
    });
    main.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });
    main.extend_from_slice(&li32(5, 0x0C00_2000));
    main.extend_from_slice(&li32(6, 1 << 10));
    main.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });
    main.extend_from_slice(&li32(5, 0x0C20_0000));
    main.push(Inst::Sw {
        rs1: 5,
        rs2: 0,
        imm: 0,
    });
    main.extend_from_slice(&li32(5, 0x1000_0000));
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 1,
    });
    main.push(Inst::Sb {
        rs1: 5,
        rs2: 6,
        imm: 1,
    });
    main.extend_from_slice(&li32(5, 1 << 11));
    main.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x304,
    });
    main.push(Inst::Csrrsi {
        rd: 0,
        uimm: 8,
        csr: 0x300,
    });
    main.push(Inst::Addi {
        rd: 21,
        rs1: 0,
        imm: 3,
    });
    main.push(Inst::Bne {
        rs1: 20,
        rs2: 21,
        imm: 0,
    });
    main.extend_from_slice(&li32(5, 0x0010_0000));
    main.extend_from_slice(&li32(6, 0x5555));
    main.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });

    let handler_addr = RAM_BASE + (main.len() as u64) * 4;
    let p = li_u32(5, handler_addr as u32);
    main[mtvec_patch..mtvec_patch + 4].copy_from_slice(&p);

    main.extend_from_slice(&li32(28, 0x0C20_0004));
    main.push(Inst::Lw {
        rd: 29,
        rs1: 28,
        imm: 0,
    });
    main.extend_from_slice(&li32(30, 0x1000_0000));
    main.push(Inst::Lbu {
        rd: 31,
        rs1: 30,
        imm: 0,
    });
    main.push(Inst::Sb {
        rs1: 30,
        rs2: 31,
        imm: 0,
    });
    main.push(Inst::Sw {
        rs1: 28,
        rs2: 29,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 20,
        rs1: 20,
        imm: 1,
    });
    main.push(Inst::Mret);

    let mut m = Machine::new(Config::default());
    m.board.uart_input(b"OK!");
    m.load(RAM_BASE, &asm(&main)).unwrap();
    assert_eq!(m.run(2000), Exit::Pass);
    assert_eq!(m.board.take_uart_output(), b"OK!");
}

#[test]
fn timer_and_software_interrupts() {
    let mut main = Vec::new();
    let mtvec_patch = main.len();
    main.extend_from_slice(&li_u32(5, 0));
    main.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x305,
    });
    main.extend_from_slice(&li32(10, 0x0200_BFF8));
    main.push(Inst::Ld {
        rd: 6,
        rs1: 10,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 6,
        rs1: 6,
        imm: 15,
    });
    main.extend_from_slice(&li32(11, 0x0200_4000));
    main.push(Inst::Sd {
        rs1: 11,
        rs2: 6,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 0x88,
    });
    main.push(Inst::Csrrw {
        rd: 0,
        rs1: 6,
        csr: 0x304,
    });
    main.push(Inst::Csrrsi {
        rd: 0,
        uimm: 8,
        csr: 0x300,
    });
    main.push(Inst::Wfi);
    main.push(Inst::Beq {
        rs1: 20,
        rs2: 0,
        imm: -4,
    });
    main.extend_from_slice(&li32(12, 0x0200_0000));
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 1,
    });
    main.push(Inst::Sw {
        rs1: 12,
        rs2: 6,
        imm: 0,
    });
    main.push(Inst::Beq {
        rs1: 21,
        rs2: 0,
        imm: 0,
    });
    main.extend_from_slice(&li32(5, 0x0010_0000));
    main.extend_from_slice(&li32(6, 0x5555));
    main.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });

    let handler_addr = RAM_BASE + (main.len() as u64) * 4;
    let p = li_u32(5, handler_addr as u32);
    main[mtvec_patch..mtvec_patch + 4].copy_from_slice(&p);

    main.push(Inst::Csrrs {
        rd: 28,
        rs1: 0,
        csr: 0x342,
    });
    main.push(Inst::Andi {
        rd: 28,
        rs1: 28,
        imm: 0xFF,
    });
    main.push(Inst::Addi {
        rd: 29,
        rs1: 0,
        imm: 7,
    });
    main.push(Inst::Bne {
        rs1: 28,
        rs2: 29,
        imm: 28,
    });
    main.extend_from_slice(&li32(30, 0x0200_4000));
    main.push(Inst::Addi {
        rd: 31,
        rs1: 0,
        imm: -1,
    });
    main.push(Inst::Sd {
        rs1: 30,
        rs2: 31,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 20,
        rs1: 20,
        imm: 1,
    });
    main.push(Inst::Mret);
    main.extend_from_slice(&li32(30, 0x0200_0000));
    main.push(Inst::Sw {
        rs1: 30,
        rs2: 0,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 21,
        rs1: 21,
        imm: 1,
    });
    main.push(Inst::Mret);

    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &asm(&main)).unwrap();
    assert_eq!(m.run(500), Exit::Pass);
    assert_eq!(m.cpu.x[20], 1);
    assert_eq!(m.cpu.x[21], 1);
    assert!(m.board.mtime() >= 15);
}

#[test]
fn keys_in_smode_with_sei_delegated() {
    let mut prog = Vec::new();
    prog.extend_from_slice(&li32(5, 1 << 9));
    prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x303,
    });
    let stvec_patch = prog.len();
    prog.extend_from_slice(&li_u32(5, 0));
    prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x105,
    });
    let mepc_patch = prog.len();
    prog.extend_from_slice(&li_u32(5, 0));
    prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x341,
    });
    prog.extend_from_slice(&li32(5, 1 << 11));
    prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x300,
    });
    prog.push(Inst::Mret);

    let smode_addr = RAM_BASE + (prog.len() as u64) * 4;
    let p_mepc = li_u32(5, smode_addr as u32);
    prog[mepc_patch..mepc_patch + 4].copy_from_slice(&p_mepc);

    prog.extend_from_slice(&li32(5, 0x0C00_0000 + 4 * 11));
    prog.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 3,
    });
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });
    prog.extend_from_slice(&li32(5, 0x0C00_2080));
    prog.extend_from_slice(&li32(6, 1 << 11));
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });
    prog.extend_from_slice(&li32(5, 0x0C20_1000));
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 0,
        imm: 0,
    });
    prog.extend_from_slice(&li32(5, 0x1000_2008));
    prog.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 1,
    });
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });
    prog.extend_from_slice(&li32(5, 1 << 9));
    prog.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x104,
    });
    prog.push(Inst::Csrrsi {
        rd: 0,
        uimm: 2,
        csr: 0x100,
    });
    prog.push(Inst::Addi {
        rd: 21,
        rs1: 0,
        imm: 2,
    });
    prog.push(Inst::Bne {
        rs1: 20,
        rs2: 21,
        imm: 0,
    });
    prog.extend_from_slice(&li32(5, 0x0010_0000));
    prog.extend_from_slice(&li32(6, 0x5555));
    prog.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });

    let shandler_addr = RAM_BASE + (prog.len() as u64) * 4;
    let p_stvec = li_u32(5, shandler_addr as u32);
    prog[stvec_patch..stvec_patch + 4].copy_from_slice(&p_stvec);

    prog.extend_from_slice(&li32(28, 0x0C20_1004));
    prog.push(Inst::Lw {
        rd: 29,
        rs1: 28,
        imm: 0,
    });
    prog.extend_from_slice(&li32(30, 0x1000_2004));
    prog.push(Inst::Lw {
        rd: 31,
        rs1: 30,
        imm: 0,
    });
    prog.extend_from_slice(&li_u32(27, 0x8000_2000));
    prog.push(Inst::Slli {
        rd: 26,
        rs1: 20,
        imm: 2,
    });
    prog.push(Inst::Add {
        rd: 27,
        rs1: 27,
        rs2: 26,
    });
    prog.push(Inst::Sw {
        rs1: 27,
        rs2: 31,
        imm: 0,
    });
    prog.push(Inst::Sw {
        rs1: 28,
        rs2: 29,
        imm: 0,
    });
    prog.push(Inst::Addi {
        rd: 20,
        rs1: 20,
        imm: 1,
    });
    prog.push(Inst::Sret);

    let mut m = Machine::new(Config::default());
    m.board.press_key(0x1E);
    m.board.press_key(0x2C);
    m.load(RAM_BASE, &asm(&prog)).unwrap();
    assert_eq!(m.run(1000), Exit::Pass);
    assert_eq!(m.board.read(0x8000_2000, 4), Ok(0x1E));
    assert_eq!(m.board.read(0x8000_2004, 4), Ok(0x2C));
    assert_eq!(m.board.read(0x1000_2000, 4), Ok(0));
    assert_eq!(
        Board::read(&mut m.board, 0xFFFF_0000, 4),
        Err(Fault::Unmapped(0xFFFF_0000))
    );
}
