#![allow(clippy::unwrap_used)]
//! C extension (RV64C) tests: exhaustive fuzz, decoding table, reserved forms, alignment,
//! straddling fetches, xepc bit 1, and mixed 2/4-byte execution on `Machine`.
use silicon::inst::{decode_c, encode, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::{Cpu, Mode, Ram, Step, Trap};

fn place16(ram: &mut Ram, addr: u64, halfs: &[u16]) {
    let bytes: Vec<u8> = halfs.iter().flat_map(|h| h.to_le_bytes()).collect();
    ram.load(addr, &bytes).unwrap();
}

#[test]
fn decode_c_all_u16_never_panics() {
    for h in 0u16..=u16::MAX {
        let res = decode_c(h);
        if h == 0 || (h & 3) == 3 {
            assert_eq!(res, None, "h={h:#06x}");
        }
    }
}

#[test]
fn decode_c_table_every_form() {
    let cases: &[(u16, Inst)] = &[
        // Q0: C.ADDI4SPN, C.LW, C.LD, C.SW, C.SD
        (
            0x0040,
            Inst::Addi {
                rd: 8,
                rs1: 2,
                imm: 4,
            },
        ),
        (
            0x1fe0,
            Inst::Addi {
                rd: 8,
                rs1: 2,
                imm: 1020,
            },
        ),
        (
            0x4000,
            Inst::Lw {
                rd: 8,
                rs1: 8,
                imm: 0,
            },
        ),
        (
            0x5c60,
            Inst::Lw {
                rd: 8,
                rs1: 8,
                imm: 124,
            },
        ),
        (
            0x6000,
            Inst::Ld {
                rd: 8,
                rs1: 8,
                imm: 0,
            },
        ),
        (
            0x7c60,
            Inst::Ld {
                rd: 8,
                rs1: 8,
                imm: 248,
            },
        ),
        (
            0xc000,
            Inst::Sw {
                rs1: 8,
                rs2: 8,
                imm: 0,
            },
        ),
        (
            0xdc60,
            Inst::Sw {
                rs1: 8,
                rs2: 8,
                imm: 124,
            },
        ),
        (
            0xe000,
            Inst::Sd {
                rs1: 8,
                rs2: 8,
                imm: 0,
            },
        ),
        (
            0xfc60,
            Inst::Sd {
                rs1: 8,
                rs2: 8,
                imm: 248,
            },
        ),
        // Q1: C.NOP, C.ADDI, C.ADDIW, C.LI, C.ADDI16SP, C.LUI
        (
            0x0001,
            Inst::Addi {
                rd: 0,
                rs1: 0,
                imm: 0,
            },
        ),
        (
            0x1141,
            Inst::Addi {
                rd: 2,
                rs1: 2,
                imm: -16,
            },
        ),
        (
            0x2085,
            Inst::Addiw {
                rd: 1,
                rs1: 1,
                imm: 1,
            },
        ),
        (
            0x30fd,
            Inst::Addiw {
                rd: 1,
                rs1: 1,
                imm: -1,
            },
        ),
        (
            0x4085,
            Inst::Addi {
                rd: 1,
                rs1: 0,
                imm: 1,
            },
        ),
        (
            0x50fd,
            Inst::Addi {
                rd: 1,
                rs1: 0,
                imm: -1,
            },
        ),
        (
            0x6141,
            Inst::Addi {
                rd: 2,
                rs1: 2,
                imm: 16,
            },
        ),
        (
            0x717d,
            Inst::Addi {
                rd: 2,
                rs1: 2,
                imm: -16,
            },
        ),
        (0x6085, Inst::Lui { rd: 1, imm: 0x1000 }),
        (0x70fd, Inst::Lui { rd: 1, imm: -4096 }),
        // Q1 ALU: C.SRLI, C.SRAI, C.ANDI, C.SUB, C.XOR, C.OR, C.AND, C.SUBW, C.ADDW
        (
            0x9005,
            Inst::Srli {
                rd: 8,
                rs1: 8,
                imm: 33,
            },
        ),
        (
            0x9405,
            Inst::Srai {
                rd: 8,
                rs1: 8,
                imm: 33,
            },
        ),
        (
            0x987d,
            Inst::Andi {
                rd: 8,
                rs1: 8,
                imm: -1,
            },
        ),
        (
            0x8c05,
            Inst::Sub {
                rd: 8,
                rs1: 8,
                rs2: 9,
            },
        ),
        (
            0x8c25,
            Inst::Xor {
                rd: 8,
                rs1: 8,
                rs2: 9,
            },
        ),
        (
            0x8c45,
            Inst::Or {
                rd: 8,
                rs1: 8,
                rs2: 9,
            },
        ),
        (
            0x8c65,
            Inst::And {
                rd: 8,
                rs1: 8,
                rs2: 9,
            },
        ),
        (
            0x9c05,
            Inst::Subw {
                rd: 8,
                rs1: 8,
                rs2: 9,
            },
        ),
        (
            0x9c25,
            Inst::Addw {
                rd: 8,
                rs1: 8,
                rs2: 9,
            },
        ),
        // Q1 branches/jumps: C.J, C.BEQZ, C.BNEZ
        (0xa009, Inst::Jal { rd: 0, imm: 2 }),
        (0xbffd, Inst::Jal { rd: 0, imm: -2 }),
        (
            0xc009,
            Inst::Beq {
                rs1: 8,
                rs2: 0,
                imm: 2,
            },
        ),
        (
            0xdc7d,
            Inst::Beq {
                rs1: 8,
                rs2: 0,
                imm: -2,
            },
        ),
        (
            0xe009,
            Inst::Bne {
                rs1: 8,
                rs2: 0,
                imm: 2,
            },
        ),
        (
            0xfc7d,
            Inst::Bne {
                rs1: 8,
                rs2: 0,
                imm: -2,
            },
        ),
        // Q2: C.SLLI, C.LWSP, C.LDSP, C.JR, C.MV, C.EBREAK, C.JALR, C.ADD, C.SWSP, C.SDSP
        (
            0x1086,
            Inst::Slli {
                rd: 1,
                rs1: 1,
                imm: 33,
            },
        ),
        (
            0x4082,
            Inst::Lw {
                rd: 1,
                rs1: 2,
                imm: 0,
            },
        ),
        (
            0x50fe,
            Inst::Lw {
                rd: 1,
                rs1: 2,
                imm: 252,
            },
        ),
        (
            0x60a2,
            Inst::Ld {
                rd: 1,
                rs1: 2,
                imm: 8,
            },
        ),
        (
            0x70fe,
            Inst::Ld {
                rd: 1,
                rs1: 2,
                imm: 504,
            },
        ),
        (
            0x8082,
            Inst::Jalr {
                rd: 0,
                rs1: 1,
                imm: 0,
            },
        ),
        (
            0x808a,
            Inst::Add {
                rd: 1,
                rs1: 0,
                rs2: 2,
            },
        ),
        (0x9002, Inst::Ebreak),
        (
            0x9082,
            Inst::Jalr {
                rd: 1,
                rs1: 1,
                imm: 0,
            },
        ),
        (
            0x908a,
            Inst::Add {
                rd: 1,
                rs1: 1,
                rs2: 2,
            },
        ),
        (
            0xc006,
            Inst::Sw {
                rs1: 2,
                rs2: 1,
                imm: 0,
            },
        ),
        (
            0xdf86,
            Inst::Sw {
                rs1: 2,
                rs2: 1,
                imm: 252,
            },
        ),
        (
            0xe006,
            Inst::Sd {
                rs1: 2,
                rs2: 1,
                imm: 0,
            },
        ),
        (
            0xff86,
            Inst::Sd {
                rs1: 2,
                rs2: 1,
                imm: 504,
            },
        ),
    ];
    for &(h, expected) in cases {
        assert_eq!(decode_c(h), Some(expected), "h={h:#06x}");
    }
}

#[test]
fn decode_c_rejects_reserved_and_fp() {
    let illegal = [
        0x0000, // all-zero
        0x0004, // C.ADDI4SPN nzuimm=0
        0x2000, // C.FLD
        0x8000, // q0 funct3=100
        0xa000, // C.FSD
        0x2001, // C.ADDIW rd=0
        0x2005, // C.ADDIW rd=0 imm!=0
        0x6101, // C.ADDI16SP imm=0
        0x6081, // C.LUI imm=0
        0x9c45, // q1 funct3=100 bit12=1 op2=10
        0x9c65, // q1 funct3=100 bit12=1 op2=11
        0x2082, // C.FLDSP
        0x4002, // C.LWSP rd=0
        0x6002, // C.LDSP rd=0
        0x8002, // C.JR rs1=0
        0xa002, // C.FSDSP
    ];
    for h in illegal {
        assert_eq!(decode_c(h), None, "expected None for {h:#06x}");
    }
}

#[test]
fn c_execution_pc_plus_2_and_c_jalr_links_pc_plus_2() {
    let mut ram = Ram::new(0, 0x1000);
    // 0x00: c.li a0, 5        (0x4515)
    // 0x02: c.bnez a0, +4     (0xe111 -> target 0x06)
    // 0x04: c.li a0, 99       (0x457d -> skipped)
    // 0x06: c.li a1, 0x12     (0x45c9)
    // 0x08: c.jalr a1         (0x9582 -> ra = 0x0a, pc = 0x12)
    // 0x0a: c.ebreak          (0x9002)
    // 0x12: c.addi a0, 3      (0x050d)
    // 0x14: c.jr ra           (0x8082 -> pc = 0x0a)
    place16(
        &mut ram,
        0,
        &[0x4515, 0xe111, 0x457d, 0x45c9, 0x9582, 0x9002],
    );
    place16(&mut ram, 0x12, &[0x050d, 0x8082]);

    let mut cpu = Cpu::new(0);
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!((cpu.pc, cpu.x[10]), (2, 5));
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!(cpu.pc, 6);
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!((cpu.pc, cpu.x[11]), (8, 0x12));
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!((cpu.pc, cpu.x[1]), (0x12, 0x0a));
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!((cpu.pc, cpu.x[10]), (0x14, 8));
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!(cpu.pc, 0x0a);
    assert_eq!(cpu.step(&mut ram), Err(Trap::Ebreak));

    // Untaken C.BEQZ advances pc by 2.
    let mut ram = Ram::new(0, 0x100);
    place16(&mut ram, 0, &[0xc109]); // c.beqz a0, +2
    let mut cpu = Cpu::new(0);
    cpu.x[10] = 1;
    cpu.step(&mut ram).unwrap();
    assert_eq!(cpu.pc, 2);
}

#[test]
fn illegal_compressed_traps_with_halfword_as_tval() {
    let mut ram = Ram::new(0, 0x1000);
    place16(&mut ram, 0x10, &[0x0004]);
    let mut cpu = Cpu::new(0x10);
    cpu.csr_write(0x305, 0x800);
    assert_eq!(
        cpu.tick(&mut ram),
        Step::Exception(Trap::IllegalInstruction(0x0004))
    );
    assert_eq!(cpu.csr_read(0x342), Some(2));
    assert_eq!(cpu.csr_read(0x343), Some(0x0004));
    assert_eq!(cpu.csr_read(0x341), Some(0x10));
}

#[test]
fn fetch_straddling_end_of_memory_and_page_boundary() {
    // 32-bit instruction at ram_end - 2 faults at ram_end, pc stays at start.
    let mut ram = Ram::new(0, 0x100);
    let word = encode(Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 7,
    });
    ram.load(0xfe, &word.to_le_bytes()[..2]).unwrap();
    let mut cpu = Cpu::new(0xfe);
    assert_eq!(cpu.step(&mut ram), Err(Trap::FetchFault(0x100)));
    assert_eq!(cpu.pc, 0xfe);

    // 16-bit compressed instruction at ram_end - 2 succeeds.
    place16(&mut ram, 0xfe, &[0x451d]); // c.li a0, 7
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!((cpu.pc, cpu.x[10]), (0x100, 7));

    // 32-bit instruction straddling a page boundary where the second page is unmapped.
    let mut ram = Ram::new(0, 0x4000);
    let root = 0x1000u64;
    let l1 = 0x2000u64;
    let l0 = 0x3000u64;
    let pte = |pa: u64, flags: u64| ((pa >> 12) << 10) | flags;
    ram.load(root, &pte(l1, 1).to_le_bytes()).unwrap();
    ram.load(l1, &pte(l0, 1).to_le_bytes()).unwrap();
    // Map only virtual page 0 -> physical 0x0000 (X|R|A|D|V), leave virtual page 1 unmapped.
    ram.load(l0, &pte(0, 1 | 2 | 8 | 64 | 128).to_le_bytes())
        .unwrap();
    ram.load(0xffe, &word.to_le_bytes()[..2]).unwrap();
    let mut cpu = Cpu::new(0xffe);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x180, (8 << 60) | (root >> 12));
    assert_eq!(cpu.step(&mut ram), Err(Trap::InstPageFault(0x1000)));
    assert_eq!(cpu.pc, 0xffe);
}

#[test]
fn misa_has_a_and_c_and_xepc_preserves_bit_1() {
    let mut cpu = Cpu::new(0x102);
    let misa = cpu.csr_read(0x301).unwrap();
    assert_eq!(misa & 5, 5, "A (bit 0) and C (bit 2) must be set");
    let mut ram = Ram::new(0, 0x1000);
    place16(&mut ram, 0x102, &[0x4509]); // c.li a0, 2
    ram.load(0x800, &encode(Inst::Mret).to_le_bytes()).unwrap();
    cpu.csr_write(0x305, 0x800);
    // Simulate a trap taken at 0x102 -> mepc = 0x102 -> mret returns to 0x102.
    cpu.csr_write(0x341, 0x103);
    assert_eq!(cpu.csr_read(0x341), Some(0x102));
    cpu.pc = 0x800;
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!(cpu.pc, 0x102);
    assert_eq!(cpu.step(&mut ram), Ok(()));
    assert_eq!((cpu.pc, cpu.x[10]), (0x104, 2));
}

#[test]
fn mixed_compressed_and_32bit_program_on_machine() {
    let mut m = Machine::new(Config::default());
    // Mix 32-bit setup with 16-bit loop body:
    // 0x00: lui t0, 0x10000 (UART)      [4 bytes]
    // 0x04: c.li a0, 3                  [2 bytes] (0x450d)
    // 0x06: addi a1, zero, 'A'          [4 bytes]
    // 0x0a: sb a1, 0(t0)                [4 bytes]
    // 0x0e: c.addi a1, 1                [2 bytes] (0x0585)
    // 0x10: c.addi a0, -1               [2 bytes] (0x157d)
    // 0x12: c.bnez a0, -8               [2 bytes] -> 0x0a (0xfd65)
    // 0x14: lui t1, 0x100               [4 bytes] (finisher 0x10_0000)
    // 0x18: addi t2, zero, 0x5555...    -> lui t2, 5; addi t2, t2, 0x555; sw t2, 0(t1)
    let mut code = Vec::new();
    code.extend_from_slice(
        &encode(Inst::Lui {
            rd: 5,
            imm: 0x1000_0000,
        })
        .to_le_bytes(),
    );
    code.extend_from_slice(&0x450du16.to_le_bytes());
    code.extend_from_slice(
        &encode(Inst::Addi {
            rd: 11,
            rs1: 0,
            imm: i64::from(b'A'),
        })
        .to_le_bytes(),
    );
    code.extend_from_slice(
        &encode(Inst::Sb {
            rs1: 5,
            rs2: 11,
            imm: 0,
        })
        .to_le_bytes(),
    );
    code.extend_from_slice(&0x0585u16.to_le_bytes());
    code.extend_from_slice(&0x157du16.to_le_bytes());
    code.extend_from_slice(&0xfd65u16.to_le_bytes());
    code.extend_from_slice(
        &encode(Inst::Lui {
            rd: 6,
            imm: 0x0010_0000,
        })
        .to_le_bytes(),
    );
    code.extend_from_slice(&encode(Inst::Lui { rd: 7, imm: 0x5000 }).to_le_bytes());
    code.extend_from_slice(
        &encode(Inst::Addi {
            rd: 7,
            rs1: 7,
            imm: 0x555,
        })
        .to_le_bytes(),
    );
    code.extend_from_slice(
        &encode(Inst::Sw {
            rs1: 6,
            rs2: 7,
            imm: 0,
        })
        .to_le_bytes(),
    );
    m.load(RAM_BASE, &code).unwrap();
    assert_eq!(m.run(200), Exit::Pass);
    assert_eq!(m.board.take_uart_output(), b"ABC");
}
