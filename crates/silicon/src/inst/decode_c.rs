//! Compressed (RV64C) halfword decoder: expands 16-bit instructions to their 32-bit `Inst`.
use super::Inst;

/// Sign-extend the low `bits` bits of `v`.
fn sext(v: u32, bits: u32) -> i64 {
    let s = 32 - bits;
    i64::from(((v << s) as i32) >> s)
}

/// 6-bit signed immediate from CI format (`h[12]` and `h[6:2]`).
fn ci_imm(w: u32) -> i64 {
    sext((((w >> 12) & 1) << 5) | ((w >> 2) & 0x1f), 6)
}

/// 6-bit unsigned shift amount from CI/CB format (`h[12]` and `h[6:2]`).
fn shamt6(w: u32) -> i64 {
    i64::from((((w >> 12) & 1) << 5) | ((w >> 2) & 0x1f))
}

/// Expand a 16-bit compressed instruction to the `Inst` of its 32-bit equivalent
/// (c.addi rd, i -> Addi { rd, rs1: rd, imm: i }, c.mv rd, rs2 -> Add { rd, rs1: 0, rs2 }, ...).
/// None for reserved encodings, FP forms and halfwords whose low two bits are 11.
pub fn decode_c(h: u16) -> Option<Inst> {
    if h == 0 || (h & 3) == 3 {
        return None;
    }
    let w = u32::from(h);
    let f3 = (w >> 13) & 7;
    let rd_p = (((w >> 2) & 7) + 8) as u8;
    let rs1_p = (((w >> 7) & 7) + 8) as u8;
    let rd = ((w >> 7) & 0x1f) as u8;
    let rs2 = ((w >> 2) & 0x1f) as u8;
    match w & 3 {
        0 => q0(w, f3, rd_p, rs1_p),
        1 => q1(w, f3, rd, rd_p, rs1_p),
        _ => q2(w, f3, rd, rs2),
    }
}

/// Quadrant 00: stack-relative addi, loads and stores.
fn q0(w: u32, f3: u32, rd_p: u8, rs1_p: u8) -> Option<Inst> {
    let imm_w = i64::from((((w >> 10) & 7) << 3) | (((w >> 6) & 1) << 2) | (((w >> 5) & 1) << 6));
    let imm_d = i64::from((((w >> 10) & 7) << 3) | (((w >> 5) & 3) << 6));
    match f3 {
        0 => {
            let nzuimm = (((w >> 11) & 3) << 4)
                | (((w >> 7) & 0xf) << 6)
                | (((w >> 6) & 1) << 2)
                | (((w >> 5) & 1) << 3);
            (nzuimm != 0).then_some(Inst::Addi {
                rd: rd_p,
                rs1: 2,
                imm: i64::from(nzuimm),
            })
        }
        2 => Some(Inst::Lw {
            rd: rd_p,
            rs1: rs1_p,
            imm: imm_w,
        }),
        3 => Some(Inst::Ld {
            rd: rd_p,
            rs1: rs1_p,
            imm: imm_d,
        }),
        6 => Some(Inst::Sw {
            rs1: rs1_p,
            rs2: rd_p,
            imm: imm_w,
        }),
        7 => Some(Inst::Sd {
            rs1: rs1_p,
            rs2: rd_p,
            imm: imm_d,
        }),
        _ => None,
    }
}

/// Quadrant 01: immediates, arithmetic, jumps and conditional branches.
fn q1(w: u32, f3: u32, rd: u8, rd_p: u8, rs1_p: u8) -> Option<Inst> {
    match f3 {
        0 => Some(Inst::Addi {
            rd,
            rs1: rd,
            imm: ci_imm(w),
        }),
        1 => (rd != 0).then_some(Inst::Addiw {
            rd,
            rs1: rd,
            imm: ci_imm(w),
        }),
        2 => Some(Inst::Addi {
            rd,
            rs1: 0,
            imm: ci_imm(w),
        }),
        3 => q1_lui_or_addi16sp(w, rd),
        4 => q1_alu(w, rd_p, rs1_p),
        5 => {
            let raw = (((w >> 12) & 1) << 11)
                | (((w >> 8) & 1) << 10)
                | (((w >> 9) & 3) << 8)
                | (((w >> 6) & 1) << 7)
                | (((w >> 7) & 1) << 6)
                | (((w >> 2) & 1) << 5)
                | (((w >> 11) & 1) << 4)
                | (((w >> 3) & 7) << 1);
            Some(Inst::Jal {
                rd: 0,
                imm: sext(raw, 12),
            })
        }
        6 | 7 => {
            let raw = (((w >> 12) & 1) << 8)
                | (((w >> 5) & 3) << 6)
                | (((w >> 2) & 1) << 5)
                | (((w >> 10) & 3) << 3)
                | (((w >> 3) & 3) << 1);
            let imm = sext(raw, 9);
            if f3 == 6 {
                Some(Inst::Beq {
                    rs1: rs1_p,
                    rs2: 0,
                    imm,
                })
            } else {
                Some(Inst::Bne {
                    rs1: rs1_p,
                    rs2: 0,
                    imm,
                })
            }
        }
        _ => None,
    }
}

fn q1_lui_or_addi16sp(w: u32, rd: u8) -> Option<Inst> {
    if rd == 2 {
        let raw = (((w >> 12) & 1) << 9)
            | (((w >> 3) & 3) << 7)
            | (((w >> 5) & 1) << 6)
            | (((w >> 2) & 1) << 5)
            | (((w >> 6) & 1) << 4);
        (raw != 0).then_some(Inst::Addi {
            rd: 2,
            rs1: 2,
            imm: sext(raw, 10),
        })
    } else {
        let raw = (((w >> 12) & 1) << 5) | ((w >> 2) & 0x1f);
        (raw != 0).then_some(Inst::Lui {
            rd,
            imm: sext(raw, 6) << 12,
        })
    }
}

fn q1_alu(w: u32, rs2_p: u8, rd_p: u8) -> Option<Inst> {
    match (w >> 10) & 3 {
        0 => Some(Inst::Srli {
            rd: rd_p,
            rs1: rd_p,
            imm: shamt6(w),
        }),
        1 => Some(Inst::Srai {
            rd: rd_p,
            rs1: rd_p,
            imm: shamt6(w),
        }),
        2 => Some(Inst::Andi {
            rd: rd_p,
            rs1: rd_p,
            imm: ci_imm(w),
        }),
        _ => match (((w >> 12) & 1), ((w >> 5) & 3)) {
            (0, 0) => Some(Inst::Sub {
                rd: rd_p,
                rs1: rd_p,
                rs2: rs2_p,
            }),
            (0, 1) => Some(Inst::Xor {
                rd: rd_p,
                rs1: rd_p,
                rs2: rs2_p,
            }),
            (0, 2) => Some(Inst::Or {
                rd: rd_p,
                rs1: rd_p,
                rs2: rs2_p,
            }),
            (0, 3) => Some(Inst::And {
                rd: rd_p,
                rs1: rd_p,
                rs2: rs2_p,
            }),
            (1, 0) => Some(Inst::Subw {
                rd: rd_p,
                rs1: rd_p,
                rs2: rs2_p,
            }),
            (1, 1) => Some(Inst::Addw {
                rd: rd_p,
                rs1: rd_p,
                rs2: rs2_p,
            }),
            _ => None,
        },
    }
}

/// Quadrant 10: shifts, stack loads/stores, jumps and register moves/adds.
fn q2(w: u32, f3: u32, rd: u8, rs2: u8) -> Option<Inst> {
    match f3 {
        0 => Some(Inst::Slli {
            rd,
            rs1: rd,
            imm: shamt6(w),
        }),
        2 => (rd != 0).then(|| {
            let imm =
                i64::from((((w >> 2) & 3) << 6) | (((w >> 12) & 1) << 5) | (((w >> 4) & 7) << 2));
            Inst::Lw { rd, rs1: 2, imm }
        }),
        3 => (rd != 0).then(|| {
            let imm =
                i64::from((((w >> 2) & 7) << 6) | (((w >> 12) & 1) << 5) | (((w >> 5) & 3) << 3));
            Inst::Ld { rd, rs1: 2, imm }
        }),
        4 => match ((w >> 12) & 1, rs2) {
            (0, 0) => (rd != 0).then_some(Inst::Jalr {
                rd: 0,
                rs1: rd,
                imm: 0,
            }),
            (0, _) => Some(Inst::Add { rd, rs1: 0, rs2 }),
            (1, 0) if rd == 0 => Some(Inst::Ebreak),
            (1, 0) => Some(Inst::Jalr {
                rd: 1,
                rs1: rd,
                imm: 0,
            }),
            (1, _) => Some(Inst::Add { rd, rs1: rd, rs2 }),
            _ => None,
        },
        6 => {
            let imm = i64::from((((w >> 7) & 3) << 6) | (((w >> 9) & 0xf) << 2));
            Some(Inst::Sw { rs1: 2, rs2, imm })
        }
        7 => {
            let imm = i64::from((((w >> 7) & 7) << 6) | (((w >> 10) & 7) << 3));
            Some(Inst::Sd { rs1: 2, rs2, imm })
        }
        _ => None,
    }
}
