//! Instruction to word: the exact inverse of `decode` for every instruction it produces.
use super::{AmoOp, Inst};

/// Register-register format.
fn r_fmt(op: u32, f3: u32, f7: u32, rd: u8, rs1: u8, rs2: u8) -> u32 {
    (f7 << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (f3 << 12)
        | (u32::from(rd) << 7)
        | op
}

/// Register-immediate format; the low 12 bits of `imm` are used.
fn i_fmt(op: u32, f3: u32, rd: u8, rs1: u8, imm: i64) -> u32 {
    (((imm as u32) & 0xfff) << 20) | (u32::from(rs1) << 15) | (f3 << 12) | (u32::from(rd) << 7) | op
}

/// Store format.
fn s_fmt(op: u32, f3: u32, rs1: u8, rs2: u8, imm: i64) -> u32 {
    let i = imm as u32;
    (((i >> 5) & 0x7f) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (f3 << 12)
        | ((i & 0x1f) << 7)
        | op
}

/// Branch format; `imm` is a byte offset with bit 0 clear.
fn b_fmt(op: u32, f3: u32, rs1: u8, rs2: u8, imm: i64) -> u32 {
    let i = imm as u32;
    (((i >> 12) & 1) << 31)
        | (((i >> 5) & 0x3f) << 25)
        | (u32::from(rs2) << 20)
        | (u32::from(rs1) << 15)
        | (f3 << 12)
        | (((i >> 1) & 0xf) << 8)
        | (((i >> 11) & 1) << 7)
        | op
}

/// Upper-immediate format; the value is already shifted and its low 12 bits are ignored.
fn u_fmt(op: u32, rd: u8, imm: i64) -> u32 {
    ((imm as u32) & 0xffff_f000) | (u32::from(rd) << 7) | op
}

/// Jump format; `imm` is a byte offset with bit 0 clear.
fn j_fmt(op: u32, rd: u8, imm: i64) -> u32 {
    let i = imm as u32;
    (((i >> 20) & 1) << 31)
        | (((i >> 1) & 0x3ff) << 21)
        | (((i >> 11) & 1) << 20)
        | (((i >> 12) & 0xff) << 12)
        | (u32::from(rd) << 7)
        | op
}

/// Encode an instruction as its 32-bit word.
pub fn encode(inst: Inst) -> u32 {
    match inst {
        Inst::Lui { rd, imm } => u_fmt(0x37, rd, imm),
        Inst::Auipc { rd, imm } => u_fmt(0x17, rd, imm),
        Inst::Jal { rd, imm } => j_fmt(0x6f, rd, imm),
        Inst::Jalr { rd, rs1, imm } => i_fmt(0x67, 0, rd, rs1, imm),
        Inst::Beq { rs1, rs2, imm } => b_fmt(0x63, 0, rs1, rs2, imm),
        Inst::Bne { rs1, rs2, imm } => b_fmt(0x63, 1, rs1, rs2, imm),
        Inst::Blt { rs1, rs2, imm } => b_fmt(0x63, 4, rs1, rs2, imm),
        Inst::Bge { rs1, rs2, imm } => b_fmt(0x63, 5, rs1, rs2, imm),
        Inst::Bltu { rs1, rs2, imm } => b_fmt(0x63, 6, rs1, rs2, imm),
        Inst::Bgeu { rs1, rs2, imm } => b_fmt(0x63, 7, rs1, rs2, imm),
        Inst::Lb { rd, rs1, imm } => i_fmt(0x03, 0, rd, rs1, imm),
        Inst::Lh { rd, rs1, imm } => i_fmt(0x03, 1, rd, rs1, imm),
        Inst::Lw { rd, rs1, imm } => i_fmt(0x03, 2, rd, rs1, imm),
        Inst::Ld { rd, rs1, imm } => i_fmt(0x03, 3, rd, rs1, imm),
        Inst::Lbu { rd, rs1, imm } => i_fmt(0x03, 4, rd, rs1, imm),
        Inst::Lhu { rd, rs1, imm } => i_fmt(0x03, 5, rd, rs1, imm),
        Inst::Lwu { rd, rs1, imm } => i_fmt(0x03, 6, rd, rs1, imm),
        Inst::Sb { rs1, rs2, imm } => s_fmt(0x23, 0, rs1, rs2, imm),
        Inst::Sh { rs1, rs2, imm } => s_fmt(0x23, 1, rs1, rs2, imm),
        Inst::Sw { rs1, rs2, imm } => s_fmt(0x23, 2, rs1, rs2, imm),
        Inst::Sd { rs1, rs2, imm } => s_fmt(0x23, 3, rs1, rs2, imm),
        Inst::Addi { rd, rs1, imm } => i_fmt(0x13, 0, rd, rs1, imm),
        Inst::Slti { rd, rs1, imm } => i_fmt(0x13, 2, rd, rs1, imm),
        Inst::Sltiu { rd, rs1, imm } => i_fmt(0x13, 3, rd, rs1, imm),
        Inst::Xori { rd, rs1, imm } => i_fmt(0x13, 4, rd, rs1, imm),
        Inst::Ori { rd, rs1, imm } => i_fmt(0x13, 6, rd, rs1, imm),
        Inst::Andi { rd, rs1, imm } => i_fmt(0x13, 7, rd, rs1, imm),
        Inst::Slli { rd, rs1, imm } => i_fmt(0x13, 1, rd, rs1, imm & 0x3f),
        Inst::Srli { rd, rs1, imm } => i_fmt(0x13, 5, rd, rs1, imm & 0x3f),
        Inst::Srai { rd, rs1, imm } => i_fmt(0x13, 5, rd, rs1, (imm & 0x3f) | 0x400),
        Inst::Add { rd, rs1, rs2 } => r_fmt(0x33, 0, 0x00, rd, rs1, rs2),
        Inst::Sub { rd, rs1, rs2 } => r_fmt(0x33, 0, 0x20, rd, rs1, rs2),
        Inst::Sll { rd, rs1, rs2 } => r_fmt(0x33, 1, 0x00, rd, rs1, rs2),
        Inst::Slt { rd, rs1, rs2 } => r_fmt(0x33, 2, 0x00, rd, rs1, rs2),
        Inst::Sltu { rd, rs1, rs2 } => r_fmt(0x33, 3, 0x00, rd, rs1, rs2),
        Inst::Xor { rd, rs1, rs2 } => r_fmt(0x33, 4, 0x00, rd, rs1, rs2),
        Inst::Srl { rd, rs1, rs2 } => r_fmt(0x33, 5, 0x00, rd, rs1, rs2),
        Inst::Sra { rd, rs1, rs2 } => r_fmt(0x33, 5, 0x20, rd, rs1, rs2),
        Inst::Or { rd, rs1, rs2 } => r_fmt(0x33, 6, 0x00, rd, rs1, rs2),
        Inst::And { rd, rs1, rs2 } => r_fmt(0x33, 7, 0x00, rd, rs1, rs2),
        Inst::Addiw { rd, rs1, imm } => i_fmt(0x1b, 0, rd, rs1, imm),
        Inst::Slliw { rd, rs1, imm } => i_fmt(0x1b, 1, rd, rs1, imm & 0x1f),
        Inst::Srliw { rd, rs1, imm } => i_fmt(0x1b, 5, rd, rs1, imm & 0x1f),
        Inst::Sraiw { rd, rs1, imm } => i_fmt(0x1b, 5, rd, rs1, (imm & 0x1f) | 0x400),
        Inst::Addw { rd, rs1, rs2 } => r_fmt(0x3b, 0, 0x00, rd, rs1, rs2),
        Inst::Subw { rd, rs1, rs2 } => r_fmt(0x3b, 0, 0x20, rd, rs1, rs2),
        Inst::Sllw { rd, rs1, rs2 } => r_fmt(0x3b, 1, 0x00, rd, rs1, rs2),
        Inst::Srlw { rd, rs1, rs2 } => r_fmt(0x3b, 5, 0x00, rd, rs1, rs2),
        Inst::Sraw { rd, rs1, rs2 } => r_fmt(0x3b, 5, 0x20, rd, rs1, rs2),
        Inst::Fence => i_fmt(0x0f, 0, 0, 0, 0x0ff),
        Inst::Ecall => 0x0000_0073,
        Inst::Ebreak => 0x0010_0073,
        Inst::Mul { rd, rs1, rs2 } => r_fmt(0x33, 0, 0x01, rd, rs1, rs2),
        Inst::Mulh { rd, rs1, rs2 } => r_fmt(0x33, 1, 0x01, rd, rs1, rs2),
        Inst::Mulhsu { rd, rs1, rs2 } => r_fmt(0x33, 2, 0x01, rd, rs1, rs2),
        Inst::Mulhu { rd, rs1, rs2 } => r_fmt(0x33, 3, 0x01, rd, rs1, rs2),
        Inst::Div { rd, rs1, rs2 } => r_fmt(0x33, 4, 0x01, rd, rs1, rs2),
        Inst::Divu { rd, rs1, rs2 } => r_fmt(0x33, 5, 0x01, rd, rs1, rs2),
        Inst::Rem { rd, rs1, rs2 } => r_fmt(0x33, 6, 0x01, rd, rs1, rs2),
        Inst::Remu { rd, rs1, rs2 } => r_fmt(0x33, 7, 0x01, rd, rs1, rs2),
        Inst::Mulw { rd, rs1, rs2 } => r_fmt(0x3b, 0, 0x01, rd, rs1, rs2),
        Inst::Divw { rd, rs1, rs2 } => r_fmt(0x3b, 4, 0x01, rd, rs1, rs2),
        Inst::Divuw { rd, rs1, rs2 } => r_fmt(0x3b, 5, 0x01, rd, rs1, rs2),
        Inst::Remw { rd, rs1, rs2 } => r_fmt(0x3b, 6, 0x01, rd, rs1, rs2),
        Inst::Remuw { rd, rs1, rs2 } => r_fmt(0x3b, 7, 0x01, rd, rs1, rs2),
        Inst::FenceI => i_fmt(0x0f, 1, 0, 0, 0),
        Inst::Csrrw { rd, rs1, csr } => i_fmt(0x73, 1, rd, rs1, i64::from(csr)),
        Inst::Csrrs { rd, rs1, csr } => i_fmt(0x73, 2, rd, rs1, i64::from(csr)),
        Inst::Csrrc { rd, rs1, csr } => i_fmt(0x73, 3, rd, rs1, i64::from(csr)),
        Inst::Csrrwi { rd, uimm, csr } => i_fmt(0x73, 5, rd, uimm & 31, i64::from(csr)),
        Inst::Csrrsi { rd, uimm, csr } => i_fmt(0x73, 6, rd, uimm & 31, i64::from(csr)),
        Inst::Csrrci { rd, uimm, csr } => i_fmt(0x73, 7, rd, uimm & 31, i64::from(csr)),
        Inst::Mret => 0x3020_0073,
        Inst::Sret => 0x1020_0073,
        Inst::Wfi => 0x1050_0073,
        Inst::SfenceVma { rs1, rs2 } => r_fmt(0x73, 0, 0x09, 0, rs1, rs2),
        Inst::HfenceVvma { rs1, rs2 } => r_fmt(0x73, 0, 0x11, 0, rs1, rs2),
        Inst::HfenceGvma { rs1, rs2 } => r_fmt(0x73, 0, 0x31, 0, rs1, rs2),
        Inst::Hlv {
            rd,
            rs1,
            width,
            unsigned,
            exec,
        } => {
            let f7 = match width {
                1 => 0x30,
                2 => 0x32,
                4 => 0x34,
                _ => 0x36,
            };
            let rs2 = if exec {
                3
            } else if unsigned && width != 8 {
                1
            } else {
                0
            };
            r_fmt(0x73, 4, f7, rd, rs1, rs2)
        }
        Inst::Hsv { rs1, rs2, width } => {
            let f7 = match width {
                1 => 0x31,
                2 => 0x33,
                4 => 0x35,
                _ => 0x37,
            };
            r_fmt(0x73, 4, f7, 0, rs1, rs2)
        }
        Inst::LrW { rd, rs1, aq, rl } => amo_fmt(2, 0x02, aq, rl, rd, rs1, 0),
        Inst::LrD { rd, rs1, aq, rl } => amo_fmt(3, 0x02, aq, rl, rd, rs1, 0),
        Inst::ScW {
            rd,
            rs1,
            rs2,
            aq,
            rl,
        } => amo_fmt(2, 0x03, aq, rl, rd, rs1, rs2),
        Inst::ScD {
            rd,
            rs1,
            rs2,
            aq,
            rl,
        } => amo_fmt(3, 0x03, aq, rl, rd, rs1, rs2),
        Inst::AmoW {
            op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        } => amo_fmt(2, amo_funct5(op), aq, rl, rd, rs1, rs2),
        Inst::AmoD {
            op,
            rd,
            rs1,
            rs2,
            aq,
            rl,
        } => amo_fmt(3, amo_funct5(op), aq, rl, rd, rs1, rs2),
    }
}

fn amo_funct5(op: AmoOp) -> u32 {
    match op {
        AmoOp::Swap => 0x01,
        AmoOp::Add => 0x00,
        AmoOp::Xor => 0x04,
        AmoOp::And => 0x0c,
        AmoOp::Or => 0x08,
        AmoOp::Min => 0x10,
        AmoOp::Max => 0x14,
        AmoOp::Minu => 0x18,
        AmoOp::Maxu => 0x1c,
    }
}

fn amo_fmt(f3: u32, f5: u32, aq: bool, rl: bool, rd: u8, rs1: u8, rs2: u8) -> u32 {
    let f7 = (f5 << 2) | (u32::from(aq) << 1) | u32::from(rl);
    r_fmt(0x2f, f3, f7, rd, rs1, rs2)
}
