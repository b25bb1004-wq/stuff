//! Layer (effect) verification, call checking, and full call-path diagnostics.
use super::lower::FnBuilder;
use super::names::{err, expr_span, resolve_builtin};
use super::{Env, ExprRes};
use crate::ast::{Block, Expr, FnBody, Item, Module, Stmt};
use crate::ir::{Builtin, FuncId, Inst, Reg, Ty};
use crate::span::{Diag, Span};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub(super) struct LayerGraph {
    pub direct_calls: Vec<Vec<FuncId>>,
    pub trans_layers: Vec<u64>,
}

impl LayerGraph {
    pub(super) fn build(env: &Env, m: &Module) -> Self {
        let n = env.funcs.len();
        let mut direct_calls = alloc::vec![Vec::new(); n];
        let mut callers = alloc::vec![Vec::new(); n];
        let mut trans_layers: Vec<u64> = env.funcs.iter().map(|f| f.layers).collect();
        let mut fn_idx = 0usize;
        for item in &m.items {
            if let Item::Fn(f) = item {
                if let FnBody::Block(b) = &f.body {
                    let mut calls = BTreeSet::new();
                    let mut locals: BTreeMap<String, usize> = BTreeMap::new();
                    for p in &f.params {
                        *locals.entry(p.name.name.clone()).or_insert(0) += 1;
                    }
                    scan_block(env, b, &mut locals, &mut calls, 0);
                    let list: Vec<FuncId> = calls.into_iter().collect();
                    for &callee in &list {
                        callers[callee as usize].push(fn_idx as FuncId);
                    }
                    direct_calls[fn_idx] = list;
                }
                fn_idx += 1;
            }
        }
        let mut queue = Vec::new();
        let mut in_queue = alloc::vec![false; n];
        for (i, &mask) in trans_layers.iter().enumerate() {
            if mask != 0 {
                queue.push(i as FuncId);
                in_queue[i] = true;
            }
        }
        let mut head = 0usize;
        while head < queue.len() {
            let v = queue[head] as usize;
            head += 1;
            in_queue[v] = false;
            let v_mask = trans_layers[v];
            for &u in &callers[v] {
                let ui = u as usize;
                let new_mask = trans_layers[ui] | v_mask;
                if new_mask != trans_layers[ui] {
                    trans_layers[ui] = new_mask;
                    if !in_queue[ui] {
                        in_queue[ui] = true;
                        queue.push(u);
                    }
                }
            }
        }
        Self {
            direct_calls,
            trans_layers,
        }
    }

    pub(super) fn check_direct_call(
        &self,
        caller: &FnBuilder<'_>,
        callee_id: FuncId,
        span: Span,
    ) -> Result<(), Diag> {
        let disallowed = self.trans_layers[callee_id as usize] & !caller.allowed_layers;
        if disallowed == 0 {
            return Ok(());
        }
        let bit = disallowed.trailing_zeros() as usize;
        let flag = 1u64 << bit;
        let lname = &caller.env.layers[bit];
        let mut path = self.walk_callers_up(caller.env, caller.caller_id, flag);
        path.push(caller.name.clone());
        path.push(caller.env.funcs[callee_id as usize].name.clone());

        let (mut cur, mut visited) = (callee_id, alloc::vec![callee_id]);
        if let Some(cid) = caller.caller_id {
            visited.push(cid);
        }
        while !caller.env.funcs[cur as usize].is_extern {
            let next = self.direct_calls[cur as usize]
                .iter()
                .copied()
                .find(|&c| !visited.contains(&c) && (self.trans_layers[c as usize] & flag) != 0);
            let Some(n) = next else { break };
            visited.push(n);
            path.push(caller.env.funcs[n as usize].name.clone());
            cur = n;
        }
        let chain = path.join(" -> ");
        err(
            span,
            format!("layer '{lname}' is not allowed here: {chain} (uses {lname})"),
        )
    }

    pub(super) fn check_indirect_call(
        &self,
        caller: &FnBuilder<'_>,
        label: &str,
        layers: u64,
        span: Span,
    ) -> Result<(), Diag> {
        let disallowed = layers & !caller.allowed_layers;
        if disallowed == 0 {
            return Ok(());
        }
        let bit = disallowed.trailing_zeros() as usize;
        let lname = &caller.env.layers[bit];
        let mut path = self.walk_callers_up(caller.env, caller.caller_id, 1u64 << bit);
        path.push(caller.name.clone());
        path.push(String::from(label));
        let chain = path.join(" -> ");
        err(
            span,
            format!("layer '{lname}' is not allowed here: {chain} (uses {lname})"),
        )
    }

    fn walk_callers_up(&self, env: &Env, caller_id: Option<FuncId>, flag: u64) -> Vec<String> {
        let mut path = Vec::new();
        if let Some(mut cur) = caller_id {
            let (mut up, mut visited) = (Vec::new(), alloc::vec![cur]);
            while let Some(p) = self.find_caller_lacking(env, cur, flag, &visited) {
                visited.push(p);
                up.push(env.funcs[p as usize].name.clone());
                cur = p;
            }
            path.extend(up.into_iter().rev());
        }
        path
    }

    fn find_caller_lacking(
        &self,
        env: &Env,
        tgt: FuncId,
        flag: u64,
        vis: &[FuncId],
    ) -> Option<FuncId> {
        if let Some(m) = env.main {
            let mi = m as usize;
            if !vis.contains(&m)
                && (env.funcs[mi].layers & flag) == 0
                && self.direct_calls[mi].contains(&tgt)
            {
                return Some(m);
            }
        }
        for (i, calls) in self.direct_calls.iter().enumerate() {
            let fid = i as FuncId;
            if !vis.contains(&fid) && (env.funcs[i].layers & flag) == 0 && calls.contains(&tgt) {
                return Some(fid);
            }
        }
        None
    }
}

impl FnBuilder<'_> {
    pub(super) fn check_call_expr(&mut self, callee: &Expr, args: &[Expr], span: Span) -> ExprRes {
        if let Expr::Name(id) = callee {
            if self.lookup_local(&id.name).is_none() {
                if let Some(f) = self.env.find_func(&id.name) {
                    let (fid, params, ret) = (f.id, f.params.clone(), f.ret.clone());
                    let (exp, got, fn_name) = (params.len(), args.len(), &id.name);
                    if got != exp {
                        return err(
                            span,
                            format!("function '{fn_name}' expects {exp} arguments, found {got}"),
                        );
                    }
                    let arg_regs = self.check_call_args(args, &params)?;
                    self.lg.check_direct_call(self, fid, span)?;
                    self.used_layers |= self.lg.trans_layers[fid as usize];
                    let dst = self.new_reg(ret.clone());
                    self.emit(Inst::Call(dst, fid, arg_regs), span);
                    return Ok((dst, ret, false));
                }
                if let Some(b) = resolve_builtin(&id.name) {
                    let (dst, ty) = self.check_builtin_call(b, args, span)?;
                    return Ok((dst, ty, false));
                }
                if self.env.find_const(&id.name).is_some() {
                    return err(id.span, format!("'{}' is not a function", id.name));
                }
                return err(id.span, format!("unknown function '{}'", id.name));
            }
        }
        let label = match callee {
            Expr::Name(id) => id.name.clone(),
            _ => String::from("<fn>"),
        };
        let (c_reg, c_ty, _) = self.check_expr(callee, None)?;
        let Ty::Fn {
            params,
            ret,
            layers,
        } = c_ty
        else {
            return err(expr_span(callee), "expression is not callable");
        };
        if args.len() != params.len() {
            return err(
                span,
                format!("expected {} arguments, found {}", params.len(), args.len()),
            );
        }
        let arg_regs = self.check_call_args(args, &params)?;
        self.lg.check_indirect_call(self, &label, layers, span)?;
        self.used_layers |= layers;
        let ret_ty = *ret;
        let dst = self.new_reg(ret_ty.clone());
        self.emit(Inst::CallValue(dst, c_reg, arg_regs), span);
        Ok((dst, ret_ty, false))
    }

    fn check_call_args(&mut self, args: &[Expr], params: &[Ty]) -> Result<Vec<Reg>, Diag> {
        let mut out = Vec::with_capacity(args.len());
        for (a, pty) in args.iter().zip(params.iter()) {
            let (r, t, _) = self.check_expr(a, Some(pty))?;
            out.push(self.coerce_if_needed(r, t, Some(pty), expr_span(a))?.0);
        }
        Ok(out)
    }

    fn check_builtin_call(
        &mut self,
        b: Builtin,
        args: &[Expr],
        span: Span,
    ) -> Result<(Reg, Ty), Diag> {
        let req = if matches!(b, Builtin::Push | Builtin::Min | Builtin::Max) {
            2
        } else {
            1
        };
        if args.len() != req {
            return err(
                span,
                format!("built-in expects {req} arguments, found {}", args.len()),
            );
        }
        let (r0, t0, _) = self.check_expr(&args[0], None)?;
        let (ret_ty, arg_regs) = match b {
            Builtin::Len if matches!(t0, Ty::Str | Ty::Array(_)) => (Ty::Int, alloc::vec![r0]),
            Builtin::Len => return err(span, "'len' requires 'str' or an array"),
            Builtin::Push => {
                let Ty::Array(elem_ty) = t0 else {
                    return err(span, "'push' requires an array");
                };
                let Expr::Name(id) = &args[0] else {
                    return err(
                        expr_span(&args[0]),
                        "first argument of 'push' must be a 'var' name",
                    );
                };
                if !self.lookup_local(&id.name).is_some_and(|v| v.mutable) {
                    return err(id.span, "first argument of 'push' must be a mutable 'var'");
                }
                let (r1, t1, _) = self.check_expr(&args[1], Some(&elem_ty))?;
                let (r1, _) = self.coerce_if_needed(r1, t1, Some(&elem_ty), expr_span(&args[1]))?;
                (Ty::Unit, alloc::vec![r0, r1])
            }
            Builtin::Pop => {
                let Ty::Array(elem_ty) = t0 else {
                    return err(span, "'pop' requires an array");
                };
                let Expr::Name(id) = &args[0] else {
                    return err(
                        expr_span(&args[0]),
                        "argument of 'pop' must be a 'var' name",
                    );
                };
                if !self.lookup_local(&id.name).is_some_and(|v| v.mutable) {
                    return err(id.span, "argument of 'pop' must be a mutable 'var'");
                }
                (Ty::Opt(elem_ty), alloc::vec![r0])
            }
            Builtin::IntToStr if t0 == Ty::Int => (Ty::Str, alloc::vec![r0]),
            Builtin::IntToStr => return err(span, "'str' requires 'int'"),
            Builtin::ParseInt if t0 == Ty::Str => (Ty::Opt(Box::new(Ty::Int)), alloc::vec![r0]),
            Builtin::ParseInt => return err(span, "'parse_int' requires 'str'"),
            Builtin::Bytes if t0 == Ty::Str => (Ty::Array(Box::new(Ty::Byte)), alloc::vec![r0]),
            Builtin::Bytes => return err(span, "'bytes' requires 'str'"),
            Builtin::Text if t0 == Ty::Array(Box::new(Ty::Byte)) => {
                (Ty::Opt(Box::new(Ty::Str)), alloc::vec![r0])
            }
            Builtin::Text => return err(span, "'text' requires '[byte]'"),
            Builtin::ToInt if t0 == Ty::Byte => (Ty::Int, alloc::vec![r0]),
            Builtin::ToInt => return err(span, "'int' requires 'byte'"),
            Builtin::ToByte if t0 == Ty::Int => (Ty::Byte, alloc::vec![r0]),
            Builtin::ToByte => return err(span, "'byte' requires 'int'"),
            Builtin::Min | Builtin::Max => {
                let (r1, t1, _) = self.check_expr(&args[1], Some(&Ty::Int))?;
                if t0 != Ty::Int || t1 != Ty::Int {
                    return err(span, "'min'/'max' require 'int' arguments");
                }
                (Ty::Int, alloc::vec![r0, r1])
            }
            Builtin::Abs if t0 == Ty::Int => (Ty::Int, alloc::vec![r0]),
            Builtin::Abs => return err(span, "'abs' requires 'int'"),
            Builtin::Concat | Builtin::Slice => return err(span, "internal builtin"),
        };
        let dst = self.new_reg(ret_ty.clone());
        self.emit(Inst::Builtin(dst, b, arg_regs), span);
        Ok((dst, ret_ty))
    }
}

pub(super) fn check_assignable(env: &Env, act: &Ty, exp: &Ty, span: Span) -> Result<(), Diag> {
    if act == exp {
        return Ok(());
    }
    if let (
        Ty::Fn {
            params: p1,
            ret: r1,
            layers: l1,
        },
        Ty::Fn {
            params: p2,
            ret: r2,
            layers: l2,
        },
    ) = (act, exp)
    {
        if p1 == p2 && r1 == r2 {
            let disallowed = *l1 & !*l2;
            if disallowed == 0 {
                return Ok(());
            }
            let lname = &env.layers[disallowed.trailing_zeros() as usize];
            return err(
                span,
                format!("layer '{lname}' is not allowed here: function value uses {lname}"),
            );
        }
    }
    err(
        span,
        format!("type mismatch: expected {exp:?}, found {act:?}"),
    )
}

fn push_local(locals: &mut BTreeMap<String, usize>, name: &str) {
    *locals.entry(String::from(name)).or_insert(0) += 1;
}

fn pop_local(locals: &mut BTreeMap<String, usize>, name: &str) {
    if let Some(count) = locals.get_mut(name) {
        if *count <= 1 {
            locals.remove(name);
        } else {
            *count -= 1;
        }
    }
}

fn scan_block(
    env: &Env,
    b: &Block,
    locals: &mut BTreeMap<String, usize>,
    calls: &mut BTreeSet<FuncId>,
    d: u32,
) {
    if d > super::MAX_DEPTH {
        return;
    }
    let mut added = Vec::new();
    for s in &b.stmts {
        match s {
            Stmt::Let { name, value, .. } => {
                scan_expr(env, value, locals, calls, d + 1);
                push_local(locals, &name.name);
                added.push(name.name.clone());
            }
            Stmt::Return { value: Some(e), .. } | Stmt::Expr { expr: e, .. } => {
                scan_expr(env, e, locals, calls, d + 1)
            }
            Stmt::Return { value: None, .. } | Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::While { cond, body, .. } => {
                scan_expr(env, cond, locals, calls, d + 1);
                scan_block(env, body, locals, calls, d + 1);
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                scan_expr(env, iter, locals, calls, d + 1);
                push_local(locals, &var.name);
                scan_block(env, body, locals, calls, d + 1);
                pop_local(locals, &var.name);
            }
            Stmt::Assert { cond, msg, .. } => {
                scan_expr(env, cond, locals, calls, d + 1);
                if let Some(m) = msg {
                    scan_expr(env, m, locals, calls, d + 1);
                }
            }
            Stmt::Assign { target, value, .. } => {
                scan_expr(env, target, locals, calls, d + 1);
                scan_expr(env, value, locals, calls, d + 1);
            }
        }
    }
    for name in added {
        pop_local(locals, &name);
    }
}

fn scan_expr(
    env: &Env,
    e: &Expr,
    locals: &mut BTreeMap<String, usize>,
    calls: &mut BTreeSet<FuncId>,
    d: u32,
) {
    if d > super::MAX_DEPTH {
        return;
    }
    let mut rec = |id: &crate::ast::Ident| {
        if !locals.contains_key(&id.name) {
            if let Some(f) = env.find_func(&id.name) {
                calls.insert(f.id);
            }
        }
    };
    match e {
        Expr::Call { callee, args, .. } => {
            if let Expr::Name(id) = callee.as_ref() {
                rec(id);
            }
            scan_expr(env, callee, locals, calls, d + 1);
            for a in args {
                scan_expr(env, a, locals, calls, d + 1);
            }
        }
        Expr::Pipe { lhs, rhs, .. } => {
            if let Expr::Name(id) = rhs.as_ref() {
                rec(id);
            }
            scan_expr(env, lhs, locals, calls, d + 1);
            scan_expr(env, rhs, locals, calls, d + 1);
        }
        Expr::Some { value: x, .. } | Expr::Unary { operand: x, .. } => {
            scan_expr(env, x, locals, calls, d + 1)
        }
        Expr::Field { base: x, .. } | Expr::TupleField { base: x, .. } => {
            scan_expr(env, x, locals, calls, d + 1)
        }
        Expr::Binary { lhs: a, rhs: b, .. }
        | Expr::Range {
            start: a, end: b, ..
        } => {
            scan_expr(env, a, locals, calls, d + 1);
            scan_expr(env, b, locals, calls, d + 1);
        }
        Expr::Index {
            base: a, index: b, ..
        }
        | Expr::Repeat {
            value: a, count: b, ..
        } => {
            scan_expr(env, a, locals, calls, d + 1);
            scan_expr(env, b, locals, calls, d + 1);
        }
        Expr::Variant { args: xs, .. }
        | Expr::List { items: xs, .. }
        | Expr::Tuple { items: xs, .. } => {
            for x in xs {
                scan_expr(env, x, locals, calls, d + 1);
            }
        }
        Expr::Record { fields, .. } => {
            for f in fields {
                scan_expr(env, &f.value, locals, calls, d + 1);
            }
        }
        Expr::If {
            cond, then, els, ..
        } => {
            scan_expr(env, cond, locals, calls, d + 1);
            scan_block(env, then, locals, calls, d + 1);
            if let Some(el) = els {
                scan_expr(env, el, locals, calls, d + 1);
            }
        }
        Expr::Match { subject, arms, .. } => {
            scan_expr(env, subject, locals, calls, d + 1);
            for arm in arms {
                scan_expr(env, &arm.body, locals, calls, d + 1);
            }
        }
        Expr::Block(b) => scan_block(env, b, locals, calls, d + 1),
        _ => {}
    }
}
