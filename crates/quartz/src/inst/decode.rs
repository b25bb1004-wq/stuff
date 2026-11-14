//! Word to instruction. Anything outside the supported set decodes to `None`.
use super::{AmoOp, Inst};

/// Sign-extend the low `bits` bits of `v`.
fn sext(v: u32, bits: u32) -> i64 {
    let s = 32 - bits;
    i64::from(((v << s) as i32) >> s)
}

fn i_imm(w: u32) -> i64 {
    i64::from((w as i32) >> 20)
}

fn s_imm(w: u32) -> i64 {
    sext(((w >> 25) << 5) | ((w >> 7) & 0x1f), 12)
}

fn b_imm(w: u32) -> i64 {
    let v = (((w >> 31) & 1) << 12)
        | (((w >> 7) & 1) << 11)
        | (((w >> 25) & 0x3f) << 5)
        | (((w >> 8) & 0xf) << 1);
    sext(v, 13)
}

fn u_imm(w: u32) -> i64 {
    i64::from((w & 0xffff_f000) as i32)
}

fn j_imm(w: u32) -> i64 {
    let v = (((w >> 31) & 1) << 20)
        | (((w >> 12) & 0xff) << 12)
        | (((w >> 20) & 1) << 11)
        | (((w >> 21) & 0x3ff) << 1);
    sext(v, 21)
}

/// Decode one 32-bit instruction word, or `None` if it is not in the supported set.
pub fn decode(w: u32) -> Option<Inst> {
    let rd = ((w >> 7) & 0x1f) as u8;
    let rs1 = ((w >> 15) & 0x1f) as u8;
    let rs2 = ((w >> 20) & 0x1f) as u8;
    let f3 = (w >> 12) & 7;
    let f7 = w >> 25;
    match w & 0x7f {
        0x37 => Some(Inst::Lui { rd, imm: u_imm(w) }),
        0x17 => Some(Inst::Auipc { rd, imm: u_imm(w) }),
        0x6f => Some(Inst::Jal { rd, imm: j_imm(w) }),
        0x67 if f3 == 0 => Some(Inst::Jalr {
            rd,
            rs1,
            imm: i_imm(w),
        }),
        0x63 => {
            let imm = b_imm(w);
            Some(match f3 {
                0 => Inst::Beq { rs1, rs2, imm },
                1 => Inst::Bne { rs1, rs2, imm },
                4 => Inst::Blt { rs1, rs2, imm },
                5 => Inst::Bge { rs1, rs2, imm },
                6 => Inst::Bltu { rs1, rs2, imm },
                7 => Inst::Bgeu { rs1, rs2, imm },
                _ => return None,
            })
        }
        0x03 => {
            let imm = i_imm(w);
            Some(match f3 {
                0 => Inst::Lb { rd, rs1, imm },
                1 => Inst::Lh { rd, rs1, imm },
                2 => Inst::Lw { rd, rs1, imm },
                3 => Inst::Ld { rd, rs1, imm },
                4 => Inst::Lbu { rd, rs1, imm },
                5 => Inst::Lhu { rd, rs1, imm },
                6 => Inst::Lwu { rd, rs1, imm },
                _ => return None,
            })
        }
        0x23 => {
            let imm = s_imm(w);
            Some(match f3 {
                0 => Inst::Sb { rs1, rs2, imm },
                1 => Inst::Sh { rs1, rs2, imm },
                2 => Inst::Sw { rs1, rs2, imm },
                3 => Inst::Sd { rs1, rs2, imm },
                _ => return None,
            })
        }
        0x13 => op_imm(w, rd, rs1, f3),
        0x33 => op_reg(rd, rs1, rs2, f3, f7),
        0x1b => op_imm32(w, rd, rs1, f3, f7),
        0x3b => op_reg32(rd, rs1, rs2, f3, f7),
        0x2f => amo(rd, rs1, rs2, f3, f7),
        0x0f => match f3 {
            0 => Some(Inst::Fence),
            1 => Some(Inst::FenceI),
            _ => None,
        },
        0x73 => system(w, rd, rs1, f3),
        _ => None,
    }
}

/// SYSTEM opcode: environment calls, privileged instructions and CSR access.
fn system(w: u32, rd: u8, rs1: u8, f3: u32) -> Option<Inst> {
    let csr = (w >> 20) as u16;
    let uimm = rs1;
    match f3 {
        0 => match w {
            0x0000_0073 => Some(Inst::Ecall),
            0x0010_0073 => Some(Inst::Ebreak),
            0x1020_0073 => Some(Inst::Sret),
            0x3020_0073 => Some(Inst::Mret),
            0x1050_0073 => Some(Inst::Wfi),
            // SFENCE.VMA: funct7 0001001, rd and funct3 zero; rs1 and rs2 are operands.
            _ if w & 0xfe00_7fff == 0x1200_0073 => Some(Inst::SfenceVma {
                rs1,
                rs2: ((w >> 20) & 0x1f) as u8,
            }),
            _ => None,
        },
        1 => Some(Inst::Csrrw { rd, rs1, csr }),
        2 => Some(Inst::Csrrs { rd, rs1, csr }),
        3 => Some(Inst::Csrrc { rd, rs1, csr }),
        5 => Some(Inst::Csrrwi { rd, uimm, csr }),
        6 => Some(Inst::Csrrsi { rd, uimm, csr }),
        7 => Some(Inst::Csrrci { rd, uimm, csr }),
        _ => None,
    }
}

fn op_imm(w: u32, rd: u8, rs1: u8, f3: u32) -> Option<Inst> {
    let imm = i_imm(w);
    let shamt = i64::from((w >> 20) & 0x3f);
    let top6 = w >> 26;
    match f3 {
        0 => Some(Inst::Addi { rd, rs1, imm }),
        1 => (top6 == 0).then_some(Inst::Slli {
            rd,
            rs1,
            imm: shamt,
        }),
        2 => Some(Inst::Slti { rd, rs1, imm }),
        3 => Some(Inst::Sltiu { rd, rs1, imm }),
        4 => Some(Inst::Xori { rd, rs1, imm }),
        5 => match top6 {
            0x00 => Some(Inst::Srli {
                rd,
                rs1,
                imm: shamt,
            }),
            0x10 => Some(Inst::Srai {
                rd,
                rs1,
                imm: shamt,
            }),
            _ => None,
        },
        6 => Some(Inst::Ori { rd, rs1, imm }),
        _ => Some(Inst::Andi { rd, rs1, imm }),
    }
}

fn op_reg(rd: u8, rs1: u8, rs2: u8, f3: u32, f7: u32) -> Option<Inst> {
    Some(match (f7, f3) {
        (0x00, 0) => Inst::Add { rd, rs1, rs2 },
        (0x20, 0) => Inst::Sub { rd, rs1, rs2 },
        (0x00, 1) => Inst::Sll { rd, rs1, rs2 },
        (0x00, 2) => Inst::Slt { rd, rs1, rs2 },
        (0x00, 3) => Inst::Sltu { rd, rs1, rs2 },
        (0x00, 4) => Inst::Xor { rd, rs1, rs2 },
        (0x00, 5) => Inst::Srl { rd, rs1, rs2 },
        (0x20, 5) => Inst::Sra { rd, rs1, rs2 },
        (0x00, 6) => Inst::Or { rd, rs1, rs2 },
        (0x00, 7) => Inst::And { rd, rs1, rs2 },
        (0x01, 0) => Inst::Mul { rd, rs1, rs2 },
        (0x01, 1) => Inst::Mulh { rd, rs1, rs2 },
        (0x01, 2) => Inst::Mulhsu { rd, rs1, rs2 },
        (0x01, 3) => Inst::Mulhu { rd, rs1, rs2 },
        (0x01, 4) => Inst::Div { rd, rs1, rs2 },
        (0x01, 5) => Inst::Divu { rd, rs1, rs2 },
        (0x01, 6) => Inst::Rem { rd, rs1, rs2 },
        (0x01, 7) => Inst::Remu { rd, rs1, rs2 },
        _ => return None,
    })
}

fn op_imm32(w: u32, rd: u8, rs1: u8, f3: u32, f7: u32) -> Option<Inst> {
    let shamt = i64::from((w >> 20) & 0x1f);
    Some(match (f3, f7) {
        (0, _) => Inst::Addiw {
            rd,
            rs1,
            imm: i_imm(w),
        },
        (1, 0x00) => Inst::Slliw {
            rd,
            rs1,
            imm: shamt,
        },
        (5, 0x00) => Inst::Srliw {
            rd,
            rs1,
            imm: shamt,
        },
        (5, 0x20) => Inst::Sraiw {
            rd,
            rs1,
            imm: shamt,
        },
        _ => return None,
    })
}

fn op_reg32(rd: u8, rs1: u8, rs2: u8, f3: u32, f7: u32) -> Option<Inst> {
    Some(match (f7, f3) {
        (0x00, 0) => Inst::Addw { rd, rs1, rs2 },
        (0x20, 0) => Inst::Subw { rd, rs1, rs2 },
        (0x00, 1) => Inst::Sllw { rd, rs1, rs2 },
        (0x00, 5) => Inst::Srlw { rd, rs1, rs2 },
        (0x20, 5) => Inst::Sraw { rd, rs1, rs2 },
        (0x01, 0) => Inst::Mulw { rd, rs1, rs2 },
        (0x01, 4) => Inst::Divw { rd, rs1, rs2 },
        (0x01, 5) => Inst::Divuw { rd, rs1, rs2 },
        (0x01, 6) => Inst::Remw { rd, rs1, rs2 },
        (0x01, 7) => Inst::Remuw { rd, rs1, rs2 },
        _ => return None,
    })
}

/// Atomic memory operations (RV64A): LR, SC and AMO*.W/D.
fn amo(rd: u8, rs1: u8, rs2: u8, f3: u32, f7: u32) -> Option<Inst> {
    if !matches!(f3, 2 | 3) {
        return None;
    }
    let aq = (f7 & 2) != 0;
    let rl = (f7 & 1) != 0;
    let f5 = f7 >> 2;
    let word = f3 == 2;
    let op = match f5 {
        0x02 => {
            if rs2 != 0 {
                return None;
            }
            return Some(if word {
                Inst::LrW { rd, rs1, aq, rl }
            } else {
                Inst::LrD { rd, rs1, aq, rl }
            });
        }
        0x03 => {
            return Some(if word {
                Inst::ScW {
                    rd,
                    rs1,
                    rs2,
                    aq,
                    rl,
                }
            } else {
                Inst::ScD {
                    rd,
                    rs1,
                    rs2,
                    aq,
                    rl,
                }
            });
        }
        0x01 => AmoOp::Swap,
        0x00 => AmoOp::Add,
        0x04 => AmoOp::Xor,
        0x0c => AmoOp::And,
        0x08 => AmoOp::Or,
        0x10 => AmoOp::Min,
        0x14 => AmoOp::Max,
        0x18 => AmoOp::Minu,
        0x1c => AmoOp::Maxu,
        _ => return None,
    };
    Some(if word {
        Inst::AmoW {
            op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        }
    } else {
        Inst::AmoD {
            op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        }
    })
}
