//! Binary encoder for MBC1 (the "Binary format" section of docs/MBC.md).

use crate::leb::{put_sint, put_uint};
use crate::module::{Const, Module, MAGIC};
use crate::op::Op;
use crate::opcode;
use alloc::vec::Vec;

/// Encode a module as MBC1 bytes. Never fails; `decode` reverses it.
pub fn encode(module: &Module) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    put_uint(&mut out, module.consts.len() as u64);
    for c in &module.consts {
        match c {
            Const::Str(s) => {
                out.push(0);
                put_bytes(&mut out, s.as_bytes());
            }
            Const::Bytes(b) => {
                out.push(1);
                put_bytes(&mut out, b);
            }
        }
    }
    put_uint(&mut out, module.imports.len() as u64);
    for name in &module.imports {
        put_bytes(&mut out, name.as_bytes());
    }
    put_uint(&mut out, module.funcs.len() as u64);
    for f in &module.funcs {
        put_bytes(&mut out, f.name.as_bytes());
        put_uint(&mut out, u64::from(f.params));
        put_uint(&mut out, u64::from(f.regs));
        put_uint(&mut out, f.code.len() as u64);
        for op in &f.code {
            put_op(&mut out, op);
        }
    }
    put_uint(&mut out, u64::from(module.entry));
    out
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_uint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn put_op(out: &mut Vec<u8>, op: &Op) {
    out.push(opcode::of(op));
    let r = |out: &mut Vec<u8>, v: u16| put_uint(out, u64::from(v));
    let w = |out: &mut Vec<u8>, v: u32| put_uint(out, u64::from(v));
    match *op {
        Op::Int(d, i) => {
            r(out, d);
            put_sint(out, i);
        }
        Op::Byte(d, b) => {
            r(out, d);
            out.push(b);
        }
        Op::Bool(d, b) => {
            r(out, d);
            out.push(u8::from(b));
        }
        Op::Unit(d) | Op::Ret(d) => r(out, d),
        Op::Trap(code) => w(out, code),
        Op::FnRef(d, x) | Op::Const(d, x) => {
            r(out, d);
            w(out, x);
        }
        Op::Move(d, a)
        | Op::Neg(d, a)
        | Op::Not(d, a)
        | Op::Tag(d, a)
        | Op::Len(d, a)
        | Op::Pop(d, a)
        | Op::Push(d, a)
        | Op::IntToStr(d, a)
        | Op::ParseInt(d, a)
        | Op::StrToBytes(d, a)
        | Op::BytesToStr(d, a)
        | Op::ToInt(d, a)
        | Op::ToByte(d, a) => {
            r(out, d);
            r(out, a);
        }
        Op::Add(d, a, b)
        | Op::Sub(d, a, b)
        | Op::Mul(d, a, b)
        | Op::AddW(d, a, b)
        | Op::SubW(d, a, b)
        | Op::MulW(d, a, b)
        | Op::Div(d, a, b)
        | Op::Rem(d, a, b)
        | Op::And(d, a, b)
        | Op::Or(d, a, b)
        | Op::Xor(d, a, b)
        | Op::Shl(d, a, b)
        | Op::Shr(d, a, b)
        | Op::Eq(d, a, b)
        | Op::Ne(d, a, b)
        | Op::Lt(d, a, b)
        | Op::Le(d, a, b)
        | Op::Gt(d, a, b)
        | Op::Ge(d, a, b)
        | Op::Concat(d, a, b)
        | Op::GetIndex(d, a, b)
        | Op::NewFill(d, a, b)
        | Op::NewArray(d, a, b)
        | Op::NewBytes(d, a, b)
        | Op::NewRecord(d, a, b)
        | Op::GetField(d, a, b)
        | Op::SetField(d, a, b)
        | Op::SetIndex(d, a, b) => {
            r(out, d);
            r(out, a);
            r(out, b);
        }
        Op::Slice(d, a, lo, hi) => {
            r(out, d);
            r(out, a);
            r(out, lo);
            r(out, hi);
        }
        Op::Jump(t) => w(out, t),
        Op::JumpIf(a, t) | Op::JumpIfNot(a, t) | Op::Assert(a, t) => {
            r(out, a);
            w(out, t);
        }
        Op::Call(d, f, first, n) => {
            r(out, d);
            w(out, f);
            r(out, first);
            r(out, n);
        }
        Op::CallValue(d, a, first, n) => {
            r(out, d);
            r(out, a);
            r(out, first);
            r(out, n);
        }
        Op::CallHost(d, h, first, n) | Op::NewVariant(d, h, first, n) => {
            r(out, d);
            w(out, h);
            r(out, first);
            r(out, n);
        }
    }
}
