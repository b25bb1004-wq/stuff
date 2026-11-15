//! IR lowering for functions, control flow, patterns, loops, and assignments.
use super::layers::{check_assignable, LayerGraph};
use super::names::{err, expr_span};
use super::{check_pattern_ty, is_exhaustive, Env, ExprRes, MAX_DEPTH};
use crate::ast::{Arm, BinOp, Block, Expr, Ident, Pattern, UnOp};
use crate::ir::{Binding, Body, Builtin, Func, FuncId, Inst, Label, Lit, Reg, Ty, TypeKind};
use crate::span::{Diag, Span};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone)]
pub(super) struct LocalVar {
    pub reg: Reg,
    pub ty: Ty,
    pub mutable: bool,
}

pub(super) struct FnBuilder<'a> {
    pub env: &'a Env,
    pub lg: &'a LayerGraph,
    pub name: String,
    pub caller_id: Option<FuncId>,
    pub ret_ty: Ty,
    pub allowed_layers: u64,
    pub used_layers: u64,
    pub regs: Vec<Ty>,
    pub insts: Vec<Inst>,
    pub spans: Vec<Span>,
    pub next_label: Label,
    pub scopes: Vec<Vec<String>>,
    pub local_map: BTreeMap<String, Vec<(usize, LocalVar)>>,
    pub loops: Vec<(Label, Label)>,
    pub depth: u32,
    /// Recorded `let`/`var` bindings, moved into `Func::bindings` by `finish`.
    pub bindings: Vec<Binding>,
    /// Bindings whose scope has not ended yet: `(scope depth, index in bindings)`, innermost last.
    open_bindings: Vec<(usize, usize)>,
}

impl<'a> FnBuilder<'a> {
    pub(super) fn new(
        env: &'a Env,
        lg: &'a LayerGraph,
        name: String,
        caller_id: Option<FuncId>,
        ret_ty: Ty,
        allowed_layers: u64,
    ) -> Self {
        Self {
            env,
            lg,
            name,
            caller_id,
            ret_ty,
            allowed_layers,
            used_layers: 0,
            regs: Vec::new(),
            insts: Vec::new(),
            spans: Vec::new(),
            next_label: 0,
            scopes: alloc::vec![Vec::new()],
            local_map: BTreeMap::new(),
            loops: Vec::new(),
            depth: 0,
            bindings: Vec::new(),
            open_bindings: Vec::new(),
        }
    }

    pub(super) fn finish(mut self, params: u32, layers: u64) -> Func {
        self.close_bindings(0);
        let body = Body::Code {
            insts: self.insts,
            spans: self.spans,
        };
        Func {
            name: self.name,
            params,
            ret: self.ret_ty,
            layers,
            regs: self.regs,
            bindings: self.bindings,
            body,
        }
    }

    /// Keeps bindings whose scope ended at instruction `at` live through the instruction at `at`,
    /// so the function's final return sees the body's locals.
    pub(super) fn keep_bindings_through(&mut self, at: u32) {
        for b in &mut self.bindings {
            if b.to == at {
                b.to = at.saturating_add(1);
            }
        }
    }

    /// Index of the next instruction, as the `from` and `to` of a binding.
    fn pc(&self) -> u32 {
        u32::try_from(self.insts.len()).unwrap_or(u32::MAX)
    }

    /// Ends the scope of every open binding at scope depth `depth` or deeper.
    fn close_bindings(&mut self, depth: usize) {
        let pc = self.pc();
        while let Some(&(d, idx)) = self.open_bindings.last() {
            if d < depth {
                break;
            }
            self.open_bindings.pop();
            if let Some(b) = self.bindings.get_mut(idx) {
                b.to = pc;
            }
        }
    }

    /// Defines a `let` or `var` local and records its binding for the debugger.
    pub(super) fn bind_local(
        &mut self,
        id: &Ident,
        reg: Reg,
        ty: Ty,
        mutable: bool,
    ) -> Result<(), Diag> {
        self.define_local(id, reg, ty, mutable)?;
        if id.name != "_" {
            let depth = self.scopes.len().saturating_sub(1);
            self.open_bindings.push((depth, self.bindings.len()));
            self.bindings.push(Binding {
                name: id.name.clone(),
                reg,
                from: self.pc(),
                to: u32::MAX,
            });
        }
        Ok(())
    }

    pub(super) fn enter(&mut self, span: Span) -> Result<(), Diag> {
        if self.depth >= MAX_DEPTH {
            return err(span, "nesting too deep");
        }
        self.depth += 1;
        Ok(())
    }

    pub(super) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(super) fn new_reg(&mut self, ty: Ty) -> Reg {
        let r = self.regs.len() as Reg;
        self.regs.push(ty);
        r
    }

    pub(super) fn new_label(&mut self) -> Label {
        let l = self.next_label;
        self.next_label += 1;
        l
    }

    pub(super) fn emit(&mut self, inst: Inst, span: Span) {
        self.insts.push(inst);
        self.spans.push(span);
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.close_bindings(self.scopes.len().saturating_sub(1));
        if let Some(names) = self.scopes.pop() {
            for name in names {
                if let Some(stack) = self.local_map.get_mut(&name) {
                    stack.pop();
                    if stack.is_empty() {
                        self.local_map.remove(&name);
                    }
                }
            }
        }
    }

    pub(super) fn define_local(
        &mut self,
        id: &Ident,
        reg: Reg,
        ty: Ty,
        mutable: bool,
    ) -> Result<(), Diag> {
        if id.name == "_" {
            return Ok(());
        }
        let depth = self.scopes.len().saturating_sub(1);
        let stack = self.local_map.entry(id.name.clone()).or_default();
        if stack.last().is_some_and(|(d, _)| *d == depth) {
            return err(
                id.span,
                format!("duplicate variable '{}' in the same block", id.name),
            );
        }
        stack.push((depth, LocalVar { reg, ty, mutable }));
        if let Some(cur) = self.scopes.last_mut() {
            cur.push(id.name.clone());
        }
        Ok(())
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<&LocalVar> {
        self.local_map
            .get(name)
            .and_then(|stack| stack.last())
            .map(|(_, v)| v)
    }

    pub(super) fn expect_ty(
        &mut self,
        e: &Expr,
        want: &Ty,
        s: Span,
        msg: &str,
    ) -> Result<Reg, Diag> {
        let (r, t, _) = self.check_expr(e, Some(want))?;
        if &t == want {
            Ok(r)
        } else {
            err(s, msg)
        }
    }

    pub(super) fn check_coerced(&mut self, e: &Expr, exp: &Ty, s: Span) -> Result<Reg, Diag> {
        let (r, t, _) = self.check_expr(e, Some(exp))?;
        Ok(self.coerce_if_needed(r, t, Some(exp), s)?.0)
    }

    pub(super) fn lower_logic(
        &mut self,
        is_and: bool,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
    ) -> Result<Reg, Diag> {
        let l_reg = self.expect_ty(
            lhs,
            &Ty::Bool,
            expr_span(lhs),
            "logical operator requires 'bool' operands",
        )?;
        let dst = self.new_reg(Ty::Bool);
        let (l_rhs, l_short, l_end) = (self.new_label(), self.new_label(), self.new_label());
        let (t_lbl, f_lbl) = if is_and {
            (l_rhs, l_short)
        } else {
            (l_short, l_rhs)
        };
        self.emit(Inst::Branch(l_reg, t_lbl, f_lbl), span);
        self.emit(Inst::Label(l_rhs), span);
        let r_reg = self.expect_ty(
            rhs,
            &Ty::Bool,
            expr_span(rhs),
            "logical operator requires 'bool' operands",
        )?;
        self.emit(Inst::Move(dst, r_reg), span);
        self.emit(Inst::Jump(l_end), span);
        self.emit(Inst::Label(l_short), span);
        self.emit(Inst::Lit(dst, Lit::Bool(!is_and)), span);
        self.emit(Inst::Label(l_end), span);
        Ok(dst)
    }

    pub(super) fn lower_cmp_eq(&mut self, op: BinOp, a: Reg, b: Reg, ty: &Ty, span: Span) -> Reg {
        let eq = match ty {
            Ty::Int | Ty::Byte | Ty::Bool | Ty::Unit | Ty::Str => {
                let dst = self.new_reg(Ty::Bool);
                self.emit(Inst::Bin(dst, op, a, b), span);
                return dst;
            }
            Ty::Tuple(items) => {
                let (dst, l_end) = (self.new_reg(Ty::Bool), self.new_label());
                self.emit(Inst::Lit(dst, Lit::Bool(true)), span);
                for (i, elem_ty) in items.iter().enumerate() {
                    let (fa, fb) = (self.new_reg(elem_ty.clone()), self.new_reg(elem_ty.clone()));
                    self.emit(Inst::GetField(fa, a, i as u32), span);
                    self.emit(Inst::GetField(fb, b, i as u32), span);
                    let feq = self.lower_cmp_eq(BinOp::Eq, fa, fb, elem_ty, span);
                    let (l_next, l_false) = (self.new_label(), self.new_label());
                    self.emit(Inst::Branch(feq, l_next, l_false), span);
                    self.emit(Inst::Label(l_false), span);
                    self.emit(Inst::Lit(dst, Lit::Bool(false)), span);
                    self.emit(Inst::Jump(l_end), span);
                    self.emit(Inst::Label(l_next), span);
                }
                self.emit(Inst::Label(l_end), span);
                dst
            }
            Ty::Opt(inner) => {
                let (ta, tb, teq) = (
                    self.new_reg(Ty::Int),
                    self.new_reg(Ty::Int),
                    self.new_reg(Ty::Bool),
                );
                self.emit(Inst::Tag(ta, a), span);
                self.emit(Inst::Tag(tb, b), span);
                self.emit(Inst::Bin(teq, BinOp::Eq, ta, tb), span);
                let (l_same, l_diff) = (self.new_label(), self.new_label());
                let (l_some, l_none, l_end) =
                    (self.new_label(), self.new_label(), self.new_label());
                let dst = self.new_reg(Ty::Bool);
                self.emit(Inst::Branch(teq, l_same, l_diff), span);
                self.emit(Inst::Label(l_diff), span);
                self.emit(Inst::Lit(dst, Lit::Bool(false)), span);
                self.emit(Inst::Jump(l_end), span);
                self.emit(Inst::Label(l_same), span);
                let (zero, is_none) = (self.new_reg(Ty::Int), self.new_reg(Ty::Bool));
                self.emit(Inst::Lit(zero, Lit::Int(0)), span);
                self.emit(Inst::Bin(is_none, BinOp::Eq, ta, zero), span);
                self.emit(Inst::Branch(is_none, l_none, l_some), span);
                self.emit(Inst::Label(l_none), span);
                self.emit(Inst::Lit(dst, Lit::Bool(true)), span);
                self.emit(Inst::Jump(l_end), span);
                self.emit(Inst::Label(l_some), span);
                let (va, vb) = (
                    self.new_reg((**inner).clone()),
                    self.new_reg((**inner).clone()),
                );
                self.emit(Inst::GetField(va, a, 0), span);
                self.emit(Inst::GetField(vb, b, 0), span);
                let veq = self.lower_cmp_eq(BinOp::Eq, va, vb, inner, span);
                self.emit(Inst::Move(dst, veq), span);
                self.emit(Inst::Label(l_end), span);
                dst
            }
            _ => self.emit_lit(Ty::Bool, Lit::Bool(false), span).0,
        };
        if op == BinOp::Ne {
            let neg = self.new_reg(Ty::Bool);
            self.emit(Inst::Un(neg, UnOp::Not, eq), span);
            neg
        } else {
            eq
        }
    }

    pub(super) fn check_match_expr(
        &mut self,
        subject: &Expr,
        arms: &[Arm],
        exp: Option<&Ty>,
        span: Span,
    ) -> ExprRes {
        let (s_reg, s_ty, _) = self.check_expr(subject, None)?;
        for arm in arms {
            check_pattern_ty(self.env, &arm.pattern, &s_ty, &mut Vec::new(), 0)?;
        }
        let pats: Vec<&Pattern> = arms.iter().map(|a| &a.pattern).collect();
        if !is_exhaustive(&self.env.types, &s_ty, &pats, 0) {
            return err(span, "non-exhaustive 'match'");
        }
        let (res_reg, l_end) = (self.new_reg(Ty::Unit), self.new_label());
        let mut cur_ty = exp
            .cloned()
            .or_else(|| arms.iter().find_map(|a| self.peek_expr_ty(&a.body, 0)));
        let (mut all_div, mut div_regs) = (true, Vec::new());
        for arm in arms {
            let (l_fail, mut bindings) = (self.new_label(), Vec::new());
            self.lower_pattern_test(&arm.pattern, s_reg, &s_ty, l_fail, &mut bindings)?;
            self.push_scope();
            for (id, b_reg, b_ty) in bindings {
                self.define_local(&id, b_reg, b_ty, false)?;
            }
            let (b_reg, b_ty, b_div) = self.check_expr(&arm.body, cur_ty.as_ref())?;
            self.pop_scope();
            if !b_div {
                all_div = false;
                if let Some(ref et) = cur_ty {
                    if b_ty != *et {
                        return err(
                            arm.span,
                            format!("'match' arm type mismatch: expected {et:?}, found {b_ty:?}"),
                        );
                    }
                } else {
                    cur_ty = Some(b_ty);
                }
            } else {
                div_regs.push(b_reg);
            }
            self.emit(Inst::Move(res_reg, b_reg), arm.span);
            self.emit(Inst::Jump(l_end), arm.span);
            self.emit(Inst::Label(l_fail), arm.span);
        }
        self.emit(Inst::Label(l_end), span);
        let final_ty = cur_ty.unwrap_or_else(|| exp.cloned().unwrap_or(Ty::Unit));
        self.regs[res_reg as usize] = final_ty.clone();
        for r in div_regs {
            self.regs[r as usize] = final_ty.clone();
        }
        Ok((res_reg, final_ty, all_div))
    }

    pub(super) fn lower_pattern_test(
        &mut self,
        pat: &Pattern,
        val_reg: Reg,
        val_ty: &Ty,
        l_fail: Label,
        bindings: &mut Vec<(Ident, Reg, Ty)>,
    ) -> Result<(), Diag> {
        match pat {
            Pattern::Wild { .. } => Ok(()),
            Pattern::Bind(id) => {
                let r = self.new_reg(val_ty.clone());
                self.emit(Inst::Move(r, val_reg), id.span);
                bindings.push((id.clone(), r, val_ty.clone()));
                Ok(())
            }
            Pattern::Int { value, span } => {
                self.emit_lit_test(val_reg, Ty::Int, Lit::Int(*value), l_fail, *span);
                Ok(())
            }
            Pattern::Byte { value, span } => {
                self.emit_lit_test(val_reg, Ty::Byte, Lit::Byte(*value), l_fail, *span);
                Ok(())
            }
            Pattern::Str { value, span } => {
                self.emit_lit_test(val_reg, Ty::Str, Lit::Str(value.clone()), l_fail, *span);
                Ok(())
            }
            Pattern::Bool { value, span } => {
                self.emit_lit_test(val_reg, Ty::Bool, Lit::Bool(*value), l_fail, *span);
                Ok(())
            }
            Pattern::None { span } => {
                let tag = self.new_reg(Ty::Int);
                self.emit(Inst::Tag(tag, val_reg), *span);
                self.emit_lit_test(tag, Ty::Int, Lit::Int(0), l_fail, *span);
                Ok(())
            }
            Pattern::Some { inner, span } => {
                let Ty::Opt(inner_ty) = val_ty else {
                    return err(*span, "pattern 'some' requires an option type");
                };
                let (tag, payload) = (self.new_reg(Ty::Int), self.new_reg((**inner_ty).clone()));
                self.emit(Inst::Tag(tag, val_reg), *span);
                self.emit_lit_test(tag, Ty::Int, Lit::Int(1), l_fail, *span);
                self.emit(Inst::GetField(payload, val_reg, 0), *span);
                self.lower_pattern_test(inner, payload, inner_ty, l_fail, bindings)
            }
            Pattern::Variant { name, args, span } => {
                let Some(ve) = self.env.find_variant(&name.name) else {
                    return err(name.span, format!("unknown variant '{}'", name.name));
                };
                let (vtag, f_tys, tag) = (ve.tag, ve.fields.clone(), self.new_reg(Ty::Int));
                self.emit(Inst::Tag(tag, val_reg), *span);
                self.emit_lit_test(tag, Ty::Int, Lit::Int(i64::from(vtag)), l_fail, *span);
                for (i, (sub_pat, f_ty)) in args.iter().zip(f_tys.iter()).enumerate() {
                    let f_reg = self.new_reg(f_ty.clone());
                    self.emit(Inst::GetField(f_reg, val_reg, i as u32), *span);
                    self.lower_pattern_test(sub_pat, f_reg, f_ty, l_fail, bindings)?;
                }
                Ok(())
            }
            Pattern::Tuple { items, span } => {
                let Ty::Tuple(elem_tys) = val_ty else {
                    return err(*span, "tuple pattern requires a tuple type");
                };
                for (i, (sub_pat, e_ty)) in items.iter().zip(elem_tys.iter()).enumerate() {
                    let e_reg = self.new_reg(e_ty.clone());
                    self.emit(Inst::GetField(e_reg, val_reg, i as u32), *span);
                    self.lower_pattern_test(sub_pat, e_reg, e_ty, l_fail, bindings)?;
                }
                Ok(())
            }
        }
    }

    fn emit_lit_test(&mut self, val_reg: Reg, ty: Ty, lit: Lit, l_fail: Label, span: Span) {
        let (lit_reg, cmp, l_ok) = (self.new_reg(ty), self.new_reg(Ty::Bool), self.new_label());
        self.emit(Inst::Lit(lit_reg, lit), span);
        self.emit(Inst::Bin(cmp, BinOp::Eq, val_reg, lit_reg), span);
        self.emit(Inst::Branch(cmp, l_ok, l_fail), span);
        self.emit(Inst::Label(l_ok), span);
    }

    pub(super) fn lower_for(
        &mut self,
        var: &Ident,
        iter: &Expr,
        body: &Block,
        span: Span,
    ) -> Result<(), Diag> {
        let (l_head, l_body) = (self.new_label(), self.new_label());
        let (l_step, l_exit, idx) = (self.new_label(), self.new_label(), self.new_reg(Ty::Int));
        let (elem_ty, arr_reg) = if let Expr::Range { start, end, .. } = iter {
            let s_reg = self.expect_ty(
                start,
                &Ty::Int,
                expr_span(start),
                "range start must be 'int'",
            )?;
            let e_reg = self.expect_ty(end, &Ty::Int, expr_span(end), "range end must be 'int'")?;
            self.emit(Inst::Move(idx, s_reg), span);
            self.emit(Inst::Label(l_head), span);
            let cond = self.new_reg(Ty::Bool);
            self.emit(Inst::Bin(cond, BinOp::Lt, idx, e_reg), span);
            self.emit(Inst::Branch(cond, l_body, l_exit), span);
            (Ty::Int, None)
        } else {
            let (a_reg, a_ty, _) = self.check_expr(iter, None)?;
            let Ty::Array(el) = a_ty else {
                return err(
                    expr_span(iter),
                    "'for' loop must iterate over a range or an array",
                );
            };
            self.emit(Inst::Lit(idx, Lit::Int(0)), span);
            self.emit(Inst::Label(l_head), span);
            let (len_reg, cond) = (self.new_reg(Ty::Int), self.new_reg(Ty::Bool));
            self.emit(
                Inst::Builtin(len_reg, Builtin::Len, alloc::vec![a_reg]),
                span,
            );
            self.emit(Inst::Bin(cond, BinOp::Lt, idx, len_reg), span);
            self.emit(Inst::Branch(cond, l_body, l_exit), span);
            (*el, Some(a_reg))
        };
        self.emit(Inst::Label(l_body), span);
        self.push_scope();
        let var_reg = self.new_reg(elem_ty.clone());
        match arr_reg {
            Some(ar) => self.emit(Inst::GetIndex(var_reg, ar, idx), var.span),
            None => self.emit(Inst::Move(var_reg, idx), var.span),
        }
        self.define_local(var, var_reg, elem_ty, false)?;
        self.loops.push((l_exit, l_step));
        let _ = self.check_block(body, None)?;
        self.loops.pop();
        self.pop_scope();
        self.emit(Inst::Label(l_step), span);
        let (one, next) = (self.new_reg(Ty::Int), self.new_reg(Ty::Int));
        self.emit(Inst::Lit(one, Lit::Int(1)), span);
        self.emit(Inst::Bin(next, BinOp::Add, idx, one), span);
        self.emit(Inst::Move(idx, next), span);
        self.emit(Inst::Jump(l_head), span);
        self.emit(Inst::Label(l_exit), span);
        Ok(())
    }

    pub(super) fn lower_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        span: Span,
    ) -> Result<(), Diag> {
        match target {
            Expr::Name(id) => {
                let Some(loc) = self.lookup_local(&id.name).cloned() else {
                    return err(
                        id.span,
                        format!(
                            "cannot assign to '{}': not a mutable local variable",
                            id.name
                        ),
                    );
                };
                if !loc.mutable {
                    return err(
                        id.span,
                        format!("cannot assign to immutable variable '{}'", id.name),
                    );
                }
                let v_reg = self.check_coerced(value, &loc.ty, span)?;
                self.emit(Inst::Move(loc.reg, v_reg), span);
                Ok(())
            }
            Expr::Field { base, name, .. } => {
                self.check_place_chain(base)?;
                let (b_reg, b_ty, _) = self.check_expr(base, None)?;
                let Ty::Named(tid) = b_ty else {
                    return err(name.span, "field assignment requires a record");
                };
                let TypeKind::Record(fields) = &self.env.types[tid as usize].kind else {
                    return err(name.span, "field assignment requires a record");
                };
                let Some((f_idx, (_, f_ty))) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (n, _))| n == &name.name)
                else {
                    return err(name.span, format!("unknown field '{}'", name.name));
                };
                let f_ty = f_ty.clone();
                let v_reg = self.check_coerced(value, &f_ty, span)?;
                self.emit(Inst::SetField(b_reg, f_idx as u32, v_reg), span);
                Ok(())
            }
            Expr::Index {
                base,
                index,
                span: ispan,
            } => {
                if matches!(index.as_ref(), Expr::Range { .. }) {
                    return err(*ispan, "cannot assign to a slice");
                }
                self.check_place_chain(base)?;
                let (b_reg, b_ty, _) = self.check_expr(base, None)?;
                let Ty::Array(elem_ty) = b_ty else {
                    return err(*ispan, "index assignment requires an array");
                };
                let i_reg = self.expect_ty(
                    index,
                    &Ty::Int,
                    expr_span(index),
                    "array index must be 'int'",
                )?;
                let v_reg = self.check_coerced(value, &elem_ty, span)?;
                self.emit(Inst::SetIndex(b_reg, i_reg, v_reg), span);
                Ok(())
            }
            _ => err(
                expr_span(target),
                "left side of '=' must be a 'var' name, record field, or array index",
            ),
        }
    }

    fn check_place_chain(&self, e: &Expr) -> Result<(), Diag> {
        match e {
            Expr::Name(id) if self.lookup_local(&id.name).is_some() => Ok(()),
            Expr::Name(id) => err(id.span, "assignment target base must be a variable"),
            Expr::Field { base, .. } => self.check_place_chain(base),
            Expr::Index { index, span, .. } if matches!(index.as_ref(), Expr::Range { .. }) => {
                err(*span, "slice is not a place")
            }
            Expr::Index { base, .. } => self.check_place_chain(base),
            _ => err(expr_span(e), "expression is not a valid place"),
        }
    }

    pub(super) fn coerce_if_needed(
        &mut self,
        reg: Reg,
        act: Ty,
        exp: Option<&Ty>,
        s: Span,
    ) -> Result<(Reg, Ty), Diag> {
        let Some(exp) = exp else {
            return Ok((reg, act));
        };
        check_assignable(self.env, &act, exp, s)?;
        if act != *exp {
            let dst = self.new_reg(exp.clone());
            self.emit(Inst::Move(dst, reg), s);
            Ok((dst, exp.clone()))
        } else {
            Ok((reg, act))
        }
    }
}
