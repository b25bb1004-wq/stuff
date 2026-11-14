//! Iterative bytecode-free IR virtual machine for Mica.

use crate::ast::{BinOp, UnOp};
use crate::ir::{Body, Builtin, FuncId, Inst, Lit, Program, Reg};
use crate::span::Span;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use super::Trap;

const MAX_CALL_DEPTH: usize = 10_000;

/// Runtime value in the Mica reference interpreter.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Val {
    Int(i64),
    Byte(u8),
    Bool(bool),
    Str(Rc<String>),
    Unit,
    Array(Rc<RefCell<Vec<Val>>>),
    Tuple(Rc<Vec<Val>>),
    Record(Rc<RefCell<Vec<Val>>>),
    Variant(u32, Rc<Vec<Val>>),
    Opt(Option<Box<Val>>),
    Fn(FuncId),
}

impl Val {
    /// Deep-clones mutable containers so property guards cannot alias-mutate prop bodies.
    pub(super) fn deep_clone(&self) -> Self {
        match self {
            Self::Array(a) => {
                let items = a.borrow().iter().map(Self::deep_clone).collect();
                Self::Array(Rc::new(RefCell::new(items)))
            }
            Self::Tuple(t) => Self::Tuple(Rc::new(t.iter().map(Self::deep_clone).collect())),
            Self::Record(r) => {
                let fields = r.borrow().iter().map(Self::deep_clone).collect();
                Self::Record(Rc::new(RefCell::new(fields)))
            }
            Self::Variant(tag, fields) => {
                Self::Variant(*tag, Rc::new(fields.iter().map(Self::deep_clone).collect()))
            }
            Self::Opt(Some(v)) => Self::Opt(Some(Box::new(v.deep_clone()))),
            other => other.clone(),
        }
    }
}

/// SplitMix64 deterministic pseudo-random generator.
#[derive(Clone, Debug)]
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(super) fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(super) fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.u64() % (hi - lo)
    }

    pub(super) fn below(&mut self, n: usize) -> usize {
        self.range(0, n as u64) as usize
    }

    pub(super) fn chance(&mut self, num: u64, den: u64) -> bool {
        self.range(0, den) < num
    }
}

struct Frame {
    func_id: FuncId,
    pc: usize,
    regs: Vec<Val>,
    ret_dst: Option<Reg>,
}

pub(super) struct Vm<'a> {
    prog: &'a Program,
    input: &'a str,
    input_pos: usize,
    pub(super) output: String,
    fuel: u64,
    rand_rng: Rng,
    label_maps: Vec<Option<Vec<usize>>>,
    stack: Vec<Frame>,
}

impl<'a> Vm<'a> {
    pub(super) fn new(prog: &'a Program, input: &'a str, fuel: u64) -> Self {
        Self {
            prog,
            input,
            input_pos: 0,
            output: String::new(),
            fuel,
            rand_rng: Rng::new(1),
            label_maps: vec![None; prog.funcs.len()],
            stack: Vec::new(),
        }
    }

    pub(super) fn run_func(&mut self, fid: FuncId, args: &[Val]) -> Result<Val, (Trap, Span)> {
        self.push_frame(fid, args, None, Span { start: 0, end: 0 })?;
        while let Some(frame) = self.stack.last() {
            let cur_fid = frame.func_id;
            let pc = frame.pc;
            let Some(func) = self.prog.funcs.get(cur_fid as usize) else {
                return Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 }));
            };
            let Body::Code { insts, spans } = &func.body else {
                return Err((Trap::NoExtern(func.name.clone()), Span { start: 0, end: 0 }));
            };
            let Some(inst) = insts.get(pc) else {
                self.stack.pop();
                if self.stack.is_empty() {
                    return Ok(Val::Unit);
                }
                continue;
            };
            let span = spans.get(pc).copied().unwrap_or(Span { start: 0, end: 0 });
            if self.fuel == 0 {
                return Err((Trap::OutOfFuel, span));
            }
            self.fuel -= 1;

            if let Some(ret_val) = self.exec_inst(cur_fid, pc, inst, span)? {
                if self.stack.is_empty() {
                    return Ok(ret_val);
                }
            }
        }
        Ok(Val::Unit)
    }

    fn push_frame(
        &mut self,
        fid: FuncId,
        args: &[Val],
        ret_dst: Option<Reg>,
        span: Span,
    ) -> Result<(), (Trap, Span)> {
        let Some(func) = self.prog.funcs.get(fid as usize) else {
            return Err((Trap::IndexOutOfRange, span));
        };
        match &func.body {
            Body::Extern(name) => {
                let res = self.call_extern(name, args, span)?;
                if let Some(dst) = ret_dst {
                    self.set_top_reg(dst, res, span)?;
                }
                Ok(())
            }
            Body::Code { insts, .. } => {
                if self.stack.len() >= MAX_CALL_DEPTH {
                    return Err((Trap::StackOverflow, span));
                }
                self.ensure_label_map(fid, insts);
                let mut regs = vec![Val::Unit; func.regs.len()];
                for (slot, arg) in regs.iter_mut().zip(args.iter()) {
                    *slot = arg.clone();
                }
                self.stack.push(Frame {
                    func_id: fid,
                    pc: 0,
                    regs,
                    ret_dst,
                });
                Ok(())
            }
        }
    }

    fn ensure_label_map(&mut self, fid: FuncId, insts: &[Inst]) {
        if let Some(slot) = self.label_maps.get(fid as usize) {
            if slot.is_some() {
                return;
            }
        } else {
            return;
        }
        let max_lbl = insts
            .iter()
            .filter_map(|i| match i {
                Inst::Label(l) => Some(*l as usize),
                _ => None,
            })
            .max();
        let mut map = vec![0usize; max_lbl.map_or(0, |m| m + 1)];
        for (idx, inst) in insts.iter().enumerate() {
            if let Inst::Label(l) = inst {
                if let Some(entry) = map.get_mut(*l as usize) {
                    *entry = idx;
                }
            }
        }
        if let Some(slot) = self.label_maps.get_mut(fid as usize) {
            *slot = Some(map);
        }
    }

    fn label_pc(&self, fid: FuncId, lbl: u32, span: Span) -> Result<usize, (Trap, Span)> {
        self.label_maps
            .get(fid as usize)
            .and_then(|o| o.as_ref())
            .and_then(|m| m.get(lbl as usize))
            .copied()
            .ok_or((Trap::IndexOutOfRange, span))
    }

    fn top_reg(&self, r: Reg, span: Span) -> Result<&Val, (Trap, Span)> {
        self.stack
            .last()
            .and_then(|f| f.regs.get(r as usize))
            .ok_or((Trap::IndexOutOfRange, span))
    }

    fn set_top_reg(&mut self, r: Reg, v: Val, span: Span) -> Result<(), (Trap, Span)> {
        let slot = self
            .stack
            .last_mut()
            .and_then(|f| f.regs.get_mut(r as usize))
            .ok_or((Trap::IndexOutOfRange, span))?;
        *slot = v;
        Ok(())
    }

    fn set_top_pc(&mut self, next_pc: usize) {
        if let Some(f) = self.stack.last_mut() {
            f.pc = next_pc;
        }
    }

    fn collect_regs(&self, rs: &[Reg], span: Span) -> Result<Vec<Val>, (Trap, Span)> {
        let mut out = Vec::with_capacity(rs.len());
        for &r in rs {
            out.push(self.top_reg(r, span)?.clone());
        }
        Ok(out)
    }

    fn exec_inst(
        &mut self,
        fid: FuncId,
        pc: usize,
        inst: &Inst,
        span: Span,
    ) -> Result<Option<Val>, (Trap, Span)> {
        match inst {
            Inst::Lit(dst, lit) => {
                let val = match lit {
                    Lit::Int(n) => Val::Int(*n),
                    Lit::Byte(b) => Val::Byte(*b),
                    Lit::Bool(b) => Val::Bool(*b),
                    Lit::Unit => Val::Unit,
                    Lit::Str(s) => Val::Str(Rc::new(s.clone())),
                    Lit::Fn(id) => Val::Fn(*id),
                };
                self.set_top_reg(*dst, val, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::Move(dst, src) => {
                let val = self.top_reg(*src, span)?.clone();
                self.set_top_reg(*dst, val, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::Un(dst, op, src) => {
                let v = eval_un(*op, self.top_reg(*src, span)?, span)?;
                self.set_top_reg(*dst, v, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::Bin(dst, op, lhs, rhs) => {
                let a = self.top_reg(*lhs, span)?;
                let b = self.top_reg(*rhs, span)?;
                let v = eval_bin(*op, a, b, span)?;
                self.set_top_reg(*dst, v, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::Call(dst, callee, args) => {
                let arg_vals = self.collect_regs(args, span)?;
                self.set_top_pc(pc + 1);
                self.push_frame(*callee, &arg_vals, Some(*dst), span)?;
            }
            Inst::CallValue(dst, callee_reg, args) => {
                let Val::Fn(callee) = *self.top_reg(*callee_reg, span)? else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let arg_vals = self.collect_regs(args, span)?;
                self.set_top_pc(pc + 1);
                self.push_frame(callee, &arg_vals, Some(*dst), span)?;
            }
            Inst::Builtin(dst, builtin, args) => {
                let arg_vals = self.collect_regs(args, span)?;
                let res = eval_builtin(*builtin, &arg_vals, span)?;
                self.set_top_reg(*dst, res, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewArray(dst, elems) => {
                let vals = self.collect_regs(elems, span)?;
                self.set_top_reg(*dst, Val::Array(Rc::new(RefCell::new(vals))), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewFill(dst, val_reg, count_reg) => {
                let val = self.top_reg(*val_reg, span)?.clone();
                let Val::Int(count) = *self.top_reg(*count_reg, span)? else {
                    return Err((Trap::Overflow, span));
                };
                if count < 0 {
                    return Err((Trap::IndexOutOfRange, span));
                }
                let n = usize::try_from(count).map_err(|_| (Trap::Overflow, span))?;
                self.set_top_reg(*dst, Val::Array(Rc::new(RefCell::new(vec![val; n]))), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewTuple(dst, elems) => {
                let vals = self.collect_regs(elems, span)?;
                self.set_top_reg(*dst, Val::Tuple(Rc::new(vals)), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewRecord(dst, _tid, fields) => {
                let vals = self.collect_regs(fields, span)?;
                self.set_top_reg(*dst, Val::Record(Rc::new(RefCell::new(vals))), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewVariant(dst, _tid, tag, fields) => {
                let vals = self.collect_regs(fields, span)?;
                self.set_top_reg(*dst, Val::Variant(*tag, Rc::new(vals)), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewSome(dst, src) => {
                let val = self.top_reg(*src, span)?.clone();
                self.set_top_reg(*dst, Val::Opt(Some(Box::new(val))), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::NewNone(dst) => {
                self.set_top_reg(*dst, Val::Opt(None), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::GetField(dst, base, idx) => {
                let val = match self.top_reg(*base, span)? {
                    Val::Record(r) => r.borrow().get(*idx as usize).cloned(),
                    Val::Tuple(t) => t.get(*idx as usize).cloned(),
                    Val::Variant(_, f) => f.get(*idx as usize).cloned(),
                    Val::Opt(Some(v)) if *idx == 0 => Some((**v).clone()),
                    _ => None,
                }
                .ok_or((Trap::IndexOutOfRange, span))?;
                self.set_top_reg(*dst, val, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::SetField(base, idx, val_reg) => {
                let val = self.top_reg(*val_reg, span)?.clone();
                let Val::Record(r) = self.top_reg(*base, span)?.clone() else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let mut borrow = r.borrow_mut();
                let slot = borrow
                    .get_mut(*idx as usize)
                    .ok_or((Trap::IndexOutOfRange, span))?;
                *slot = val;
                self.set_top_pc(pc + 1);
            }
            Inst::Tag(dst, src) => {
                let tag = match self.top_reg(*src, span)? {
                    Val::Variant(t, _) => i64::from(*t),
                    Val::Opt(None) => 0,
                    Val::Opt(Some(_)) => 1,
                    _ => return Err((Trap::IndexOutOfRange, span)),
                };
                self.set_top_reg(*dst, Val::Int(tag), span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::GetIndex(dst, base, idx_reg) => {
                let Val::Int(idx) = *self.top_reg(*idx_reg, span)? else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let Val::Array(arr) = self.top_reg(*base, span)? else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let i = usize::try_from(idx).map_err(|_| (Trap::IndexOutOfRange, span))?;
                let val = arr
                    .borrow()
                    .get(i)
                    .cloned()
                    .ok_or((Trap::IndexOutOfRange, span))?;
                self.set_top_reg(*dst, val, span)?;
                self.set_top_pc(pc + 1);
            }
            Inst::SetIndex(base, idx_reg, val_reg) => {
                let Val::Int(idx) = *self.top_reg(*idx_reg, span)? else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let val = self.top_reg(*val_reg, span)?.clone();
                let Val::Array(arr) = self.top_reg(*base, span)?.clone() else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let i = usize::try_from(idx).map_err(|_| (Trap::IndexOutOfRange, span))?;
                let mut borrow = arr.borrow_mut();
                let slot = borrow.get_mut(i).ok_or((Trap::IndexOutOfRange, span))?;
                *slot = val;
                self.set_top_pc(pc + 1);
            }
            Inst::Label(_) => {
                self.set_top_pc(pc + 1);
            }
            Inst::Jump(lbl) => {
                let target = self.label_pc(fid, *lbl, span)?;
                self.set_top_pc(target);
            }
            Inst::Branch(cond_reg, t_lbl, f_lbl) => {
                let Val::Bool(cond) = *self.top_reg(*cond_reg, span)? else {
                    return Err((Trap::IndexOutOfRange, span));
                };
                let target = self.label_pc(fid, if cond { *t_lbl } else { *f_lbl }, span)?;
                self.set_top_pc(target);
            }
            Inst::Ret(reg) => {
                let val = self.top_reg(*reg, span)?.clone();
                let popped = self.stack.pop();
                if let Some(Frame {
                    ret_dst: Some(dst), ..
                }) = popped
                {
                    self.set_top_reg(dst, val.clone(), span)?;
                }
                return Ok(Some(val));
            }
            Inst::Assert(cond_reg, msg) => {
                let Val::Bool(cond) = *self.top_reg(*cond_reg, span)? else {
                    return Err((Trap::AssertFailed(msg.clone()), span));
                };
                if !cond {
                    return Err((Trap::AssertFailed(msg.clone()), span));
                }
                self.set_top_pc(pc + 1);
            }
        }
        Ok(None)
    }

    fn call_extern(&mut self, name: &str, args: &[Val], span: Span) -> Result<Val, (Trap, Span)> {
        match name {
            "io.print" => {
                if let Some(Val::Str(s)) = args.first() {
                    self.output.push_str(s);
                }
                Ok(Val::Unit)
            }
            "io.read_line" => {
                if self.input_pos >= self.input.len() {
                    return Ok(Val::Opt(None));
                }
                let rest = &self.input[self.input_pos..];
                let line = match rest.find('\n') {
                    Some(nl) => {
                        self.input_pos += nl + 1;
                        let s = &rest[..nl];
                        s.strip_suffix('\r').unwrap_or(s)
                    }
                    None => {
                        self.input_pos = self.input.len();
                        rest
                    }
                };
                Ok(Val::Opt(Some(Box::new(Val::Str(Rc::new(String::from(
                    line,
                )))))))
            }
            "time.now" => Ok(Val::Int(0)),
            "rand.next" => Ok(Val::Int(self.rand_rng.u64() as i64)),
            _ => Err((Trap::NoExtern(String::from(name)), span)),
        }
    }
}

fn eval_un(op: UnOp, val: &Val, span: Span) -> Result<Val, (Trap, Span)> {
    match (op, val) {
        (UnOp::Neg, Val::Int(n)) => n.checked_neg().map(Val::Int).ok_or((Trap::Overflow, span)),
        (UnOp::Neg, Val::Byte(0)) => Ok(Val::Byte(0)),
        (UnOp::Neg, Val::Byte(_)) => Err((Trap::Overflow, span)),
        (UnOp::Not, Val::Bool(b)) => Ok(Val::Bool(!b)),
        _ => Err((Trap::Overflow, span)),
    }
}

fn eval_bin(op: BinOp, lhs: &Val, rhs: &Val, span: Span) -> Result<Val, (Trap, Span)> {
    match (lhs, rhs) {
        (Val::Int(a), Val::Int(b)) => eval_int_bin(op, *a, *b, span),
        (Val::Byte(a), Val::Byte(b)) => eval_byte_bin(op, *a, *b, span),
        (Val::Bool(a), Val::Bool(b)) => match op {
            BinOp::Eq => Ok(Val::Bool(a == b)),
            BinOp::Ne => Ok(Val::Bool(a != b)),
            _ => Err((Trap::Overflow, span)),
        },
        (Val::Unit, Val::Unit) => match op {
            BinOp::Eq => Ok(Val::Bool(true)),
            BinOp::Ne => Ok(Val::Bool(false)),
            _ => Err((Trap::Overflow, span)),
        },
        (Val::Str(a), Val::Str(b)) => match op {
            BinOp::Eq => Ok(Val::Bool(a == b)),
            BinOp::Ne => Ok(Val::Bool(a != b)),
            BinOp::Lt => Ok(Val::Bool(a < b)),
            BinOp::Le => Ok(Val::Bool(a <= b)),
            BinOp::Gt => Ok(Val::Bool(a > b)),
            BinOp::Ge => Ok(Val::Bool(a >= b)),
            _ => Err((Trap::Overflow, span)),
        },
        _ => Err((Trap::Overflow, span)),
    }
}

fn eval_int_bin(op: BinOp, a: i64, b: i64, span: Span) -> Result<Val, (Trap, Span)> {
    let val = match op {
        BinOp::Add => Val::Int(a.checked_add(b).ok_or((Trap::Overflow, span))?),
        BinOp::Sub => Val::Int(a.checked_sub(b).ok_or((Trap::Overflow, span))?),
        BinOp::Mul => Val::Int(a.checked_mul(b).ok_or((Trap::Overflow, span))?),
        BinOp::AddWrap => Val::Int(a.wrapping_add(b)),
        BinOp::SubWrap => Val::Int(a.wrapping_sub(b)),
        BinOp::MulWrap => Val::Int(a.wrapping_mul(b)),
        BinOp::Div => {
            if b == 0 {
                return Err((Trap::DivideByZero, span));
            }
            Val::Int(a.checked_div(b).ok_or((Trap::Overflow, span))?)
        }
        BinOp::Rem => {
            if b == 0 {
                return Err((Trap::DivideByZero, span));
            }
            Val::Int(a.checked_rem(b).ok_or((Trap::Overflow, span))?)
        }
        BinOp::Shl => {
            if !(0..=63).contains(&b) {
                return Err((Trap::Overflow, span));
            }
            Val::Int(a.wrapping_shl(b as u32))
        }
        BinOp::Shr => {
            if !(0..=63).contains(&b) {
                return Err((Trap::Overflow, span));
            }
            Val::Int(a.wrapping_shr(b as u32))
        }
        BinOp::BitOr => Val::Int(a | b),
        BinOp::BitXor => Val::Int(a ^ b),
        BinOp::BitAnd => Val::Int(a & b),
        BinOp::Eq => Val::Bool(a == b),
        BinOp::Ne => Val::Bool(a != b),
        BinOp::Lt => Val::Bool(a < b),
        BinOp::Le => Val::Bool(a <= b),
        BinOp::Gt => Val::Bool(a > b),
        BinOp::Ge => Val::Bool(a >= b),
        BinOp::And | BinOp::Or => return Err((Trap::Overflow, span)),
    };
    Ok(val)
}

fn eval_byte_bin(op: BinOp, a: u8, b: u8, span: Span) -> Result<Val, (Trap, Span)> {
    let val = match op {
        BinOp::Add => Val::Byte(a.checked_add(b).ok_or((Trap::Overflow, span))?),
        BinOp::Sub => Val::Byte(a.checked_sub(b).ok_or((Trap::Overflow, span))?),
        BinOp::Mul => Val::Byte(a.checked_mul(b).ok_or((Trap::Overflow, span))?),
        BinOp::AddWrap => Val::Byte(a.wrapping_add(b)),
        BinOp::SubWrap => Val::Byte(a.wrapping_sub(b)),
        BinOp::MulWrap => Val::Byte(a.wrapping_mul(b)),
        BinOp::Div => {
            if b == 0 {
                return Err((Trap::DivideByZero, span));
            }
            Val::Byte(a / b)
        }
        BinOp::Rem => {
            if b == 0 {
                return Err((Trap::DivideByZero, span));
            }
            Val::Byte(a % b)
        }
        BinOp::Shl => {
            if b >= 8 {
                return Err((Trap::Overflow, span));
            }
            Val::Byte(a << b)
        }
        BinOp::Shr => {
            if b >= 8 {
                return Err((Trap::Overflow, span));
            }
            Val::Byte(a >> b)
        }
        BinOp::BitOr => Val::Byte(a | b),
        BinOp::BitXor => Val::Byte(a ^ b),
        BinOp::BitAnd => Val::Byte(a & b),
        BinOp::Eq => Val::Bool(a == b),
        BinOp::Ne => Val::Bool(a != b),
        BinOp::Lt => Val::Bool(a < b),
        BinOp::Le => Val::Bool(a <= b),
        BinOp::Gt => Val::Bool(a > b),
        BinOp::Ge => Val::Bool(a >= b),
        BinOp::And | BinOp::Or => return Err((Trap::Overflow, span)),
    };
    Ok(val)
}

fn eval_builtin(b: Builtin, args: &[Val], span: Span) -> Result<Val, (Trap, Span)> {
    match b {
        Builtin::Len => match args.first() {
            Some(Val::Str(s)) => Ok(Val::Int(s.len() as i64)),
            Some(Val::Array(a)) => Ok(Val::Int(a.borrow().len() as i64)),
            _ => Err((Trap::IndexOutOfRange, span)),
        },
        Builtin::Push => match (args.first(), args.get(1)) {
            (Some(Val::Array(a)), Some(v)) => {
                a.borrow_mut().push(v.clone());
                Ok(Val::Unit)
            }
            _ => Err((Trap::IndexOutOfRange, span)),
        },
        Builtin::Pop => match args.first() {
            Some(Val::Array(a)) => Ok(Val::Opt(a.borrow_mut().pop().map(Box::new))),
            _ => Err((Trap::IndexOutOfRange, span)),
        },
        Builtin::Slice => eval_slice(args, span),
        Builtin::Concat => match (args.first(), args.get(1)) {
            (Some(Val::Str(a)), Some(Val::Str(b))) => {
                let mut s = String::with_capacity(a.len() + b.len());
                s.push_str(a);
                s.push_str(b);
                Ok(Val::Str(Rc::new(s)))
            }
            (Some(Val::Array(a)), Some(Val::Array(b))) => {
                let ab = a.borrow();
                let bb = b.borrow();
                let mut out = Vec::with_capacity(ab.len() + bb.len());
                out.extend_from_slice(&ab);
                out.extend_from_slice(&bb);
                Ok(Val::Array(Rc::new(RefCell::new(out))))
            }
            _ => Err((Trap::IndexOutOfRange, span)),
        },
        Builtin::IntToStr => match args.first() {
            Some(Val::Int(n)) => Ok(Val::Str(Rc::new(format!("{n}")))),
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::ParseInt => match args.first() {
            Some(Val::Str(s)) => Ok(Val::Opt(
                s.parse::<i64>().ok().map(|n| Box::new(Val::Int(n))),
            )),
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::Bytes => match args.first() {
            Some(Val::Str(s)) => {
                let bytes = s.as_bytes().iter().copied().map(Val::Byte).collect();
                Ok(Val::Array(Rc::new(RefCell::new(bytes))))
            }
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::Text => match args.first() {
            Some(Val::Array(a)) => {
                let mut raw = Vec::with_capacity(a.borrow().len());
                for item in a.borrow().iter() {
                    let Val::Byte(byte) = *item else {
                        return Err((Trap::Overflow, span));
                    };
                    raw.push(byte);
                }
                match String::from_utf8(raw) {
                    Ok(s) => Ok(Val::Opt(Some(Box::new(Val::Str(Rc::new(s)))))),
                    Err(_) => Ok(Val::Opt(None)),
                }
            }
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::ToInt => match args.first() {
            Some(Val::Byte(byte)) => Ok(Val::Int(i64::from(*byte))),
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::ToByte => match args.first() {
            Some(Val::Int(n)) => u8::try_from(*n)
                .map(Val::Byte)
                .map_err(|_| (Trap::Overflow, span)),
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::Min => match (args.first(), args.get(1)) {
            (Some(Val::Int(a)), Some(Val::Int(b))) => Ok(Val::Int((*a).min(*b))),
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::Max => match (args.first(), args.get(1)) {
            (Some(Val::Int(a)), Some(Val::Int(b))) => Ok(Val::Int((*a).max(*b))),
            _ => Err((Trap::Overflow, span)),
        },
        Builtin::Abs => match args.first() {
            Some(Val::Int(a)) => a.checked_abs().map(Val::Int).ok_or((Trap::Overflow, span)),
            _ => Err((Trap::Overflow, span)),
        },
    }
}

fn eval_slice(args: &[Val], span: Span) -> Result<Val, (Trap, Span)> {
    let (Some(base), Some(Val::Int(start)), Some(Val::Int(end))) =
        (args.first(), args.get(1), args.get(2))
    else {
        return Err((Trap::IndexOutOfRange, span));
    };
    let s = usize::try_from(*start).map_err(|_| (Trap::IndexOutOfRange, span))?;
    let e = usize::try_from(*end).map_err(|_| (Trap::IndexOutOfRange, span))?;
    match base {
        Val::Str(text) => {
            let sub = text.get(s..e).ok_or((Trap::IndexOutOfRange, span))?;
            Ok(Val::Str(Rc::new(String::from(sub))))
        }
        Val::Array(arr) => {
            let borrow = arr.borrow();
            let slice = borrow.get(s..e).ok_or((Trap::IndexOutOfRange, span))?;
            Ok(Val::Array(Rc::new(RefCell::new(slice.to_vec()))))
        }
        _ => Err((Trap::IndexOutOfRange, span)),
    }
}
