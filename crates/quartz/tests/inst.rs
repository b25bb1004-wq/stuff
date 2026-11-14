#![allow(clippy::unwrap_used)]
//! Decoder and encoder: round trips, panic freedom and known encodings.
use assay::{forall, Rng};
use quartz::inst::{decode, encode, Inst};

/// Number of instruction kinds that `make` produces.
const KINDS: usize = 65;

/// A random valid instruction of kind `kind` (0..KINDS).
fn make(kind: usize, r: &mut Rng) -> Inst {
    let rd = r.below(32) as u8;
    let rs1 = r.below(32) as u8;
    let rs2 = r.below(32) as u8;
    let imm12 = r.range(0, 4096) as i64 - 2048;
    let imm13 = (r.range(0, 4096) as i64 - 2048) * 2;
    let imm21 = (r.range(0, 1 << 20) as i64 - (1 << 19)) * 2;
    let upper = i64::from((r.u64() as u32 & 0xffff_f000) as i32);
    let sh6 = r.range(0, 64) as i64;
    let sh5 = r.range(0, 32) as i64;
    match kind {
        0 => Inst::Lui { rd, imm: upper },
        1 => Inst::Auipc { rd, imm: upper },
        2 => Inst::Jal { rd, imm: imm21 },
        3 => Inst::Jalr {
            rd,
            rs1,
            imm: imm12,
        },
        4 => Inst::Beq {
            rs1,
            rs2,
            imm: imm13,
        },
        5 => Inst::Bne {
            rs1,
            rs2,
            imm: imm13,
        },
        6 => Inst::Blt {
            rs1,
            rs2,
            imm: imm13,
        },
        7 => Inst::Bge {
            rs1,
            rs2,
            imm: imm13,
        },
        8 => Inst::Bltu {
            rs1,
            rs2,
            imm: imm13,
        },
        9 => Inst::Bgeu {
            rs1,
            rs2,
            imm: imm13,
        },
        10 => Inst::Lb {
            rd,
            rs1,
            imm: imm12,
        },
        11 => Inst::Lh {
            rd,
            rs1,
            imm: imm12,
        },
        12 => Inst::Lw {
            rd,
            rs1,
            imm: imm12,
        },
        13 => Inst::Ld {
            rd,
            rs1,
            imm: imm12,
        },
        14 => Inst::Lbu {
            rd,
            rs1,
            imm: imm12,
        },
        15 => Inst::Lhu {
            rd,
            rs1,
            imm: imm12,
        },
        16 => Inst::Lwu {
            rd,
            rs1,
            imm: imm12,
        },
        17 => Inst::Sb {
            rs1,
            rs2,
            imm: imm12,
        },
        18 => Inst::Sh {
            rs1,
            rs2,
            imm: imm12,
        },
        19 => Inst::Sw {
            rs1,
            rs2,
            imm: imm12,
        },
        20 => Inst::Sd {
            rs1,
            rs2,
            imm: imm12,
        },
        21 => Inst::Addi {
            rd,
            rs1,
            imm: imm12,
        },
        22 => Inst::Slti {
            rd,
            rs1,
            imm: imm12,
        },
        23 => Inst::Sltiu {
            rd,
            rs1,
            imm: imm12,
        },
        24 => Inst::Xori {
            rd,
            rs1,
            imm: imm12,
        },
        25 => Inst::Ori {
            rd,
            rs1,
            imm: imm12,
        },
        26 => Inst::Andi {
            rd,
            rs1,
            imm: imm12,
        },
        27 => Inst::Slli { rd, rs1, imm: sh6 },
        28 => Inst::Srli { rd, rs1, imm: sh6 },
        29 => Inst::Srai { rd, rs1, imm: sh6 },
        30 => Inst::Add { rd, rs1, rs2 },
        31 => Inst::Sub { rd, rs1, rs2 },
        32 => Inst::Sll { rd, rs1, rs2 },
        33 => Inst::Slt { rd, rs1, rs2 },
        34 => Inst::Sltu { rd, rs1, rs2 },
        35 => Inst::Xor { rd, rs1, rs2 },
        36 => Inst::Srl { rd, rs1, rs2 },
        37 => Inst::Sra { rd, rs1, rs2 },
        38 => Inst::Or { rd, rs1, rs2 },
        39 => Inst::And { rd, rs1, rs2 },
        40 => Inst::Mul { rd, rs1, rs2 },
        41 => Inst::Mulh { rd, rs1, rs2 },
        42 => Inst::Mulhsu { rd, rs1, rs2 },
        43 => Inst::Mulhu { rd, rs1, rs2 },
        44 => Inst::Div { rd, rs1, rs2 },
        45 => Inst::Divu { rd, rs1, rs2 },
        46 => Inst::Rem { rd, rs1, rs2 },
        47 => Inst::Remu { rd, rs1, rs2 },
        48 => Inst::Addiw {
            rd,
            rs1,
            imm: imm12,
        },
        49 => Inst::Slliw { rd, rs1, imm: sh5 },
        50 => Inst::Srliw { rd, rs1, imm: sh5 },
        51 => Inst::Sraiw { rd, rs1, imm: sh5 },
        52 => Inst::Addw { rd, rs1, rs2 },
        53 => Inst::Subw { rd, rs1, rs2 },
        54 => Inst::Sllw { rd, rs1, rs2 },
        55 => Inst::Srlw { rd, rs1, rs2 },
        56 => Inst::Sraw { rd, rs1, rs2 },
        57 => Inst::Mulw { rd, rs1, rs2 },
        58 => Inst::Divw { rd, rs1, rs2 },
        59 => Inst::Divuw { rd, rs1, rs2 },
        60 => Inst::Remw { rd, rs1, rs2 },
        61 => Inst::Remuw { rd, rs1, rs2 },
        62 => Inst::Fence,
        63 => Inst::Ecall,
        _ => Inst::Ebreak,
    }
}

#[test]
fn decode_inverts_encode_for_every_kind() {
    forall(2000, 0xD1C0_0001, |r| {
        for k in 0..KINDS {
            let i = make(k, r);
            assert_eq!(decode(encode(i)), Some(i), "kind {k}: {i:?}");
        }
    });
}

#[test]
fn decode_never_panics() {
    forall(50_000, 0xD1C0_0002, |r| {
        let _ = decode(r.u64() as u32);
    });
}

#[test]
fn all_zero_and_all_one_words_are_illegal() {
    assert_eq!(decode(0), None);
    assert_eq!(decode(u32::MAX), None);
}

#[test]
fn known_encodings_match_the_spec() {
    let cases = [
        (
            0x0050_0093,
            Inst::Addi {
                rd: 1,
                rs1: 0,
                imm: 5,
            },
        ),
        (
            0x0020_81b3,
            Inst::Add {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
        ),
        (
            0x4020_81b3,
            Inst::Sub {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
        ),
        (
            0x0220_81b3,
            Inst::Mul {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
        ),
        (
            0x1234_52b7,
            Inst::Lui {
                rd: 5,
                imm: 0x1234_5000,
            },
        ),
        (0x0000_0073, Inst::Ecall),
        (0x0010_0073, Inst::Ebreak),
        (0x0ff0_000f, Inst::Fence),
    ];
    for (word, inst) in cases {
        assert_eq!(decode(word), Some(inst), "decode {word:#010x}");
        assert_eq!(encode(inst), word, "encode {inst:?}");
    }
}

#[test]
fn shift_immediate_rejects_bad_funct6() {
    // slli x1, x1, 1 is legal; setting bit 30 makes funct6 0x10, which is not a valid SLLI.
    assert!(decode(0x0010_9093).is_some());
    assert_eq!(decode(0x4010_9093), None);
}
