//! Unit tests for RV64IMAC instructions, pseudos, expressions, labels, macros, and alignment.
#![allow(clippy::unwrap_used)]

use chisel::assemble;
use quartz::inst::{decode, encode, AmoOp, Inst};

fn asm_u32(src: &str) -> u32 {
    let out = assemble(src, 0).unwrap_or_else(|e| panic!("failed '{src}': {e}"));
    assert_eq!(out.bytes.len(), 4, "expected 4 bytes for '{src}'");
    u32::from_le_bytes(out.bytes[..4].try_into().unwrap())
}

fn asm_u16(src: &str) -> u16 {
    let out = assemble(src, 0).unwrap_or_else(|e| panic!("failed '{src}': {e}"));
    assert_eq!(out.bytes.len(), 2, "expected 2 bytes for '{src}'");
    u16::from_le_bytes(out.bytes[..2].try_into().unwrap())
}

#[test]
fn all_rv64ima_zicsr_priv_formats_and_pseudos() {
    assert_eq!(
        asm_u32("nop"),
        encode(Inst::Addi {
            rd: 0,
            rs1: 0,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("mv a1, t2"),
        encode(Inst::Addi {
            rd: 11,
            rs1: 7,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("not a0, a1"),
        encode(Inst::Xori {
            rd: 10,
            rs1: 11,
            imm: -1
        })
    );
    assert_eq!(
        asm_u32("neg a0, a1"),
        encode(Inst::Sub {
            rd: 10,
            rs1: 0,
            rs2: 11
        })
    );
    assert_eq!(
        asm_u32("negw a0, a1"),
        encode(Inst::Subw {
            rd: 10,
            rs1: 0,
            rs2: 11
        })
    );
    assert_eq!(
        asm_u32("sext.w a0, a1"),
        encode(Inst::Addiw {
            rd: 10,
            rs1: 11,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("seqz a0, a1"),
        encode(Inst::Sltiu {
            rd: 10,
            rs1: 11,
            imm: 1
        })
    );
    assert_eq!(
        asm_u32("snez a0, a1"),
        encode(Inst::Sltu {
            rd: 10,
            rs1: 0,
            rs2: 11
        })
    );
    assert_eq!(
        asm_u32("sltz a0, a1"),
        encode(Inst::Slt {
            rd: 10,
            rs1: 11,
            rs2: 0
        })
    );
    assert_eq!(
        asm_u32("sgtz a0, a1"),
        encode(Inst::Slt {
            rd: 10,
            rs1: 0,
            rs2: 11
        })
    );
    assert_eq!(
        asm_u32("ret"),
        encode(Inst::Jalr {
            rd: 0,
            rs1: 1,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("jr s3"),
        encode(Inst::Jalr {
            rd: 0,
            rs1: 19,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("jalr t0"),
        encode(Inst::Jalr {
            rd: 1,
            rs1: 5,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("jalr a0, -12(sp)"),
        encode(Inst::Jalr {
            rd: 10,
            rs1: 2,
            imm: -12
        })
    );
    assert_eq!(
        asm_u32("jalr a0, (sp)"),
        encode(Inst::Jalr {
            rd: 10,
            rs1: 2,
            imm: 0
        })
    );
    assert_eq!(
        asm_u32("jalr a0, sp, 16"),
        encode(Inst::Jalr {
            rd: 10,
            rs1: 2,
            imm: 16
        })
    );

    // Branches and branch pseudos with plain numeric offsets.
    assert_eq!(
        asm_u32("beq a0, a1, -8"),
        encode(Inst::Beq {
            rs1: 10,
            rs2: 11,
            imm: -8
        })
    );
    assert_eq!(
        asm_u32("beqz a0, 12"),
        encode(Inst::Beq {
            rs1: 10,
            rs2: 0,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bnez a0, 12"),
        encode(Inst::Bne {
            rs1: 10,
            rs2: 0,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("blez a0, 12"),
        encode(Inst::Bge {
            rs1: 0,
            rs2: 10,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bgez a0, 12"),
        encode(Inst::Bge {
            rs1: 10,
            rs2: 0,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bltz a0, 12"),
        encode(Inst::Blt {
            rs1: 10,
            rs2: 0,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bgtz a0, 12"),
        encode(Inst::Blt {
            rs1: 0,
            rs2: 10,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bgt a0, a1, 12"),
        encode(Inst::Blt {
            rs1: 11,
            rs2: 10,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("ble a0, a1, 12"),
        encode(Inst::Bge {
            rs1: 11,
            rs2: 10,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bgtu a0, a1, 12"),
        encode(Inst::Bltu {
            rs1: 11,
            rs2: 10,
            imm: 12
        })
    );
    assert_eq!(
        asm_u32("bleu a0, a1, 12"),
        encode(Inst::Bgeu {
            rs1: 11,
            rs2: 10,
            imm: 12
        })
    );
    assert_eq!(asm_u32("j 16"), encode(Inst::Jal { rd: 0, imm: 16 }));
    assert_eq!(asm_u32("jal 16"), encode(Inst::Jal { rd: 1, imm: 16 }));
    assert_eq!(asm_u32("jal t0, 16"), encode(Inst::Jal { rd: 5, imm: 16 }));

    // Fence and system
    assert_eq!(asm_u32("fence"), 0x0ff0_000f);
    assert_eq!(asm_u32("fence iorw, iorw"), 0x0ff0_000f);
    assert_eq!(asm_u32("fence rw, w"), (0x3 << 24) | (0x1 << 20) | 0x0f);
    assert_eq!(asm_u32("fence i, o"), (0x8 << 24) | (0x4 << 20) | 0x0f);
    assert_eq!(asm_u32("fence.i"), encode(Inst::FenceI));
    assert_eq!(asm_u32("ecall"), encode(Inst::Ecall));
    assert_eq!(asm_u32("ebreak"), encode(Inst::Ebreak));
    assert_eq!(asm_u32("mret"), encode(Inst::Mret));
    assert_eq!(asm_u32("sret"), encode(Inst::Sret));
    assert_eq!(asm_u32("wfi"), encode(Inst::Wfi));
    assert_eq!(
        asm_u32("sfence.vma"),
        encode(Inst::SfenceVma { rs1: 0, rs2: 0 })
    );
    assert_eq!(
        asm_u32("sfence.vma a0"),
        encode(Inst::SfenceVma { rs1: 10, rs2: 0 })
    );
    assert_eq!(
        asm_u32("sfence.vma a0, a1"),
        encode(Inst::SfenceVma { rs1: 10, rs2: 11 })
    );

    // CSR names and pseudos
    let csrs = [
        ("cycle", 0xC00),
        ("time", 0xC01),
        ("instret", 0xC02),
        ("sstatus", 0x100),
        ("sie", 0x104),
        ("stvec", 0x105),
        ("scounteren", 0x106),
        ("sscratch", 0x140),
        ("sepc", 0x141),
        ("scause", 0x142),
        ("stval", 0x143),
        ("sip", 0x144),
        ("satp", 0x180),
        ("mvendorid", 0xF11),
        ("marchid", 0xF12),
        ("mimpid", 0xF13),
        ("mhartid", 0xF14),
        ("mstatus", 0x300),
        ("misa", 0x301),
        ("medeleg", 0x302),
        ("mideleg", 0x303),
        ("mie", 0x304),
        ("mtvec", 0x305),
        ("mcounteren", 0x306),
        ("mscratch", 0x340),
        ("mepc", 0x341),
        ("mcause", 0x342),
        ("mtval", 0x343),
        ("mip", 0x344),
        ("mcycle", 0xB00),
        ("minstret", 0xB02),
    ];
    for (name, addr) in csrs {
        assert_eq!(
            asm_u32(&format!("csrr a0, {name}")),
            encode(Inst::Csrrs {
                rd: 10,
                rs1: 0,
                csr: addr
            })
        );
    }
    assert_eq!(
        asm_u32("csrw mtvec, a0"),
        encode(Inst::Csrrw {
            rd: 0,
            rs1: 10,
            csr: 0x305
        })
    );
    assert_eq!(
        asm_u32("csrs sstatus, a0"),
        encode(Inst::Csrrs {
            rd: 0,
            rs1: 10,
            csr: 0x100
        })
    );
    assert_eq!(
        asm_u32("csrc mie, a0"),
        encode(Inst::Csrrc {
            rd: 0,
            rs1: 10,
            csr: 0x304
        })
    );
    assert_eq!(
        asm_u32("csrwi satp, 5"),
        encode(Inst::Csrrwi {
            rd: 0,
            uimm: 5,
            csr: 0x180
        })
    );
    assert_eq!(
        asm_u32("csrsi mstatus, 8"),
        encode(Inst::Csrrsi {
            rd: 0,
            uimm: 8,
            csr: 0x300
        })
    );
    assert_eq!(
        asm_u32("csrci sip, 2"),
        encode(Inst::Csrrci {
            rd: 0,
            uimm: 2,
            csr: 0x144
        })
    );
    assert_eq!(
        asm_u32("rdcycle a0"),
        encode(Inst::Csrrs {
            rd: 10,
            rs1: 0,
            csr: 0xC00
        })
    );
    assert_eq!(
        asm_u32("rdtime a1"),
        encode(Inst::Csrrs {
            rd: 11,
            rs1: 0,
            csr: 0xC01
        })
    );
    assert_eq!(
        asm_u32("rdinstret a2"),
        encode(Inst::Csrrs {
            rd: 12,
            rs1: 0,
            csr: 0xC02
        })
    );

    // AMOs with 0(rs1)
    assert_eq!(
        asm_u32("amoadd.w.aqrl a0, a1, 0(a2)"),
        encode(Inst::AmoW {
            op: AmoOp::Add,
            rd: 10,
            rs1: 12,
            rs2: 11,
            aq: true,
            rl: true
        })
    );

    // call / tail / lla / la
    let out = assemble(
        "call target\ntail target\nlla a0, target\nla a1, target\ntarget: nop",
        0x1000,
    )
    .unwrap();
    assert_eq!(out.bytes.len(), 36);
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[0..4].try_into().unwrap())).unwrap(),
        Inst::Auipc { rd: 1, imm: 0 }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[4..8].try_into().unwrap())).unwrap(),
        Inst::Jalr {
            rd: 1,
            rs1: 1,
            imm: 32
        }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[8..12].try_into().unwrap())).unwrap(),
        Inst::Auipc { rd: 6, imm: 0 }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[12..16].try_into().unwrap())).unwrap(),
        Inst::Jalr {
            rd: 0,
            rs1: 6,
            imm: 24
        }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[16..20].try_into().unwrap())).unwrap(),
        Inst::Auipc { rd: 10, imm: 0 }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[20..24].try_into().unwrap())).unwrap(),
        Inst::Addi {
            rd: 10,
            rs1: 10,
            imm: 16
        }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[24..28].try_into().unwrap())).unwrap(),
        Inst::Auipc { rd: 11, imm: 0 }
    );
    assert_eq!(
        decode(u32::from_le_bytes(out.bytes[28..32].try_into().unwrap())).unwrap(),
        Inst::Addi {
            rd: 11,
            rs1: 11,
            imm: 8
        }
    );
}

#[test]
fn expressions_and_equ_redefinition() {
    let src = "
        .equ A, 6 & 3 - 1
        .equ B, 1 + 2 << 3
        .set C, 5 | 2 ^ 3
        .equ D, -7 / 2
        .equ E, -8 >> 60
        .equ F, 'Z' - 'A'
        .equ G, 0b1010 + 0x10
        .dword A, B, C, D, E, F, G
        .equ X, 10
        addi a0, zero, X
        .equ X, -20
        addi a1, zero, X
    L1: nop
        nop
    L2: .word L2 - L1
    ";
    let out = assemble(src, 0x8000_0000).unwrap();
    let vals: Vec<i64> = out.bytes[..56]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&c| i64::from_le_bytes(c))
        .collect();
    assert_eq!(vals, vec![1, 17, 4, -3, 15, 25, 26]);
    let w1 = u32::from_le_bytes(out.bytes[56..60].try_into().unwrap());
    let w2 = u32::from_le_bytes(out.bytes[60..64].try_into().unwrap());
    let diff = i32::from_le_bytes(out.bytes[72..76].try_into().unwrap());
    assert_eq!(
        w1,
        encode(Inst::Addi {
            rd: 10,
            rs1: 0,
            imm: 10
        })
    );
    assert_eq!(
        w2,
        encode(Inst::Addi {
            rd: 11,
            rs1: 0,
            imm: -20
        })
    );
    assert_eq!(diff, 8);
}

#[test]
fn labels_pcrel_and_numeric_zero_b() {
    let src = "
    0:
        addi a0, zero, 0b101
        j 0b
        j 1f
        a: b: nop
    1:  .Lpc: auipc t0, %pcrel_hi(target)
        addi t0, t0, %pcrel_lo(.Lpc)
        addi t1, zero, %pcrel_lo(1f)
    1:  auipc t2, %pcrel_hi(0b)
    target: nop
    ";
    let out = assemble(src, 0x1000).unwrap();
    assert_eq!(
        out.symbols,
        vec![
            ("a".into(), 0x100c),
            ("b".into(), 0x100c),
            (".Lpc".into(), 0x1010),
            ("target".into(), 0x1020),
        ]
    );
    // addi a0, zero, 5
    assert_eq!(
        u32::from_le_bytes(out.bytes[0..4].try_into().unwrap()),
        encode(Inst::Addi {
            rd: 10,
            rs1: 0,
            imm: 5
        })
    );
    // j 0b -> -4
    assert_eq!(
        u32::from_le_bytes(out.bytes[4..8].try_into().unwrap()),
        encode(Inst::Jal { rd: 0, imm: -4 })
    );
    // j 1f -> 8
    assert_eq!(
        u32::from_le_bytes(out.bytes[8..12].try_into().unwrap()),
        encode(Inst::Jal { rd: 0, imm: 8 })
    );
    // %pcrel_lo(1f) resolves to (0b - 0x101c) = 0x1000 - 0x101c = -28
    assert_eq!(
        u32::from_le_bytes(out.bytes[24..28].try_into().unwrap()),
        encode(Inst::Addi {
            rd: 6,
            rs1: 0,
            imm: -28
        })
    );
}

#[test]
fn alignment_from_every_offset_mod_8_and_data() {
    let expected_pads: [&[u8]; 8] = [
        &[],
        &[0x00, 0x01, 0x00, 0x13, 0x00, 0x00, 0x00],
        &[0x01, 0x00, 0x13, 0x00, 0x00, 0x00],
        &[0x00, 0x13, 0x00, 0x00, 0x00],
        &[0x13, 0x00, 0x00, 0x00],
        &[0x00, 0x01, 0x00],
        &[0x01, 0x00],
        &[0x00],
    ];
    for (rem, exp) in expected_pads.iter().enumerate() {
        let mut src = String::new();
        if rem > 0 {
            src.push_str(&format!(".zero {rem}\n"));
        }
        src.push_str(".balign 8\n");
        let out = assemble(&src, 0x8000_0000).unwrap();
        assert_eq!(&out.bytes[rem..], *exp, "balign 8 from offset {rem}");

        let src_p2 = if rem > 0 {
            format!(".zero {rem}\n.p2align 3\n")
        } else {
            ".p2align 3\n".to_string()
        };
        assert_eq!(assemble(&src_p2, 0x8000_0000).unwrap().bytes, out.bytes);

        let src_al = if rem > 0 {
            format!(".zero {rem}\n.align 3\n")
        } else {
            ".align 3\n".to_string()
        };
        assert_eq!(assemble(&src_al, 0x8000_0000).unwrap().bytes, out.bytes);
    }

    let data_src = r#"
        .text
        .globl main
        .option rvc
        .byte -128, 255
        .half -32768, 65535
        .2byte 1
        .word -2147483648, 4294967295
        .4byte 2
        .long 3
        .dword -1
        .8byte 4
        .quad 5
        .space 2
        .ascii "a#b\n\t\r\0\\\""
        .asciz "xy"
        .string "z"
    "#;
    let out = assemble(data_src, 0).unwrap();
    let mut exp = Vec::new();
    exp.extend_from_slice(&[0x80, 0xff]);
    exp.extend_from_slice(&(-32768i16).to_le_bytes());
    exp.extend_from_slice(&65535u16.to_le_bytes());
    exp.extend_from_slice(&1u16.to_le_bytes());
    exp.extend_from_slice(&i32::MIN.to_le_bytes());
    exp.extend_from_slice(&u32::MAX.to_le_bytes());
    exp.extend_from_slice(&2u32.to_le_bytes());
    exp.extend_from_slice(&3u32.to_le_bytes());
    exp.extend_from_slice(&(-1i64).to_le_bytes());
    exp.extend_from_slice(&4u64.to_le_bytes());
    exp.extend_from_slice(&5u64.to_le_bytes());
    exp.extend_from_slice(&[0, 0]);
    exp.extend_from_slice(b"a#b\n\t\r\0\\\"xy\0z\0");
    assert_eq!(out.bytes, exp);
}

#[test]
fn macros_with_args_and_numeric_labels() {
    let src = "
    .macro PUSH reg, off
        addi sp, sp, -\\off
        sd \\reg, 0(sp)
    1:  beqz \\reg, 1b
    .endm
    start: PUSH ra, 16
        PUSH s0, 8
    ";
    let out = assemble(src, 0x2000).unwrap();
    assert_eq!(out.bytes.len(), 24);
    assert_eq!(out.symbols, vec![("start".into(), 0x2000)]);
    assert_eq!(
        u32::from_le_bytes(out.bytes[0..4].try_into().unwrap()),
        encode(Inst::Addi {
            rd: 2,
            rs1: 2,
            imm: -16
        })
    );
    assert_eq!(
        u32::from_le_bytes(out.bytes[8..12].try_into().unwrap()),
        encode(Inst::Beq {
            rs1: 1,
            rs2: 0,
            imm: 0
        })
    );
    assert_eq!(
        u32::from_le_bytes(out.bytes[20..24].try_into().unwrap()),
        encode(Inst::Beq {
            rs1: 8,
            rs2: 0,
            imm: 0
        })
    );
}

#[test]
fn c_instructions_all_33_smoke() {
    assert_eq!(asm_u16("c.nop"), 0x0001);
    assert_eq!(asm_u16("c.ebreak"), 0x9002);
    assert_eq!(asm_u16("c.addi4spn a0, sp, 4"), 0x0048);
    assert_eq!(asm_u16("c.li a0, 0"), 0x4501);
    assert_eq!(asm_u16("c.addiw a0, 0"), 0x2501);
    assert_eq!(asm_u16("c.andi a0, -1"), 0x997d);
    assert_eq!(asm_u16("c.slli a0, 63"), 0x157e);
    assert_eq!(asm_u16("c.srli a0, 1"), 0x8105);
    assert_eq!(asm_u16("c.srai a0, 1"), 0x8505);
    assert_eq!(asm_u16("c.beqz a0, 0"), 0xc101);
    assert_eq!(asm_u16("c.bnez a0, 0"), 0xe101);
    assert_eq!(asm_u16("c.lw a0, (a1)"), 0x4188);
    assert_eq!(asm_u16("c.ld a0, (a1)"), 0x6188);
    assert_eq!(asm_u16("c.sw a0, (a1)"), 0xc188);
    assert_eq!(asm_u16("c.sd a0, (a1)"), 0xe188);
    assert_eq!(asm_u16("c.lwsp a0, (sp)"), 0x4502);
    assert_eq!(asm_u16("c.ldsp a0, (sp)"), 0x6502);
    assert_eq!(asm_u16("c.swsp zero, (sp)"), 0xc002);
    assert_eq!(asm_u16("c.sdsp zero, (sp)"), 0xe002);
}
