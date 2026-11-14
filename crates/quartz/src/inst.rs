//! RV64IM, Zicsr, Zifencei and the privileged instructions: the typed form the decoder produces
//! and the encoder consumes.
mod decode;
mod encode;

pub use decode::decode;
pub use encode::encode;

/// One RV64IM instruction, with the Zicsr and privileged instructions.
///
/// `rd`, `rs1` and `rs2` are register numbers (0..=31). `imm` is the sign-extended immediate.
/// For LUI and AUIPC it is the value already shifted left by 12. For the shift-immediate forms
/// it is the shift amount (0..=63, or 0..=31 for W forms). The variant doc covers its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)] // every field is one of rd, rs1, rs2 or imm, described above
pub enum Inst {
    /// LUI: load upper immediate.
    Lui { rd: u8, imm: i64 },
    /// AUIPC: add upper immediate to pc.
    Auipc { rd: u8, imm: i64 },
    /// JAL: jump and link.
    Jal { rd: u8, imm: i64 },
    /// JALR: jump to register plus immediate, and link.
    Jalr { rd: u8, rs1: u8, imm: i64 },
    /// BEQ: branch if equal.
    Beq { rs1: u8, rs2: u8, imm: i64 },
    /// BNE: branch if not equal.
    Bne { rs1: u8, rs2: u8, imm: i64 },
    /// BLT: branch if less than, signed.
    Blt { rs1: u8, rs2: u8, imm: i64 },
    /// BGE: branch if greater or equal, signed.
    Bge { rs1: u8, rs2: u8, imm: i64 },
    /// BLTU: branch if less than, unsigned.
    Bltu { rs1: u8, rs2: u8, imm: i64 },
    /// BGEU: branch if greater or equal, unsigned.
    Bgeu { rs1: u8, rs2: u8, imm: i64 },
    /// LB: load signed byte.
    Lb { rd: u8, rs1: u8, imm: i64 },
    /// LH: load signed halfword.
    Lh { rd: u8, rs1: u8, imm: i64 },
    /// LW: load signed word.
    Lw { rd: u8, rs1: u8, imm: i64 },
    /// LD: load doubleword.
    Ld { rd: u8, rs1: u8, imm: i64 },
    /// LBU: load unsigned byte.
    Lbu { rd: u8, rs1: u8, imm: i64 },
    /// LHU: load unsigned halfword.
    Lhu { rd: u8, rs1: u8, imm: i64 },
    /// LWU: load unsigned word.
    Lwu { rd: u8, rs1: u8, imm: i64 },
    /// SB: store byte.
    Sb { rs1: u8, rs2: u8, imm: i64 },
    /// SH: store halfword.
    Sh { rs1: u8, rs2: u8, imm: i64 },
    /// SW: store word.
    Sw { rs1: u8, rs2: u8, imm: i64 },
    /// SD: store doubleword.
    Sd { rs1: u8, rs2: u8, imm: i64 },
    /// ADDI: add immediate.
    Addi { rd: u8, rs1: u8, imm: i64 },
    /// SLTI: set if less than immediate, signed.
    Slti { rd: u8, rs1: u8, imm: i64 },
    /// SLTIU: set if less than immediate, unsigned.
    Sltiu { rd: u8, rs1: u8, imm: i64 },
    /// XORI: exclusive or with immediate.
    Xori { rd: u8, rs1: u8, imm: i64 },
    /// ORI: or with immediate.
    Ori { rd: u8, rs1: u8, imm: i64 },
    /// ANDI: and with immediate.
    Andi { rd: u8, rs1: u8, imm: i64 },
    /// SLLI: shift left logical by immediate.
    Slli { rd: u8, rs1: u8, imm: i64 },
    /// SRLI: shift right logical by immediate.
    Srli { rd: u8, rs1: u8, imm: i64 },
    /// SRAI: shift right arithmetic by immediate.
    Srai { rd: u8, rs1: u8, imm: i64 },
    /// ADD: rs1 plus rs2.
    Add { rd: u8, rs1: u8, rs2: u8 },
    /// SUB: rs1 minus rs2.
    Sub { rd: u8, rs1: u8, rs2: u8 },
    /// SLL: shift left logical by rs2.
    Sll { rd: u8, rs1: u8, rs2: u8 },
    /// SLT: set if less than, signed.
    Slt { rd: u8, rs1: u8, rs2: u8 },
    /// SLTU: set if less than, unsigned.
    Sltu { rd: u8, rs1: u8, rs2: u8 },
    /// XOR: exclusive or.
    Xor { rd: u8, rs1: u8, rs2: u8 },
    /// SRL: shift right logical by rs2.
    Srl { rd: u8, rs1: u8, rs2: u8 },
    /// SRA: shift right arithmetic by rs2.
    Sra { rd: u8, rs1: u8, rs2: u8 },
    /// OR: bitwise or.
    Or { rd: u8, rs1: u8, rs2: u8 },
    /// AND: bitwise and.
    And { rd: u8, rs1: u8, rs2: u8 },
    /// ADDIW: 32-bit add immediate, sign-extended.
    Addiw { rd: u8, rs1: u8, imm: i64 },
    /// SLLIW: 32-bit shift left logical by immediate, sign-extended.
    Slliw { rd: u8, rs1: u8, imm: i64 },
    /// SRLIW: 32-bit shift right logical by immediate, sign-extended.
    Srliw { rd: u8, rs1: u8, imm: i64 },
    /// SRAIW: 32-bit shift right arithmetic by immediate, sign-extended.
    Sraiw { rd: u8, rs1: u8, imm: i64 },
    /// ADDW: 32-bit add, sign-extended.
    Addw { rd: u8, rs1: u8, rs2: u8 },
    /// SUBW: 32-bit subtract, sign-extended.
    Subw { rd: u8, rs1: u8, rs2: u8 },
    /// SLLW: 32-bit shift left logical, sign-extended.
    Sllw { rd: u8, rs1: u8, rs2: u8 },
    /// SRLW: 32-bit shift right logical, sign-extended.
    Srlw { rd: u8, rs1: u8, rs2: u8 },
    /// SRAW: 32-bit shift right arithmetic, sign-extended.
    Sraw { rd: u8, rs1: u8, rs2: u8 },
    /// FENCE: memory ordering, a no-op in this emulator.
    Fence,
    /// ECALL: environment call.
    Ecall,
    /// EBREAK: environment break.
    Ebreak,
    /// MUL: low 64 bits of the product.
    Mul { rd: u8, rs1: u8, rs2: u8 },
    /// MULH: high 64 bits of the signed by signed product.
    Mulh { rd: u8, rs1: u8, rs2: u8 },
    /// MULHSU: high 64 bits of the signed by unsigned product.
    Mulhsu { rd: u8, rs1: u8, rs2: u8 },
    /// MULHU: high 64 bits of the unsigned product.
    Mulhu { rd: u8, rs1: u8, rs2: u8 },
    /// DIV: signed division.
    Div { rd: u8, rs1: u8, rs2: u8 },
    /// DIVU: unsigned division.
    Divu { rd: u8, rs1: u8, rs2: u8 },
    /// REM: signed remainder.
    Rem { rd: u8, rs1: u8, rs2: u8 },
    /// REMU: unsigned remainder.
    Remu { rd: u8, rs1: u8, rs2: u8 },
    /// MULW: 32-bit multiply, sign-extended.
    Mulw { rd: u8, rs1: u8, rs2: u8 },
    /// DIVW: 32-bit signed division, sign-extended.
    Divw { rd: u8, rs1: u8, rs2: u8 },
    /// DIVUW: 32-bit unsigned division, sign-extended.
    Divuw { rd: u8, rs1: u8, rs2: u8 },
    /// REMW: 32-bit signed remainder, sign-extended.
    Remw { rd: u8, rs1: u8, rs2: u8 },
    /// REMUW: 32-bit unsigned remainder, sign-extended.
    Remuw { rd: u8, rs1: u8, rs2: u8 },
    /// FENCE.I: instruction stream fence, a no-op in this emulator.
    FenceI,
    /// CSRRW: atomic read and write of a CSR.
    Csrrw { rd: u8, rs1: u8, csr: u16 },
    /// CSRRS: atomic read and set bits of a CSR.
    Csrrs { rd: u8, rs1: u8, csr: u16 },
    /// CSRRC: atomic read and clear bits of a CSR.
    Csrrc { rd: u8, rs1: u8, csr: u16 },
    /// CSRRWI: CSRRW with a five-bit zero-extended immediate.
    Csrrwi { rd: u8, uimm: u8, csr: u16 },
    /// CSRRSI: CSRRS with a five-bit zero-extended immediate.
    Csrrsi { rd: u8, uimm: u8, csr: u16 },
    /// CSRRCI: CSRRC with a five-bit zero-extended immediate.
    Csrrci { rd: u8, uimm: u8, csr: u16 },
    /// MRET: return from a machine-mode trap.
    Mret,
    /// SRET: return from a supervisor-mode trap.
    Sret,
    /// WFI: wait for interrupt.
    Wfi,
    /// SFENCE.VMA: supervisor memory-management fence, a no-op here (no TLB).
    SfenceVma { rs1: u8, rs2: u8 },
}
