//! Lowering from checked `logos::ir::Program` to `elysium::Module`.
use crate::debug::{func_debug, Debug, FuncDebug, LocalVar};
use crate::meta::encode_meta;
use crate::{Compiled, CompiledProp};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use elysium::{Const, Func, Module, Op};
use logos::ast::{BinOp, UnOp};
use logos::ir::{self, Body, Builtin, Inst, Lit, Reg, Ty};
use logos::Span;

/// Compiles a checked Logos IR program into a Elysium module and metadata.
pub fn compile(p: &ir::Program) -> Compiled {
    let mut cx = ModCx {
        p,
        consts: Vec::new(),
        imports: Vec::new(),
        used_names: Vec::new(),
        ext_span: find_call_value_span(p),
    };
    let mut funcs = Vec::with_capacity(p.funcs.len().max(1));
    let mut spans = Vec::with_capacity(p.funcs.len().max(1));

    let mut functions = Vec::with_capacity(p.funcs.len().max(1));

    for (fid, f) in p.funcs.iter().enumerate() {
        let (gf, f_spans, locals) = cx.compile_func(fid as u32, f);
        functions.push(FuncDebug {
            locals,
            ..func_debug(&gf)
        });
        funcs.push(gf);
        spans.push(f_spans);
    }

    if funcs.is_empty() {
        funcs.push(Func {
            name: String::from("__empty"),
            params: 0,
            regs: 1,
            code: vec![Op::Unit(0), Op::Ret(0)],
        });
        spans.push(vec![Span { start: 0, end: 0 }; 2]);
        functions.push(FuncDebug {
            name: String::from("__empty"),
            locals: Vec::new(),
        });
    }

    let debug = Debug {
        line_starts: Vec::new(),
        functions,
    };

    if !p.props.is_empty() {
        cx.consts.push(Const::Bytes(encode_meta(p)));
    }

    let props = p
        .props
        .iter()
        .map(|pr| {
            let params = p
                .funcs
                .get(pr.func as usize)
                .and_then(|f| f.regs.get(..f.params as usize))
                .map(|s| s.to_vec())
                .unwrap_or_default();
            CompiledProp {
                name: pr.name.clone(),
                guard: pr.guard,
                body: pr.func,
                params,
            }
        })
        .collect();

    Compiled {
        module: Module {
            consts: cx.consts,
            imports: cx.imports,
            funcs,
            entry: p.main.unwrap_or(0),
        },
        main: p.main,
        tests: p.tests.clone(),
        props,
        spans,
        debug,
    }
}

fn find_call_value_span(p: &ir::Program) -> Span {
    for f in &p.funcs {
        if let Body::Code { insts, spans } = &f.body {
            for (inst, sp) in insts.iter().zip(spans.iter()) {
                if matches!(inst, Inst::CallValue(..)) {
                    return *sp;
                }
            }
        }
    }
    Span { start: 0, end: 0 }
}

struct ModCx<'a> {
    p: &'a ir::Program,
    consts: Vec<Const>,
    imports: Vec<String>,
    used_names: Vec<String>,
    ext_span: Span,
}

impl ModCx<'_> {
    fn intern_str(&mut self, s: &str) -> u32 {
        if let Some(idx) = self
            .consts
            .iter()
            .position(|c| matches!(c, Const::Str(x) if x == s))
        {
            return idx as u32;
        }
        let idx = self.consts.len() as u32;
        self.consts.push(Const::Str(String::from(s)));
        idx
    }

    fn intern_import(&mut self, name: &str) -> u32 {
        if let Some(idx) = self.imports.iter().position(|x| x == name) {
            return idx as u32;
        }
        let idx = self.imports.len() as u32;
        self.imports.push(String::from(name));
        idx
    }

    fn unique_func_name(&mut self, fid: u32, raw: &str) -> String {
        let trimmed = trim_utf8(raw, 200);
        let name = if !trimmed.is_empty() && !self.used_names.iter().any(|n| n == trimmed) {
            String::from(trimmed)
        } else {
            format!("{trimmed}${fid}")
        };
        self.used_names.push(name.clone());
        name
    }

    fn compile_func(&mut self, fid: u32, f: &ir::Func) -> (Func, Vec<Span>, Vec<LocalVar>) {
        let name = self.unique_func_name(fid, &f.name);
        let params = r16(f.params);
        match &f.body {
            Body::Extern(ext_name) => {
                let h = self.intern_import(ext_name);
                let regs = params.max(1);
                let code = vec![Op::CallHost(0, h, 0, params), Op::Ret(0)];
                let sp = vec![self.ext_span, self.ext_span];
                (
                    Func {
                        name,
                        params,
                        regs,
                        code,
                    },
                    sp,
                    Vec::new(),
                )
            }
            Body::Code { insts, spans } => {
                let base_regs = u16::try_from(f.regs.len()).unwrap_or(0);
                let max_scratch = insts.iter().map(scratch_for_inst).max().unwrap_or(0);
                let regs = base_regs.saturating_add(max_scratch).max(params).max(1);
                let mut fcx = FnCx {
                    mcx: self,
                    reg_tys: &f.regs,
                    base_regs,
                    code: Vec::with_capacity(insts.len() + 8),
                    spans: Vec::with_capacity(insts.len() + 8),
                    labels: Vec::new(),
                    fixups: Vec::new(),
                };
                // op_start[i] is the first op of IR instruction i; the last entry is the body end.
                let mut op_start = Vec::with_capacity(insts.len() + 1);
                for (i, (inst, &sp)) in insts.iter().zip(spans.iter()).enumerate() {
                    op_start.push(op_index(fcx.code.len()));
                    fcx.lower_inst(inst, sp, insts.get(i + 1));
                }
                let body_end = op_index(fcx.code.len());
                op_start.push(body_end);
                let at = |pc: u32| {
                    usize::try_from(pc)
                        .ok()
                        .and_then(|i| op_start.get(i))
                        .copied()
                        .unwrap_or(body_end)
                };
                let locals = f
                    .bindings
                    .iter()
                    .map(|b| LocalVar {
                        reg: b.reg,
                        name: b.name.clone(),
                        from_pc: at(b.from),
                        to_pc: at(b.to),
                    })
                    .collect();
                if !matches!(
                    fcx.code.last(),
                    Some(Op::Ret(_) | Op::Jump(_) | Op::Trap(_))
                ) {
                    let last_sp = spans.last().copied().unwrap_or(Span { start: 0, end: 0 });
                    fcx.emit(Op::Unit(0), last_sp);
                    fcx.emit(Op::Ret(0), last_sp);
                }
                fcx.patch_jumps();
                let func = Func {
                    name,
                    params,
                    regs,
                    code: fcx.code,
                };
                (func, fcx.spans, locals)
            }
        }
    }
}

struct FnCx<'a, 'b> {
    mcx: &'a mut ModCx<'b>,
    reg_tys: &'a [Ty],
    base_regs: u16,
    code: Vec<Op>,
    spans: Vec<Span>,
    labels: Vec<u32>,
    fixups: Vec<(usize, u32)>,
}

impl FnCx<'_, '_> {
    fn emit(&mut self, op: Op, sp: Span) {
        self.code.push(op);
        self.spans.push(sp);
    }

    fn emit_jump(&mut self, op: Op, lbl: u32, sp: Span) {
        let at = self.code.len();
        self.fixups.push((at, lbl));
        self.emit(op, sp);
    }

    fn stage(&mut self, args: &[Reg], sp: Span) -> (u16, u16) {
        let n = u16::try_from(args.len()).unwrap_or(0);
        if n == 0 {
            return (0, 0);
        }
        if n == 1 {
            return (r16(args[0]), 1);
        }
        if args.windows(2).all(|w| w[1] == w[0].saturating_add(1)) {
            return (r16(args[0]), n);
        }
        for (i, &r) in args.iter().enumerate() {
            self.emit(Op::Move(self.base_regs + i as u16, r16(r)), sp);
        }
        (self.base_regs, n)
    }

    fn lower_inst(&mut self, inst: &Inst, sp: Span, next: Option<&Inst>) {
        match inst {
            Inst::Lit(d, lit) => {
                let op = match lit {
                    Lit::Int(v) => Op::Int(r16(*d), *v),
                    Lit::Byte(v) => Op::Byte(r16(*d), *v),
                    Lit::Bool(v) => Op::Bool(r16(*d), *v),
                    Lit::Unit => Op::Unit(r16(*d)),
                    Lit::Str(s) => Op::Const(r16(*d), self.mcx.intern_str(s)),
                    Lit::Fn(fid) => Op::FnRef(r16(*d), *fid),
                };
                self.emit(op, sp);
            }
            Inst::Move(d, a) => self.emit(Op::Move(r16(*d), r16(*a)), sp),
            Inst::Un(d, UnOp::Neg, a) => self.emit(Op::Neg(r16(*d), r16(*a)), sp),
            Inst::Un(d, UnOp::Not, a) => self.emit(Op::Not(r16(*d), r16(*a)), sp),
            Inst::Bin(d, op, a, b) => self.emit(lower_bin(*op, r16(*d), r16(*a), r16(*b)), sp),
            Inst::Call(d, callee, args) => {
                let (first, n) = self.stage(args, sp);
                let ext = self
                    .mcx
                    .p
                    .funcs
                    .get(*callee as usize)
                    .and_then(|f| match &f.body {
                        Body::Extern(name) => Some(name.as_str()),
                        Body::Code { .. } => None,
                    });
                if let Some(ext_name) = ext {
                    let h = self.mcx.intern_import(ext_name);
                    self.emit(Op::CallHost(r16(*d), h, first, n), sp);
                } else {
                    self.emit(Op::Call(r16(*d), *callee, first, n), sp);
                }
            }
            Inst::CallValue(d, callee, args) => {
                let (first, n) = self.stage(args, sp);
                self.emit(Op::CallValue(r16(*d), r16(*callee), first, n), sp);
            }
            Inst::Builtin(d, b, args) => self.lower_builtin(r16(*d), *b, args, sp),
            Inst::NewArray(d, elems) => {
                let (first, n) = self.stage(elems, sp);
                let is_bytes = matches!(
                    self.reg_tys.get(*d as usize),
                    Some(Ty::Array(el)) if **el == Ty::Byte
                );
                if is_bytes {
                    self.emit(Op::NewBytes(r16(*d), first, n), sp);
                } else {
                    self.emit(Op::NewArray(r16(*d), first, n), sp);
                }
            }
            Inst::NewFill(d, val, cnt) => {
                self.emit(Op::NewFill(r16(*d), r16(*val), r16(*cnt)), sp);
            }
            Inst::NewTuple(d, elems) | Inst::NewRecord(d, _, elems) => {
                let (first, n) = self.stage(elems, sp);
                self.emit(Op::NewRecord(r16(*d), first, n), sp);
            }
            Inst::NewVariant(d, _, tag, fields) => {
                let (first, n) = self.stage(fields, sp);
                self.emit(Op::NewVariant(r16(*d), *tag, first, n), sp);
            }
            Inst::NewSome(d, src) => self.emit(Op::NewVariant(r16(*d), 1, r16(*src), 1), sp),
            Inst::NewNone(d) => self.emit(Op::NewVariant(r16(*d), 0, 0, 0), sp),
            Inst::GetField(d, base, idx) => {
                self.emit(Op::GetField(r16(*d), r16(*base), r16(*idx)), sp);
            }
            Inst::SetField(base, idx, val) => {
                self.emit(Op::SetField(r16(*base), r16(*idx), r16(*val)), sp);
            }
            Inst::Tag(d, src) => self.emit(Op::Tag(r16(*d), r16(*src)), sp),
            Inst::GetIndex(d, base, idx) => {
                self.emit(Op::GetIndex(r16(*d), r16(*base), r16(*idx)), sp);
            }
            Inst::SetIndex(base, idx, val) => {
                self.emit(Op::SetIndex(r16(*base), r16(*idx), r16(*val)), sp);
            }
            Inst::Label(lbl) => {
                let l = *lbl as usize;
                if self.labels.len() <= l {
                    self.labels.resize(l + 1, 0);
                }
                self.labels[l] = self.code.len() as u32;
            }
            Inst::Jump(lbl) => self.emit_jump(Op::Jump(0), *lbl, sp),
            Inst::Branch(cond, t_lbl, f_lbl) => {
                let c = r16(*cond);
                if next == Some(&Inst::Label(*t_lbl)) {
                    self.emit_jump(Op::JumpIfNot(c, 0), *f_lbl, sp);
                } else if next == Some(&Inst::Label(*f_lbl)) {
                    self.emit_jump(Op::JumpIf(c, 0), *t_lbl, sp);
                } else {
                    self.emit_jump(Op::JumpIf(c, 0), *t_lbl, sp);
                    self.emit_jump(Op::Jump(0), *f_lbl, sp);
                }
            }
            Inst::Ret(r) => self.emit(Op::Ret(r16(*r)), sp),
            Inst::Assert(cond, msg) => {
                let k = self.mcx.intern_str(msg);
                self.emit(Op::Assert(r16(*cond), k), sp);
            }
        }
    }

    fn lower_builtin(&mut self, d: u16, b: Builtin, args: &[Reg], sp: Span) {
        let a0 = args.first().copied().map_or(0, r16);
        let a1 = args.get(1).copied().map_or(0, r16);
        let a2 = args.get(2).copied().map_or(0, r16);
        match b {
            Builtin::Len => self.emit(Op::Len(d, a0), sp),
            Builtin::Push => {
                self.emit(Op::Push(a0, a1), sp);
                self.emit(Op::Unit(d), sp);
            }
            Builtin::Pop => self.emit(Op::Pop(d, a0), sp),
            Builtin::Slice => self.emit(Op::Slice(d, a0, a1, a2), sp),
            Builtin::Concat => self.emit(Op::Concat(d, a0, a1), sp),
            Builtin::IntToStr => self.emit(Op::IntToStr(d, a0), sp),
            Builtin::ParseInt => self.emit(Op::ParseInt(d, a0), sp),
            Builtin::Bytes => self.emit(Op::StrToBytes(d, a0), sp),
            Builtin::Text => self.emit(Op::BytesToStr(d, a0), sp),
            Builtin::ToInt => self.emit(Op::ToInt(d, a0), sp),
            Builtin::ToByte => self.emit(Op::ToByte(d, a0), sp),
            Builtin::Min | Builtin::Max => {
                let sc = self.base_regs;
                let base = self.code.len() as u32;
                if b == Builtin::Min {
                    self.emit(Op::Lt(sc, a0, a1), sp);
                } else {
                    self.emit(Op::Gt(sc, a0, a1), sp);
                }
                self.emit(Op::JumpIf(sc, base + 4), sp);
                self.emit(Op::Move(d, a1), sp);
                self.emit(Op::Jump(base + 5), sp);
                self.emit(Op::Move(d, a0), sp);
            }
            Builtin::Abs => {
                let (sc0, sc1) = (self.base_regs, self.base_regs + 1);
                let base = self.code.len() as u32;
                self.emit(Op::Int(sc0, 0), sp);
                self.emit(Op::Lt(sc1, a0, sc0), sp);
                self.emit(Op::JumpIf(sc1, base + 5), sp);
                self.emit(Op::Move(d, a0), sp);
                self.emit(Op::Jump(base + 6), sp);
                self.emit(Op::Neg(d, a0), sp);
            }
        }
    }

    fn patch_jumps(&mut self) {
        for &(at, lbl) in &self.fixups {
            let target = self.labels.get(lbl as usize).copied().unwrap_or(0);
            if let Some(Op::Jump(t) | Op::JumpIf(_, t) | Op::JumpIfNot(_, t)) =
                self.code.get_mut(at)
            {
                *t = target;
            }
        }
    }
}

fn lower_bin(op: BinOp, d: u16, a: u16, b: u16) -> Op {
    match op {
        BinOp::Add => Op::Add(d, a, b),
        BinOp::Sub => Op::Sub(d, a, b),
        BinOp::Mul => Op::Mul(d, a, b),
        BinOp::AddWrap => Op::AddW(d, a, b),
        BinOp::SubWrap => Op::SubW(d, a, b),
        BinOp::MulWrap => Op::MulW(d, a, b),
        BinOp::Div => Op::Div(d, a, b),
        BinOp::Rem => Op::Rem(d, a, b),
        BinOp::BitAnd | BinOp::And => Op::And(d, a, b),
        BinOp::BitOr | BinOp::Or => Op::Or(d, a, b),
        BinOp::BitXor => Op::Xor(d, a, b),
        BinOp::Shl => Op::Shl(d, a, b),
        BinOp::Shr => Op::Shr(d, a, b),
        BinOp::Eq => Op::Eq(d, a, b),
        BinOp::Ne => Op::Ne(d, a, b),
        BinOp::Lt => Op::Lt(d, a, b),
        BinOp::Le => Op::Le(d, a, b),
        BinOp::Gt => Op::Gt(d, a, b),
        BinOp::Ge => Op::Ge(d, a, b),
    }
}

fn r16(r: u32) -> u16 {
    u16::try_from(r).unwrap_or(0)
}

/// Op index as the `u32` used by debug info; saturates instead of wrapping.
fn op_index(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn scratch_for_inst(inst: &Inst) -> u16 {
    match inst {
        Inst::Call(_, _, xs)
        | Inst::CallValue(_, _, xs)
        | Inst::NewArray(_, xs)
        | Inst::NewTuple(_, xs)
        | Inst::NewRecord(_, _, xs)
        | Inst::NewVariant(_, _, _, xs) => {
            if xs.len() > 1 {
                u16::try_from(xs.len()).unwrap_or(0)
            } else {
                0
            }
        }
        Inst::Builtin(_, Builtin::Abs, _) => 2,
        Inst::Builtin(_, Builtin::Min | Builtin::Max, _) => 1,
        _ => 0,
    }
}

fn trim_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
