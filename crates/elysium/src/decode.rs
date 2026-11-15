//! Binary decoder for MBC1. Reads hostile bytes: every count is checked against the bytes left,
//! every LEB128 is bounded, and nothing indexes past the input. It does not run `verify`.

use crate::leb::Reader;
use crate::module::{Const, Func, Module, MAGIC};
use crate::op::Op;
use crate::opcode;
use alloc::vec::Vec;

/// Why bytes are not a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The input does not start with `MBC1`.
    BadMagic,
    /// The input ends before the module does.
    Truncated,
    /// An unknown opcode, or an invalid tag byte (constant kind or bool), with the byte read.
    BadOpcode(u8),
    /// A string is not valid UTF-8.
    BadUtf8,
    /// A count or length that cannot fit in the bytes that remain.
    TooLarge,
    /// A LEB128 number longer or larger than its type allows, or with a redundant zero group.
    Overlong,
}

/// Decode MBC1 bytes into a module. Returns `Err` for anything malformed; never panics.
pub fn decode(bytes: &[u8]) -> Result<Module, DecodeError> {
    let mut r = Reader::new(bytes);
    if r.take(4)? != MAGIC.as_slice() {
        return Err(DecodeError::BadMagic);
    }
    let nconsts = r.count()?;
    let mut consts = Vec::with_capacity(nconsts);
    for _ in 0..nconsts {
        let c = match r.u8()? {
            0 => Const::Str(r.string()?),
            1 => Const::Bytes(r.blob()?.to_vec()),
            other => return Err(DecodeError::BadOpcode(other)),
        };
        consts.push(c);
    }
    let nimports = r.count()?;
    let mut imports = Vec::with_capacity(nimports);
    for _ in 0..nimports {
        imports.push(r.string()?);
    }
    let nfuncs = r.count()?;
    let mut funcs = Vec::with_capacity(nfuncs);
    for _ in 0..nfuncs {
        let name = r.string()?;
        let params = r.uint16()?;
        let regs = r.uint16()?;
        let ncode = r.count()?;
        let mut code = Vec::with_capacity(ncode);
        for _ in 0..ncode {
            code.push(read_op(&mut r)?);
        }
        funcs.push(Func {
            name,
            params,
            regs,
            code,
        });
    }
    let entry = r.uint32()?;
    if r.remaining() != 0 {
        // Trailing bytes would give one module two encodings; refuse them.
        return Err(DecodeError::BadOpcode(r.u8()?));
    }
    Ok(Module {
        consts,
        imports,
        funcs,
        entry,
    })
}

/// Read a bool byte (0 or 1); anything else is an invalid tag.
fn read_bool(r: &mut Reader) -> Result<bool, DecodeError> {
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(DecodeError::BadOpcode(other)),
    }
}

/// Read one instruction; the operand layout mirrors `encode::put_op`.
fn read_op(r: &mut Reader) -> Result<Op, DecodeError> {
    macro_rules! r2 {
        ($v:ident) => {{
            let a = r.uint16()?;
            let b = r.uint16()?;
            Op::$v(a, b)
        }};
    }
    macro_rules! r3 {
        ($v:ident) => {{
            let a = r.uint16()?;
            let b = r.uint16()?;
            let c = r.uint16()?;
            Op::$v(a, b, c)
        }};
    }
    let code = r.u8()?;
    let op = match code {
        opcode::INT => {
            let d = r.uint16()?;
            Op::Int(d, r.sint()?)
        }
        opcode::BYTE => {
            let d = r.uint16()?;
            Op::Byte(d, r.u8()?)
        }
        opcode::BOOL => {
            let d = r.uint16()?;
            Op::Bool(d, read_bool(r)?)
        }
        opcode::UNIT => Op::Unit(r.uint16()?),
        opcode::FN_REF => {
            let d = r.uint16()?;
            Op::FnRef(d, r.uint32()?)
        }
        opcode::CONST => {
            let d = r.uint16()?;
            Op::Const(d, r.uint32()?)
        }
        opcode::MOVE => r2!(Move),
        opcode::ADD => r3!(Add),
        opcode::SUB => r3!(Sub),
        opcode::MUL => r3!(Mul),
        opcode::ADD_W => r3!(AddW),
        opcode::SUB_W => r3!(SubW),
        opcode::MUL_W => r3!(MulW),
        opcode::DIV => r3!(Div),
        opcode::REM => r3!(Rem),
        opcode::NEG => r2!(Neg),
        opcode::AND => r3!(And),
        opcode::OR => r3!(Or),
        opcode::XOR => r3!(Xor),
        opcode::SHL => r3!(Shl),
        opcode::SHR => r3!(Shr),
        opcode::NOT => r2!(Not),
        opcode::EQ => r3!(Eq),
        opcode::NE => r3!(Ne),
        opcode::LT => r3!(Lt),
        opcode::LE => r3!(Le),
        opcode::GT => r3!(Gt),
        opcode::GE => r3!(Ge),
        opcode::JUMP => Op::Jump(r.uint32()?),
        opcode::JUMP_IF => {
            let a = r.uint16()?;
            Op::JumpIf(a, r.uint32()?)
        }
        opcode::JUMP_IF_NOT => {
            let a = r.uint16()?;
            Op::JumpIfNot(a, r.uint32()?)
        }
        opcode::CALL => {
            let d = r.uint16()?;
            let f = r.uint32()?;
            let first = r.uint16()?;
            Op::Call(d, f, first, r.uint16()?)
        }
        opcode::CALL_VALUE => {
            let d = r.uint16()?;
            let a = r.uint16()?;
            let first = r.uint16()?;
            Op::CallValue(d, a, first, r.uint16()?)
        }
        opcode::CALL_HOST => {
            let d = r.uint16()?;
            let h = r.uint32()?;
            let first = r.uint16()?;
            Op::CallHost(d, h, first, r.uint16()?)
        }
        opcode::RET => Op::Ret(r.uint16()?),
        opcode::NEW_ARRAY => r3!(NewArray),
        opcode::NEW_BYTES => r3!(NewBytes),
        opcode::NEW_FILL => r3!(NewFill),
        opcode::NEW_RECORD => r3!(NewRecord),
        opcode::NEW_VARIANT => {
            let d = r.uint16()?;
            let tag = r.uint32()?;
            let first = r.uint16()?;
            Op::NewVariant(d, tag, first, r.uint16()?)
        }
        opcode::GET_FIELD => r3!(GetField),
        opcode::SET_FIELD => r3!(SetField),
        opcode::TAG => r2!(Tag),
        opcode::GET_INDEX => r3!(GetIndex),
        opcode::SET_INDEX => r3!(SetIndex),
        opcode::LEN => r2!(Len),
        opcode::PUSH => r2!(Push),
        opcode::POP => r2!(Pop),
        opcode::SLICE => {
            let d = r.uint16()?;
            let a = r.uint16()?;
            let lo = r.uint16()?;
            Op::Slice(d, a, lo, r.uint16()?)
        }
        opcode::CONCAT => r3!(Concat),
        opcode::INT_TO_STR => r2!(IntToStr),
        opcode::PARSE_INT => r2!(ParseInt),
        opcode::STR_TO_BYTES => r2!(StrToBytes),
        opcode::BYTES_TO_STR => r2!(BytesToStr),
        opcode::TO_INT => r2!(ToInt),
        opcode::TO_BYTE => r2!(ToByte),
        opcode::ASSERT => {
            let a = r.uint16()?;
            Op::Assert(a, r.uint32()?)
        }
        opcode::TRAP => Op::Trap(r.uint32()?),
        other => return Err(DecodeError::BadOpcode(other)),
    };
    Ok(op)
}
