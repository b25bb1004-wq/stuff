//! Test-only generator of random modules that satisfy every verifier rule.

#![allow(clippy::unwrap_used)]

use crate::module::{Const, Func, Module};
use crate::op::Op;
use crate::opcode;
use assay::Rng;

/// What the generator knows about the module it is filling.
struct Ctx {
    regs: u16,
    len: usize,
    nconsts: usize,
    strs: Vec<u32>,
    nimports: usize,
    params: Vec<u16>,
}

/// A random register span `first..first+n` that fits in `regs`.
fn span(r: &mut Rng, regs: u16) -> (u16, u16) {
    let n = r.below(usize::from(regs.min(4)) + 1);
    let first = r.below(usize::from(regs) - n + 1);
    (first as u16, n as u16)
}

/// A random module that verifies, with every instruction kind appearing across many cases.
pub(crate) fn random_module(r: &mut Rng) -> Module {
    let mut consts = vec![Const::Str(String::from("assert failed"))];
    let mut strs = vec![0u32];
    for i in 1..=r.below(4) {
        if r.chance(1, 2) {
            strs.push(i as u32);
            consts.push(Const::Str(format!("msg{i}")));
        } else {
            consts.push(Const::Bytes(r.bytes(6)));
        }
    }
    let imports: Vec<String> = (0..=r.below(2)).map(|i| format!("io.f{i}")).collect();
    let nfuncs = 1 + r.below(4);
    let params: Vec<u16> = (0..nfuncs).map(|_| r.below(4) as u16).collect();
    let maxp = params.iter().copied().max().unwrap_or(0);
    let mut funcs = Vec::with_capacity(nfuncs);
    for (f, &p) in params.iter().enumerate() {
        let regs = maxp + 1 + r.below(6) as u16;
        let body = 1 + r.below(24);
        let cx = Ctx {
            regs,
            len: body + 1,
            nconsts: consts.len(),
            strs: strs.clone(),
            nimports: imports.len(),
            params: params.clone(),
        };
        let mut code = Vec::with_capacity(body + 1);
        for _ in 0..body {
            let kind = r.below(usize::from(opcode::COUNT)) as u8;
            code.push(gen_op(r, &cx, kind));
        }
        code.push(Op::Ret(r.below(usize::from(regs)) as u16));
        funcs.push(Func {
            name: format!("f{f}"),
            params: p,
            regs,
            code,
        });
    }
    let entry = r.below(nfuncs) as u32;
    Module {
        consts,
        imports,
        funcs,
        entry,
    }
}

/// One instruction of the given opcode kind, with every operand in range.
fn gen_op(r: &mut Rng, cx: &Ctx, kind: u8) -> Op {
    let regs = usize::from(cx.regs);
    let reg = |r: &mut Rng| r.below(regs) as u16;
    let d = reg(r);
    let a = reg(r);
    let b = reg(r);
    let c = reg(r);
    let (first, n) = span(r, cx.regs);
    let f = r.below(cx.params.len());
    let callee_n = cx.params[f];
    let call_first = r.below(regs - usize::from(callee_n) + 1) as u16;
    let t = r.below(cx.len) as u32;
    let k = r.below(cx.nconsts) as u32;
    let s = *r.pick(&cx.strs);
    let h = r.below(cx.nimports) as u32;
    let fi = r.below(4) as u16;
    let imm = r.u64() as i64;
    let byte = r.u64() as u8;
    let flag = r.chance(1, 2);
    let word = r.u64() as u32;
    match kind {
        opcode::INT => Op::Int(d, imm),
        opcode::BYTE => Op::Byte(d, byte),
        opcode::BOOL => Op::Bool(d, flag),
        opcode::UNIT => Op::Unit(d),
        opcode::FN_REF => Op::FnRef(d, f as u32),
        opcode::CONST => Op::Const(d, k),
        opcode::MOVE => Op::Move(d, a),
        opcode::ADD => Op::Add(d, a, b),
        opcode::SUB => Op::Sub(d, a, b),
        opcode::MUL => Op::Mul(d, a, b),
        opcode::ADD_W => Op::AddW(d, a, b),
        opcode::SUB_W => Op::SubW(d, a, b),
        opcode::MUL_W => Op::MulW(d, a, b),
        opcode::DIV => Op::Div(d, a, b),
        opcode::REM => Op::Rem(d, a, b),
        opcode::NEG => Op::Neg(d, a),
        opcode::AND => Op::And(d, a, b),
        opcode::OR => Op::Or(d, a, b),
        opcode::XOR => Op::Xor(d, a, b),
        opcode::SHL => Op::Shl(d, a, b),
        opcode::SHR => Op::Shr(d, a, b),
        opcode::NOT => Op::Not(d, a),
        opcode::EQ => Op::Eq(d, a, b),
        opcode::NE => Op::Ne(d, a, b),
        opcode::LT => Op::Lt(d, a, b),
        opcode::LE => Op::Le(d, a, b),
        opcode::GT => Op::Gt(d, a, b),
        opcode::GE => Op::Ge(d, a, b),
        opcode::JUMP => Op::Jump(t),
        opcode::JUMP_IF => Op::JumpIf(a, t),
        opcode::JUMP_IF_NOT => Op::JumpIfNot(a, t),
        opcode::CALL => Op::Call(d, f as u32, call_first, callee_n),
        opcode::CALL_VALUE => Op::CallValue(d, a, first, n),
        opcode::CALL_HOST => Op::CallHost(d, h, first, n),
        opcode::RET => Op::Ret(a),
        opcode::NEW_ARRAY => Op::NewArray(d, first, n),
        opcode::NEW_BYTES => Op::NewBytes(d, first, n),
        opcode::NEW_FILL => Op::NewFill(d, a, b),
        opcode::NEW_RECORD => Op::NewRecord(d, first, n),
        opcode::NEW_VARIANT => Op::NewVariant(d, word, first, n),
        opcode::GET_FIELD => Op::GetField(d, a, fi),
        opcode::SET_FIELD => Op::SetField(a, fi, b),
        opcode::TAG => Op::Tag(d, a),
        opcode::GET_INDEX => Op::GetIndex(d, a, b),
        opcode::SET_INDEX => Op::SetIndex(a, b, c),
        opcode::LEN => Op::Len(d, a),
        opcode::PUSH => Op::Push(a, b),
        opcode::POP => Op::Pop(d, a),
        opcode::SLICE => Op::Slice(d, a, b, c),
        opcode::CONCAT => Op::Concat(d, a, b),
        opcode::INT_TO_STR => Op::IntToStr(d, a),
        opcode::PARSE_INT => Op::ParseInt(d, a),
        opcode::STR_TO_BYTES => Op::StrToBytes(d, a),
        opcode::BYTES_TO_STR => Op::BytesToStr(d, a),
        opcode::TO_INT => Op::ToInt(d, a),
        opcode::TO_BYTE => Op::ToByte(d, a),
        opcode::ASSERT => Op::Assert(a, s),
        opcode::TRAP => Op::Trap(word),
        // Unreachable for kinds from 0..COUNT; the coverage test would catch a missing arm.
        _ => Op::Trap(0),
    }
}
