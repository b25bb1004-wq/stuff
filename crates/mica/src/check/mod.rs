//! Type checker, layer checker, and typed IR lowering for Mica modules.
use crate::ast::{BinOp, FnBody, FnDef, Item, Module, Param, Pattern, PropDef, TestDef};
use crate::ir::{
    Body, Func, FuncId, Inst, Lit, Program, Prop, Reg, Ty, TypeId, TypeInfo, TypeKind,
};
use crate::span::{Diag, Span};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

mod layers;
mod lower;
mod names;
mod types;

#[cfg(test)]
mod tests;

use layers::LayerGraph;
use lower::FnBuilder;
use names::{err, expr_span};

pub(super) const MAX_DEPTH: u32 = 200;
pub(super) type ExprRes = Result<(Reg, Ty, bool), Diag>;

#[derive(Clone)]
pub(super) struct VariantEntry {
    pub name: String,
    pub type_id: TypeId,
    pub tag: u32,
    pub fields: Vec<Ty>,
}

#[derive(Clone)]
pub(super) struct ConstEntry {
    pub name: String,
    pub ty: Ty,
    pub lit: Lit,
}

#[derive(Clone)]
pub(super) struct FuncEntry {
    pub name: String,
    pub id: FuncId,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub layers: u64,
    pub is_extern: bool,
}

pub(super) struct Env {
    pub layers: Vec<String>,
    pub types: Vec<TypeInfo>,
    pub type_names: Vec<(String, TypeId)>,
    pub variants: Vec<VariantEntry>,
    pub consts: Vec<ConstEntry>,
    pub funcs: Vec<FuncEntry>,
    pub main: Option<FuncId>,
}

impl Env {
    pub(super) fn find_type(&self, name: &str) -> Option<TypeId> {
        self.type_names
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }
    pub(super) fn find_variant(&self, name: &str) -> Option<&VariantEntry> {
        self.variants.iter().find(|v| v.name == name)
    }
    pub(super) fn find_const(&self, name: &str) -> Option<&ConstEntry> {
        self.consts.iter().find(|c| c.name == name)
    }
    pub(super) fn find_func(&self, name: &str) -> Option<&FuncEntry> {
        self.funcs.iter().find(|f| f.name == name)
    }
}

/// Checks a parsed module and lowers it to a typed [`Program`]. First error wins.
pub fn check(m: &Module) -> Result<Program, Diag> {
    let env = Env::collect(m)?;
    let lg = LayerGraph::build(&env, m);
    let mut funcs: Vec<Func> = env
        .funcs
        .iter()
        .map(|f| stub_func(f, String::new()))
        .collect();
    let (mut tests, mut props, mut fn_idx) = (Vec::new(), Vec::new(), 0usize);
    for item in &m.items {
        match item {
            Item::Fn(f) => {
                let fid = fn_idx as FuncId;
                fn_idx += 1;
                funcs[fid as usize] = check_fn_def(&env, &lg, f, fid)?;
            }
            Item::Test(t) => {
                let func = check_test_def(&env, &lg, t)?;
                let fid = funcs.len() as FuncId;
                funcs.push(func);
                tests.push((t.name.clone(), fid));
            }
            Item::Prop(p) => {
                let (guard_fn, body_fn) = check_prop_def(&env, &lg, p)?;
                let guard = guard_fn.map(|gf| {
                    let gid = funcs.len() as FuncId;
                    funcs.push(gf);
                    gid
                });
                let fid = funcs.len() as FuncId;
                funcs.push(body_fn);
                props.push(Prop {
                    name: p.name.clone(),
                    func: fid,
                    guard,
                });
            }
            Item::Type(_) | Item::Const(_) | Item::Layer(_) => {}
        }
    }
    let prog = Program {
        types: env.types,
        layers: env.layers,
        funcs,
        tests,
        props,
        main: env.main,
    };
    validate(&prog)?;
    Ok(prog)
}

fn stub_func(f: &FuncEntry, path: String) -> Func {
    let (name, params, ret, layers, regs) = (
        f.name.clone(),
        f.params.len() as u32,
        f.ret.clone(),
        f.layers,
        f.params.clone(),
    );
    Func {
        name,
        params,
        ret,
        layers,
        regs,
        body: Body::Extern(path),
    }
}

fn check_fn_def(env: &Env, lg: &LayerGraph, f: &FnDef, fid: FuncId) -> Result<Func, Diag> {
    let fe = &env.funcs[fid as usize];
    match &f.body {
        FnBody::Extern { path, .. } => Ok(stub_func(fe, path.clone())),
        FnBody::Block(b) => {
            let mut fb = FnBuilder::new(
                env,
                lg,
                fe.name.clone(),
                Some(fid),
                fe.ret.clone(),
                fe.layers,
            );
            bind_params(&mut fb, &f.params, &fe.params)?;
            let (mut r, rt, div) = fb.check_block(b, Some(&fe.ret))?;
            if !div {
                r = fb.coerce_if_needed(r, rt, Some(&fe.ret), b.span)?.0;
            }
            fb.emit(Inst::Ret(r), b.span);
            Ok(fb.finish(fe.params.len() as u32, fe.layers))
        }
    }
}

fn check_test_def(env: &Env, lg: &LayerGraph, t: &TestDef) -> Result<Func, Diag> {
    let mut b = FnBuilder::new(env, lg, t.name.clone(), None, Ty::Unit, u64::MAX);
    let (mut r, rt, div) = b.check_block(&t.body, Some(&Ty::Unit))?;
    if !div {
        r = b.coerce_if_needed(r, rt, Some(&Ty::Unit), t.span)?.0;
    }
    b.emit(Inst::Ret(r), t.span);
    let used = b.used_layers;
    Ok(b.finish(0, used))
}

fn check_prop_def(env: &Env, lg: &LayerGraph, p: &PropDef) -> Result<(Option<Func>, Func), Diag> {
    let (mut seen, mut p_tys) = (Vec::new(), Vec::with_capacity(p.params.len()));
    for param in &p.params {
        if param.name.name != "_" && seen.contains(&param.name.name.as_str()) {
            return err(
                param.name.span,
                format!("duplicate parameter '{}'", param.name.name),
            );
        }
        seen.push(param.name.name.as_str());
        p_tys.push(env.resolve_ty(&param.ty, 0)?);
    }
    let guard_fn = if let Some(cond) = &p.cond {
        let mut gb = FnBuilder::new(env, lg, format!("{}#guard", p.name), None, Ty::Bool, 0);
        bind_params(&mut gb, &p.params, &p_tys)?;
        let c_reg = gb.expect_ty(
            cond,
            &Ty::Bool,
            expr_span(cond),
            "'where' guard must have type 'bool'",
        )?;
        gb.emit(Inst::Ret(c_reg), expr_span(cond));
        Some(gb.finish(p_tys.len() as u32, 0))
    } else {
        None
    };
    let mut bb = FnBuilder::new(env, lg, p.name.clone(), None, Ty::Unit, 0);
    bind_params(&mut bb, &p.params, &p_tys)?;
    let (mut r, rt, div) = bb.check_block(&p.body, Some(&Ty::Unit))?;
    if !div {
        r = bb.coerce_if_needed(r, rt, Some(&Ty::Unit), p.span)?.0;
    }
    bb.emit(Inst::Ret(r), p.span);
    Ok((guard_fn, bb.finish(p_tys.len() as u32, 0)))
}

fn bind_params(b: &mut FnBuilder<'_>, params: &[Param], tys: &[Ty]) -> Result<(), Diag> {
    for (p, ty) in params.iter().zip(tys.iter()) {
        let r = b.new_reg(ty.clone());
        b.define_local(&p.name, r, ty.clone(), false)?;
    }
    Ok(())
}

pub(super) fn is_eq_type(ty: &Ty) -> bool {
    match ty {
        Ty::Int | Ty::Byte | Ty::Bool | Ty::Str | Ty::Unit => true,
        Ty::Tuple(items) => items.iter().all(is_eq_type),
        Ty::Opt(inner) => is_eq_type(inner),
        Ty::Array(_) | Ty::Named(_) | Ty::Fn { .. } => false,
    }
}

pub(super) fn check_pattern_ty(
    env: &Env,
    pat: &Pattern,
    ty: &Ty,
    seen: &mut Vec<String>,
    d: u32,
) -> Result<(), Diag> {
    if d > MAX_DEPTH {
        return err(Span { start: 0, end: 0 }, "pattern nesting too deep");
    }
    match pat {
        Pattern::Wild { .. } => Ok(()),
        Pattern::Bind(id) if seen.contains(&id.name) => err(
            id.span,
            format!("duplicate binding '{}' in pattern", id.name),
        ),
        Pattern::Bind(id) => {
            seen.push(id.name.clone());
            Ok(())
        }
        Pattern::Int { .. } if *ty == Ty::Int => Ok(()),
        Pattern::Byte { .. } if *ty == Ty::Byte => Ok(()),
        Pattern::Str { .. } if *ty == Ty::Str => Ok(()),
        Pattern::Bool { .. } if *ty == Ty::Bool => Ok(()),
        Pattern::Int { span, .. } | Pattern::Byte { span, .. } => {
            err(*span, format!("pattern type mismatch for {ty:?}"))
        }
        Pattern::Str { span, .. } | Pattern::Bool { span, .. } => {
            err(*span, format!("pattern type mismatch for {ty:?}"))
        }
        Pattern::None { .. } if matches!(ty, Ty::Opt(_)) => Ok(()),
        Pattern::None { span } => err(*span, "pattern 'none' requires an option type"),
        Pattern::Some { inner, .. } if matches!(ty, Ty::Opt(_)) => {
            let Ty::Opt(inner_ty) = ty else {
                unreachable!()
            };
            check_pattern_ty(env, inner, inner_ty, seen, d + 1)
        }
        Pattern::Some { span, .. } => err(*span, "pattern 'some' requires an option type"),
        Pattern::Variant { name, args, span } => {
            let Some(ve) = env.find_variant(&name.name) else {
                return err(name.span, format!("unknown variant '{}'", name.name));
            };
            if *ty != Ty::Named(ve.type_id) || args.len() != ve.fields.len() {
                return err(*span, format!("variant pattern '{}' mismatch", name.name));
            }
            for (p, fty) in args.iter().zip(ve.fields.iter()) {
                check_pattern_ty(env, p, fty, seen, d + 1)?;
            }
            Ok(())
        }
        Pattern::Tuple { items, span } => {
            let Ty::Tuple(elem_tys) = ty else {
                return err(*span, "tuple pattern requires a tuple type");
            };
            if items.len() != elem_tys.len() {
                return err(*span, "tuple pattern arity mismatch");
            }
            for (p, ety) in items.iter().zip(elem_tys.iter()) {
                check_pattern_ty(env, p, ety, seen, d + 1)?;
            }
            Ok(())
        }
    }
}

const WILD_PAT: Pattern = Pattern::Wild {
    span: Span { start: 0, end: 0 },
};

pub(super) fn is_exhaustive(types: &[TypeInfo], ty: &Ty, pats: &[&Pattern], depth: u32) -> bool {
    let rows: Vec<Vec<&Pattern>> = pats.iter().map(|&p| alloc::vec![p]).collect();
    is_exhaustive_rows(types, core::slice::from_ref(ty), &rows, depth)
}

fn is_wild(p: &Pattern) -> bool {
    matches!(p, Pattern::Wild { .. } | Pattern::Bind(_))
}

fn expand<'a>(mut prefix: Vec<&'a Pattern>, rest: &[&'a Pattern]) -> Vec<&'a Pattern> {
    prefix.extend_from_slice(rest);
    prefix
}

fn is_exhaustive_rows(types: &[TypeInfo], tys: &[Ty], rows: &[Vec<&Pattern>], d: u32) -> bool {
    if rows.is_empty() || d > 64 {
        return false;
    }
    let Some((first_ty, rest_tys)) = tys.split_first() else {
        return true;
    };
    if rows.iter().all(|r| is_wild(r[0])) {
        let next: Vec<Vec<&Pattern>> = rows.iter().map(|r| r[1..].to_vec()).collect();
        return is_exhaustive_rows(types, rest_tys, &next, d + 1);
    }
    match first_ty {
        Ty::Bool => [true, false].into_iter().all(|b| {
            let sub: Vec<Vec<&Pattern>> = rows
                .iter()
                .filter(|r| {
                    is_wild(r[0]) || matches!(r[0], Pattern::Bool { value, .. } if *value == b)
                })
                .map(|r| r[1..].to_vec())
                .collect();
            is_exhaustive_rows(types, rest_tys, &sub, d + 1)
        }),
        Ty::Opt(inner) => {
            let n_rows: Vec<Vec<&Pattern>> = rows
                .iter()
                .filter(|r| is_wild(r[0]) || matches!(r[0], Pattern::None { .. }))
                .map(|r| r[1..].to_vec())
                .collect();
            let s_tys = [core::slice::from_ref(inner.as_ref()), rest_tys].concat();
            let s_rows: Vec<Vec<&Pattern>> = rows
                .iter()
                .filter_map(|r| match r[0] {
                    p if is_wild(p) => Some(expand(alloc::vec![&WILD_PAT], &r[1..])),
                    Pattern::Some { inner: s, .. } => {
                        Some(expand(alloc::vec![s.as_ref()], &r[1..]))
                    }
                    _ => None,
                })
                .collect();
            is_exhaustive_rows(types, rest_tys, &n_rows, d + 1)
                && is_exhaustive_rows(types, &s_tys, &s_rows, d + 1)
        }
        Ty::Named(tid) => match &types[*tid as usize].kind {
            TypeKind::Variants(vars) => vars.iter().all(|(vname, vfields)| {
                let vtys = [vfields.as_slice(), rest_tys].concat();
                let vrows: Vec<Vec<&Pattern>> = rows
                    .iter()
                    .filter_map(|r| match r[0] {
                        p if is_wild(p) => {
                            Some(expand(alloc::vec![&WILD_PAT; vfields.len()], &r[1..]))
                        }
                        Pattern::Variant { name, args, .. } if &name.name == vname => {
                            Some(expand(args.iter().collect(), &r[1..]))
                        }
                        _ => None,
                    })
                    .collect();
                is_exhaustive_rows(types, &vtys, &vrows, d + 1)
            }),
            TypeKind::Record(_) => false,
        },
        Ty::Tuple(elem_tys) => {
            let ttys = [elem_tys.as_slice(), rest_tys].concat();
            let trows: Vec<Vec<&Pattern>> = rows
                .iter()
                .filter_map(|r| match r[0] {
                    p if is_wild(p) => {
                        Some(expand(alloc::vec![&WILD_PAT; elem_tys.len()], &r[1..]))
                    }
                    Pattern::Tuple { items, .. } => Some(expand(items.iter().collect(), &r[1..])),
                    _ => None,
                })
                .collect();
            is_exhaustive_rows(types, &ttys, &trows, d + 1)
        }
        _ => {
            let w: Vec<Vec<&Pattern>> = rows
                .iter()
                .filter(|r| is_wild(r[0]))
                .map(|r| r[1..].to_vec())
                .collect();
            is_exhaustive_rows(types, rest_tys, &w, d + 1)
        }
    }
}

/// Validates structural invariants of a lowered [`Program`].
pub(crate) fn validate(p: &Program) -> Result<(), Diag> {
    let dummy = Span { start: 0, end: 0 };
    if p.layers.len() < 7 || p.layers.len() > 64 {
        return err(dummy, "invalid layer table size");
    }
    for func in &p.funcs {
        if func.params as usize > func.regs.len() {
            return err(dummy, "func params exceed regs");
        }
        if let Body::Code { insts, spans } = &func.body {
            if insts.len() != spans.len()
                || !matches!(insts.last(), Some(Inst::Ret(_) | Inst::Jump(_)))
            {
                return err(dummy, "invalid code body terminator or spans");
            }
            let (mut defined, mut used, nr) = (Vec::new(), Vec::new(), func.regs.len() as u32);
            for inst in insts {
                if !valid_inst_regs(inst, &func.regs, nr) {
                    return err(dummy, "invalid register or binop in IR");
                }
                match inst {
                    Inst::Label(l) if defined.contains(l) => return err(dummy, "duplicate label"),
                    Inst::Label(l) => defined.push(*l),
                    Inst::Jump(l) => used.push(*l),
                    Inst::Branch(_, l1, l2) => used.extend_from_slice(&[*l1, *l2]),
                    _ => {}
                }
            }
            if !used.iter().all(|l| defined.contains(l)) {
                return err(dummy, "undefined label used in jump or branch");
            }
        }
    }
    Ok(())
}

fn valid_inst_regs(inst: &Inst, regs: &[Ty], nr: u32) -> bool {
    match inst {
        Inst::Lit(d, _) | Inst::NewNone(d) | Inst::Ret(d) => *d < nr,
        Inst::Assert(d, _) | Inst::Branch(d, _, _) => *d < nr,
        Inst::Move(d, a) | Inst::Un(d, _, a) | Inst::NewSome(d, a) | Inst::Tag(d, a) => {
            *d < nr && *a < nr
        }
        Inst::GetField(d, a, _) | Inst::SetField(d, _, a) => *d < nr && *a < nr,
        Inst::Bin(d, op, a, b) => {
            let prim = matches!(
                regs.get(*a as usize),
                Some(Ty::Int | Ty::Byte | Ty::Bool | Ty::Unit | Ty::Str)
            );
            let eq_ok = !matches!(op, BinOp::Eq | BinOp::Ne) || prim;
            *d < nr && *a < nr && *b < nr && !matches!(op, BinOp::And | BinOp::Or) && eq_ok
        }
        Inst::NewFill(d, a, b) | Inst::GetIndex(d, a, b) | Inst::SetIndex(d, a, b) => {
            *d < nr && *a < nr && *b < nr
        }
        Inst::Call(d, _, xs) | Inst::Builtin(d, _, xs) => *d < nr && xs.iter().all(|&r| r < nr),
        Inst::NewArray(d, xs) | Inst::NewTuple(d, xs) => *d < nr && xs.iter().all(|&r| r < nr),
        Inst::NewRecord(d, _, xs) | Inst::NewVariant(d, _, _, xs) => {
            *d < nr && xs.iter().all(|&r| r < nr)
        }
        Inst::CallValue(d, f, xs) => *d < nr && *f < nr && xs.iter().all(|&r| r < nr),
        Inst::Label(_) | Inst::Jump(_) => true,
    }
}
