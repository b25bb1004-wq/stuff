//! Opcode bytes, numbered from 0 in the order of the instruction table in docs/MBC.md.
//! This is the only place the numbering is written down.

use crate::op::Op;

pub(crate) const INT: u8 = 0;
pub(crate) const BYTE: u8 = 1;
pub(crate) const BOOL: u8 = 2;
pub(crate) const UNIT: u8 = 3;
pub(crate) const FN_REF: u8 = 4;
pub(crate) const CONST: u8 = 5;
pub(crate) const MOVE: u8 = 6;
pub(crate) const ADD: u8 = 7;
pub(crate) const SUB: u8 = 8;
pub(crate) const MUL: u8 = 9;
pub(crate) const ADD_W: u8 = 10;
pub(crate) const SUB_W: u8 = 11;
pub(crate) const MUL_W: u8 = 12;
pub(crate) const DIV: u8 = 13;
pub(crate) const REM: u8 = 14;
pub(crate) const NEG: u8 = 15;
pub(crate) const AND: u8 = 16;
pub(crate) const OR: u8 = 17;
pub(crate) const XOR: u8 = 18;
pub(crate) const SHL: u8 = 19;
pub(crate) const SHR: u8 = 20;
pub(crate) const NOT: u8 = 21;
pub(crate) const EQ: u8 = 22;
pub(crate) const NE: u8 = 23;
pub(crate) const LT: u8 = 24;
pub(crate) const LE: u8 = 25;
pub(crate) const GT: u8 = 26;
pub(crate) const GE: u8 = 27;
pub(crate) const JUMP: u8 = 28;
pub(crate) const JUMP_IF: u8 = 29;
pub(crate) const JUMP_IF_NOT: u8 = 30;
pub(crate) const CALL: u8 = 31;
pub(crate) const CALL_VALUE: u8 = 32;
pub(crate) const CALL_HOST: u8 = 33;
pub(crate) const RET: u8 = 34;
pub(crate) const NEW_ARRAY: u8 = 35;
pub(crate) const NEW_BYTES: u8 = 36;
pub(crate) const NEW_FILL: u8 = 37;
pub(crate) const NEW_RECORD: u8 = 38;
pub(crate) const NEW_VARIANT: u8 = 39;
pub(crate) const GET_FIELD: u8 = 40;
pub(crate) const SET_FIELD: u8 = 41;
pub(crate) const TAG: u8 = 42;
pub(crate) const GET_INDEX: u8 = 43;
pub(crate) const SET_INDEX: u8 = 44;
pub(crate) const LEN: u8 = 45;
pub(crate) const PUSH: u8 = 46;
pub(crate) const POP: u8 = 47;
pub(crate) const SLICE: u8 = 48;
pub(crate) const CONCAT: u8 = 49;
pub(crate) const INT_TO_STR: u8 = 50;
pub(crate) const PARSE_INT: u8 = 51;
pub(crate) const STR_TO_BYTES: u8 = 52;
pub(crate) const BYTES_TO_STR: u8 = 53;
pub(crate) const TO_INT: u8 = 54;
pub(crate) const TO_BYTE: u8 = 55;
pub(crate) const ASSERT: u8 = 56;
pub(crate) const TRAP: u8 = 57;
/// Number of instructions in the table.
#[cfg(test)]
pub(crate) const COUNT: u8 = 58;

/// Opcode byte of an instruction.
pub(crate) fn of(op: &Op) -> u8 {
    match op {
        Op::Int(..) => INT,
        Op::Byte(..) => BYTE,
        Op::Bool(..) => BOOL,
        Op::Unit(..) => UNIT,
        Op::FnRef(..) => FN_REF,
        Op::Const(..) => CONST,
        Op::Move(..) => MOVE,
        Op::Add(..) => ADD,
        Op::Sub(..) => SUB,
        Op::Mul(..) => MUL,
        Op::AddW(..) => ADD_W,
        Op::SubW(..) => SUB_W,
        Op::MulW(..) => MUL_W,
        Op::Div(..) => DIV,
        Op::Rem(..) => REM,
        Op::Neg(..) => NEG,
        Op::And(..) => AND,
        Op::Or(..) => OR,
        Op::Xor(..) => XOR,
        Op::Shl(..) => SHL,
        Op::Shr(..) => SHR,
        Op::Not(..) => NOT,
        Op::Eq(..) => EQ,
        Op::Ne(..) => NE,
        Op::Lt(..) => LT,
        Op::Le(..) => LE,
        Op::Gt(..) => GT,
        Op::Ge(..) => GE,
        Op::Jump(..) => JUMP,
        Op::JumpIf(..) => JUMP_IF,
        Op::JumpIfNot(..) => JUMP_IF_NOT,
        Op::Call(..) => CALL,
        Op::CallValue(..) => CALL_VALUE,
        Op::CallHost(..) => CALL_HOST,
        Op::Ret(..) => RET,
        Op::NewArray(..) => NEW_ARRAY,
        Op::NewBytes(..) => NEW_BYTES,
        Op::NewFill(..) => NEW_FILL,
        Op::NewRecord(..) => NEW_RECORD,
        Op::NewVariant(..) => NEW_VARIANT,
        Op::GetField(..) => GET_FIELD,
        Op::SetField(..) => SET_FIELD,
        Op::Tag(..) => TAG,
        Op::GetIndex(..) => GET_INDEX,
        Op::SetIndex(..) => SET_INDEX,
        Op::Len(..) => LEN,
        Op::Push(..) => PUSH,
        Op::Pop(..) => POP,
        Op::Slice(..) => SLICE,
        Op::Concat(..) => CONCAT,
        Op::IntToStr(..) => INT_TO_STR,
        Op::ParseInt(..) => PARSE_INT,
        Op::StrToBytes(..) => STR_TO_BYTES,
        Op::BytesToStr(..) => BYTES_TO_STR,
        Op::ToInt(..) => TO_INT,
        Op::ToByte(..) => TO_BYTE,
        Op::Assert(..) => ASSERT,
        Op::Trap(..) => TRAP,
    }
}
