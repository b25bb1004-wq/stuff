//! Integration tests for UART polling, finisher, block DMA and framebuffer on `Machine`.
#![allow(clippy::unwrap_used)]

use silicon::inst::{encode, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};

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
fn uart_polling_hello_and_finisher_codes() {
    let msg = b"Hello, Monolith!\n\0";
    let mut prog = Vec::new();
    prog.extend_from_slice(&li32(10, 0x1000_0000));
    let patch_idx = prog.len();
    prog.extend_from_slice(&li_u32(11, 0));
    let loop_idx = prog.len();
    prog.extend_from_slice(&[
        Inst::Lbu {
            rd: 12,
            rs1: 11,
            imm: 0,
        },
        Inst::Beq {
            rs1: 12,
            rs2: 0,
            imm: 28,
        },
        Inst::Lbu {
            rd: 13,
            rs1: 10,
            imm: 5,
        },
        Inst::Andi {
            rd: 13,
            rs1: 13,
            imm: 0x20,
        },
        Inst::Beq {
            rs1: 13,
            rs2: 0,
            imm: -8,
        },
        Inst::Sb {
            rs1: 10,
            rs2: 12,
            imm: 0,
        },
        Inst::Addi {
            rd: 11,
            rs1: 11,
            imm: 1,
        },
    ]);
    let back = -(((prog.len() - loop_idx) * 4) as i64);
    prog.push(Inst::Jal { rd: 0, imm: back });
    prog.extend_from_slice(&li32(14, 0x0010_0000));
    prog.extend_from_slice(&li32(15, 0x5555));
    prog.push(Inst::Sw {
        rs1: 14,
        rs2: 15,
        imm: 0,
    });
    prog.push(Inst::Jal { rd: 0, imm: 0 });

    let msg_addr = RAM_BASE + (prog.len() as u64) * 4;
    let patch = li_u32(11, msg_addr as u32);
    prog[patch_idx..patch_idx + 4].copy_from_slice(&patch);

    let mut bytes = asm(&prog);
    bytes.extend_from_slice(msg);

    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &bytes).unwrap();
    assert_eq!(m.run(2), Exit::OutOfSteps);
    assert_eq!(m.run(1000), Exit::Pass);
    assert_eq!(m.board.take_uart_output(), b"Hello, Monolith!\n");

    let mut fail_prog = Vec::new();
    fail_prog.extend_from_slice(&li32(14, 0x0010_0000));
    fail_prog.extend_from_slice(&li32(15, (0x42 << 16) | 0x3333));
    fail_prog.push(Inst::Sw {
        rs1: 14,
        rs2: 15,
        imm: 0,
    });
    let mut m2 = Machine::new(Config::default());
    m2.load(RAM_BASE, &asm(&fail_prog)).unwrap();
    assert_eq!(m2.run(100), Exit::Fail(0x42));
}

#[test]
fn block_dma_read_write_with_interrupt() {
    let mut main = Vec::new();
    let mtvec_patch = main.len();
    main.extend_from_slice(&li_u32(5, 0));
    main.push(Inst::Csrrw {
        rd: 0,
        rs1: 5,
        csr: 0x305,
    });
    main.extend_from_slice(&li32(5, 0x0C00_0000 + 4 * 8));
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 2,
    });
    main.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });
    main.extend_from_slice(&li32(5, 0x0C00_2000));
    main.extend_from_slice(&li32(6, 1 << 8));
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

    main.extend_from_slice(&li32(10, 0x1000_1000));
    main.extend_from_slice(&li_u32(11, 0x8000_2000));
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 1,
    });
    main.push(Inst::Sw {
        rs1: 10,
        rs2: 6,
        imm: 0x1C,
    });
    main.push(Inst::Sd {
        rs1: 10,
        rs2: 0,
        imm: 0x00,
    });
    main.push(Inst::Sd {
        rs1: 10,
        rs2: 11,
        imm: 0x08,
    });
    main.push(Inst::Sw {
        rs1: 10,
        rs2: 6,
        imm: 0x10,
    });
    main.push(Inst::Sw {
        rs1: 10,
        rs2: 6,
        imm: 0x14,
    });
    main.push(Inst::Beq {
        rs1: 20,
        rs2: 0,
        imm: 0,
    });

    main.push(Inst::Lw {
        rd: 7,
        rs1: 11,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 7,
        rs1: 7,
        imm: 1,
    });
    main.push(Inst::Sw {
        rs1: 11,
        rs2: 7,
        imm: 0,
    });
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 1,
    });
    main.push(Inst::Sd {
        rs1: 10,
        rs2: 6,
        imm: 0x00,
    });
    main.push(Inst::Addi {
        rd: 6,
        rs1: 0,
        imm: 2,
    });
    main.push(Inst::Sw {
        rs1: 10,
        rs2: 6,
        imm: 0x14,
    });
    main.push(Inst::Addi {
        rd: 21,
        rs1: 0,
        imm: 2,
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
    main.extend_from_slice(&li32(30, 0x1000_1000));
    main.push(Inst::Sw {
        rs1: 30,
        rs2: 0,
        imm: 0x20,
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

    let mut disk = vec![0u8; 1024];
    disk[0..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    let mut m = Machine::new(Config {
        disk,
        ..Config::default()
    });
    m.load(RAM_BASE, &asm(&main)).unwrap();
    assert_eq!(m.run(1000), Exit::Pass);
    assert_eq!(&m.board.disk()[512..516], &0x1234_5679u32.to_le_bytes());
}

#[test]
fn framebuffer_pattern() {
    let mut fb_prog = Vec::new();
    fb_prog.extend_from_slice(&li32(10, 0x1000_3000));
    fb_prog.push(Inst::Lw {
        rd: 11,
        rs1: 10,
        imm: 0,
    });
    fb_prog.push(Inst::Lw {
        rd: 12,
        rs1: 10,
        imm: 4,
    });
    fb_prog.extend_from_slice(&li32(13, 0x3000_0000));
    fb_prog.push(Inst::Add {
        rd: 14,
        rs1: 11,
        rs2: 0,
    });
    fb_prog.push(Inst::Addi {
        rd: 14,
        rs1: 14,
        imm: 3,
    });
    fb_prog.push(Inst::Slli {
        rd: 14,
        rs1: 14,
        imm: 2,
    });
    fb_prog.push(Inst::Add {
        rd: 14,
        rs1: 13,
        rs2: 14,
    });
    fb_prog.extend_from_slice(&li32(15, 0xFF33_2211));
    fb_prog.push(Inst::Sw {
        rs1: 14,
        rs2: 15,
        imm: 0,
    });
    fb_prog.extend_from_slice(&li32(5, 0x0010_0000));
    fb_prog.extend_from_slice(&li32(6, 0x5555));
    fb_prog.push(Inst::Sw {
        rs1: 5,
        rs2: 6,
        imm: 0,
    });

    let mut m = Machine::new(Config {
        fb_width: 4,
        fb_height: 2,
        ..Config::default()
    });
    m.load(RAM_BASE, &asm(&fb_prog)).unwrap();
    assert_eq!(m.run(200), Exit::Pass);
    let off = 4 * (4 + 3);
    assert_eq!(
        &m.board.framebuffer()[off..off + 4],
        &[0x11, 0x22, 0x33, 0xFF]
    );
}
