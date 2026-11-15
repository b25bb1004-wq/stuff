//! Expression, statement, and block type checking.
use super::layers::check_assignable;
use super::lower::FnBuilder;
use super::names::{err, expr_span, resolve_builtin};
use super::{is_eq_type, ExprRes};
use crate::ast::{BinOp, Block, Expr, Ident, RecordField, Stmt, UnOp};
use crate::ir::{Builtin, Inst, Lit, Reg, Ty, TypeKind};
use crate::span::{Diag, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

impl FnBuilder<'_> {
    pub(super) fn peek_block_ty(&self, b: &Block, depth: u32) -> Option<Ty> {
        match b.stmts.last() {
            Some(Stmt::Expr { expr, .. }) => self.peek_expr_ty(expr, depth + 1),
            Some(Stmt::Return { .. } | Stmt::Break { .. } | Stmt::Continue { .. }) => None,
            _ => Some(Ty::Unit),
        }
    }

    pub(super) fn peek_expr_ty(&self, e: &Expr, d: u32) -> Option<Ty> {
        if d > 32 {
            return None;
        }
        match e {
            Expr::Int { .. } => Some(Ty::Int),
            Expr::Byte { .. } => Some(Ty::Byte),
            Expr::Str { .. } => Some(Ty::Str),
            Expr::Bool { .. } => Some(Ty::Bool),
            Expr::Unit { .. } => Some(Ty::Unit),
            Expr::None { .. } => None,
            Expr::Some { value, .. } => Some(Ty::Opt(Box::new(self.peek_expr_ty(value, d + 1)?))),
            Expr::Name(id) => {
                if let Some(loc) = self.lookup_local(&id.name) {
                    return Some(loc.ty.clone());
                }
                if let Some(c) = self.env.find_const(&id.name) {
                    return Some(c.ty.clone());
                }
                self.env.find_func(&id.name).map(|f| Ty::Fn {
                    params: f.params.clone(),
                    ret: Box::new(f.ret.clone()),
                    layers: f.layers,
                })
            }
            Expr::Variant { name, .. } => self
                .env
                .find_variant(&name.name)
                .map(|v| Ty::Named(v.type_id)),
            Expr::Record { name, .. } => self.env.find_type(&name.name).map(Ty::Named),
            Expr::List { items, .. } => items
                .iter()
                .find_map(|it| self.peek_expr_ty(it, d + 1))
                .map(|t| Ty::Array(Box::new(t))),
            Expr::Repeat { value, .. } => {
                Some(Ty::Array(Box::new(self.peek_expr_ty(value, d + 1)?)))
            }
            Expr::Tuple { items, .. } => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.peek_expr_ty(it, d + 1)?);
                }
                Some(Ty::Tuple(out))
            }
            Expr::Unary { op, .. } => Some(if *op == UnOp::Neg { Ty::Int } else { Ty::Bool }),
            Expr::Binary { op, lhs, rhs, .. } => match op {
                BinOp::And | BinOp::Or | BinOp::Eq | BinOp::Ne => Some(Ty::Bool),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some(Ty::Bool),
                _ => self
                    .peek_expr_ty(lhs, d + 1)
                    .or_else(|| self.peek_expr_ty(rhs, d + 1)),
            },
            Expr::Call { callee, args, .. } => {
                if let Expr::Name(id) = callee.as_ref() {
                    if self.lookup_local(&id.name).is_none() {
                        if let Some(f) = self.env.find_func(&id.name) {
                            return Some(f.ret.clone());
                        }
                        if let Some(b) = resolve_builtin(&id.name) {
                            return match b {
                                Builtin::Len | Builtin::ToInt | Builtin::Abs => Some(Ty::Int),
                                Builtin::Min | Builtin::Max => Some(Ty::Int),
                                Builtin::ToByte => Some(Ty::Byte),
                                Builtin::IntToStr => Some(Ty::Str),
                                Builtin::Push => Some(Ty::Unit),
                                Builtin::ParseInt => Some(Ty::Opt(Box::new(Ty::Int))),
                                Builtin::Text => Some(Ty::Opt(Box::new(Ty::Str))),
                                Builtin::Bytes => Some(Ty::Array(Box::new(Ty::Byte))),
                                Builtin::Pop => match self.peek_expr_ty(args.first()?, d + 1)? {
                                    Ty::Array(el) => Some(Ty::Opt(el)),
                                    _ => None,
                                },
                                _ => None,
                            };
                        }
                    }
                }
                match self.peek_expr_ty(callee, d + 1)? {
                    Ty::Fn { ret, .. } => Some(*ret),
                    _ => None,
                }
            }
            Expr::If {
                then,
                els: Some(el),
                ..
            } => self
                .peek_block_ty(then, d + 1)
                .or_else(|| self.peek_expr_ty(el, d + 1)),
            Expr::If { els: None, .. } => Some(Ty::Unit),
            Expr::Match { arms, .. } => arms.iter().find_map(|a| self.peek_expr_ty(&a.body, d + 1)),
            Expr::Block(b) => self.peek_block_ty(b, d + 1),
            _ => None,
        }
    }

    pub(super) fn check_block(&mut self, block: &Block, expected: Option<&Ty>) -> ExprRes {
        self.enter(block.span)?;
        self.push_scope();
        let res = self.check_block_inner(block, expected);
        self.pop_scope();
        self.leave();
        res
    }

    fn check_block_inner(&mut self, block: &Block, expected: Option<&Ty>) -> ExprRes {
        let (mut diverges, n) = (false, block.stmts.len());
        for (i, s) in block.stmts.iter().enumerate() {
            if i + 1 == n {
                if let Stmt::Expr { expr, .. } = s {
                    let (r, ty, d) = self.check_expr(expr, expected)?;
                    diverges |= d;
                    if diverges {
                        if let Some(exp) = expected {
                            return Ok((self.new_reg(exp.clone()), exp.clone(), true));
                        }
                    }
                    return Ok((r, ty, diverges));
                }
            }
            diverges |= self.check_stmt(s)?;
        }
        let ty = if diverges {
            expected.cloned().unwrap_or(Ty::Unit)
        } else {
            Ty::Unit
        };
        let r = self.new_reg(ty.clone());
        if ty == Ty::Unit {
            self.emit(Inst::Lit(r, Lit::Unit), super::tail_span(block));
        }
        Ok((r, ty, diverges))
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Result<bool, Diag> {
        match stmt {
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
                span,
            } => {
                let ann = ty.as_ref().map(|t| self.env.resolve_ty(t, 0)).transpose()?;
                let (v_reg, v_ty, d) = self.check_expr(value, ann.as_ref())?;
                let (v_reg, final_ty) = self.coerce_if_needed(v_reg, v_ty, ann.as_ref(), *span)?;
                let var_reg = self.new_reg(final_ty.clone());
                self.emit(Inst::Move(var_reg, v_reg), *span);
                self.bind_local(name, var_reg, final_ty, *mutable)?;
                Ok(d)
            }
            Stmt::Return { value, span } => {
                let ret_ty = self.ret_ty.clone();
                let r = match value {
                    Some(e) => self.check_coerced(e, &ret_ty, *span)?,
                    None if ret_ty == Ty::Unit => self.emit_lit(Ty::Unit, Lit::Unit, *span).0,
                    None => return err(*span, "missing return value"),
                };
                self.emit(Inst::Ret(r), *span);
                Ok(true)
            }
            Stmt::Break { span } => {
                let Some(&(l_brk, _)) = self.loops.last() else {
                    return err(*span, "'break' is only allowed inside a loop");
                };
                self.emit(Inst::Jump(l_brk), *span);
                Ok(true)
            }
            Stmt::Continue { span } => {
                let Some(&(_, l_cnt)) = self.loops.last() else {
                    return err(*span, "'continue' is only allowed inside a loop");
                };
                self.emit(Inst::Jump(l_cnt), *span);
                Ok(true)
            }
            Stmt::While { cond, body, span } => {
                let (l_head, l_body, l_exit) =
                    (self.new_label(), self.new_label(), self.new_label());
                self.emit(Inst::Label(l_head), *span);
                let c_reg = self.expect_ty(
                    cond,
                    &Ty::Bool,
                    expr_span(cond),
                    "'while' condition must be 'bool'",
                )?;
                self.emit(Inst::Branch(c_reg, l_body, l_exit), *span);
                self.emit(Inst::Label(l_body), *span);
                self.loops.push((l_exit, l_head));
                let _ = self.check_block(body, None)?;
                self.loops.pop();
                self.emit(Inst::Jump(l_head), *span);
                self.emit(Inst::Label(l_exit), *span);
                Ok(false)
            }
            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                self.lower_for(var, iter, body, *span)?;
                Ok(false)
            }
            Stmt::Assert { cond, msg, span } => {
                let c_reg = self.expect_ty(
                    cond,
                    &Ty::Bool,
                    expr_span(cond),
                    "'assert' condition must be 'bool'",
                )?;
                let msg_text = match msg {
                    None => String::new(),
                    Some(Expr::Str { value, .. }) => value.clone(),
                    Some(other) => {
                        if let Expr::Name(id) = other {
                            if let Some(c) = self.env.find_const(&id.name) {
                                if let Lit::Str(s) = &c.lit {
                                    self.emit(Inst::Assert(c_reg, s.clone()), *span);
                                    return Ok(false);
                                }
                            }
                        }
                        let _ = self.expect_ty(
                            other,
                            &Ty::Str,
                            expr_span(other),
                            "'assert' message must be 'str'",
                        )?;
                        return err(
                            expr_span(other),
                            "'assert' message must be a string literal",
                        );
                    }
                };
                self.emit(Inst::Assert(c_reg, msg_text), *span);
                Ok(false)
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                self.lower_assign(target, value, *span)?;
                Ok(false)
            }
            Stmt::Expr { expr, .. } => Ok(self.check_expr(expr, None)?.2),
        }
    }

    pub(super) fn check_expr(&mut self, expr: &Expr, expected: Option<&Ty>) -> ExprRes {
        let span = expr_span(expr);
        self.enter(span)?;
        let res = self.check_expr_inner(expr, expected);
        self.leave();
        let (reg, ty, div) = res?;
        if matches!((&ty, expected), (Ty::Fn { .. }, Some(Ty::Fn { .. }))) {
            let (reg, ty) = self.coerce_if_needed(reg, ty, expected, span)?;
            return Ok((reg, ty, div));
        }
        Ok((reg, ty, div))
    }

    fn check_expr_inner(&mut self, expr: &Expr, expected: Option<&Ty>) -> ExprRes {
        match expr {
            Expr::Int { value, span } => Ok(self.emit_lit(Ty::Int, Lit::Int(*value), *span)),
            Expr::Byte { value, span } => Ok(self.emit_lit(Ty::Byte, Lit::Byte(*value), *span)),
            Expr::Str { value, span } => Ok(self.emit_lit(Ty::Str, Lit::Str(value.clone()), *span)),
            Expr::Bool { value, span } => Ok(self.emit_lit(Ty::Bool, Lit::Bool(*value), *span)),
            Expr::Unit { span } => Ok(self.emit_lit(Ty::Unit, Lit::Unit, *span)),
            Expr::None { span } => {
                let Some(Ty::Opt(inner)) = expected else {
                    return err(*span, "cannot infer type for 'none'; add a type annotation");
                };
                let ty = Ty::Opt(inner.clone());
                let r = self.new_reg(ty.clone());
                self.emit(Inst::NewNone(r), *span);
                Ok((r, ty, false))
            }
            Expr::Some { value, span } => {
                let inner_exp = match expected {
                    Some(Ty::Opt(inner)) => Some(inner.as_ref()),
                    _ => None,
                };
                let (v_reg, v_ty, d) = self.check_expr(value, inner_exp)?;
                let ty = Ty::Opt(Box::new(v_ty));
                let r = self.new_reg(ty.clone());
                self.emit(Inst::NewSome(r, v_reg), *span);
                Ok((r, ty, d))
            }
            Expr::Name(id) => self.check_name_expr(id, expected),
            Expr::Variant { name, args, span } => self.check_variant_expr(name, args, *span),
            Expr::Record { name, fields, span } => self.check_record_expr(name, fields, *span),
            Expr::List { items, span } => self.check_list_expr(items, expected, *span),
            Expr::Repeat { value, count, span } => {
                let elem_exp = match expected {
                    Some(Ty::Array(el)) => Some(el.as_ref()),
                    _ => None,
                };
                let (v_reg, v_ty, _) = self.check_expr(value, elem_exp)?;
                let c_reg = self.expect_ty(
                    count,
                    &Ty::Int,
                    expr_span(count),
                    "repeat count must be 'int'",
                )?;
                let ty = Ty::Array(Box::new(v_ty));
                let r = self.new_reg(ty.clone());
                self.emit(Inst::NewFill(r, v_reg, c_reg), *span);
                Ok((r, ty, false))
            }
            Expr::Tuple { items, span } => {
                let (mut regs, mut tys) = (
                    Vec::with_capacity(items.len()),
                    Vec::with_capacity(items.len()),
                );
                for (i, it) in items.iter().enumerate() {
                    let it_exp = match expected {
                        Some(Ty::Tuple(ts)) if ts.len() == items.len() => Some(&ts[i]),
                        _ => None,
                    };
                    let (r, t, _) = self.check_expr(it, it_exp)?;
                    regs.push(r);
                    tys.push(t);
                }
                let ty = Ty::Tuple(tys);
                let dst = self.new_reg(ty.clone());
                self.emit(Inst::NewTuple(dst, regs), *span);
                Ok((dst, ty, false))
            }
            Expr::Unary { op, operand, span } => {
                let exp = if *op == UnOp::Neg {
                    &Ty::Int
                } else {
                    &Ty::Bool
                };
                let (o_reg, o_ty, _) = self.check_expr(operand, Some(exp))?;
                if o_ty != *exp {
                    return err(*span, format!("invalid operand type {o_ty:?} for {op:?}"));
                }
                let dst = self.new_reg(o_ty.clone());
                self.emit(Inst::Un(dst, *op, o_reg), *span);
                Ok((dst, o_ty, false))
            }
            Expr::Binary { op, lhs, rhs, span } => self.check_binary_expr(*op, lhs, rhs, *span),
            Expr::Pipe { lhs, rhs, span } => match rhs.as_ref() {
                Expr::Call { callee, args, .. } => {
                    let mut all = Vec::with_capacity(1 + args.len());
                    all.push((**lhs).clone());
                    all.extend(args.iter().cloned());
                    self.check_call_expr(callee, &all, *span)
                }
                Expr::Name(_) => self.check_call_expr(rhs, &[(**lhs).clone()], *span),
                _ => err(*span, "right side of '|>' must be a function call"),
            },
            Expr::Range { span, .. } => {
                err(*span, "ranges are only allowed in 'for' loops and slices")
            }
            Expr::Call { callee, args, span } => self.check_call_expr(callee, args, *span),
            Expr::Index { base, index, span } => self.check_index_expr(base, index, *span),
            Expr::Field { base, name, span } => {
                let (b_reg, b_ty, _) = self.check_expr(base, None)?;
                let Ty::Named(tid) = b_ty else {
                    return err(name.span, "field access requires a record type");
                };
                let TypeKind::Record(fields) = &self.env.types[tid as usize].kind else {
                    return err(name.span, "field access requires a record type");
                };
                let Some((idx, (_, f_ty))) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (n, _))| n == &name.name)
                else {
                    return err(name.span, format!("unknown field '{}'", name.name));
                };
                let f_ty = f_ty.clone();
                let dst = self.new_reg(f_ty.clone());
                self.emit(Inst::GetField(dst, b_reg, idx as u32), *span);
                Ok((dst, f_ty, false))
            }
            Expr::TupleField { base, index, span } => {
                let (b_reg, b_ty, _) = self.check_expr(base, None)?;
                let Ty::Tuple(items) = b_ty else {
                    return err(*span, "tuple field access requires a tuple type");
                };
                let Some(elem_ty) = items.get(*index as usize).cloned() else {
                    return err(*span, "tuple index out of bounds");
                };
                let dst = self.new_reg(elem_ty.clone());
                self.emit(Inst::GetField(dst, b_reg, *index), *span);
                Ok((dst, elem_ty, false))
            }
            Expr::If {
                cond,
                then,
                els,
                span,
            } => self.check_if_expr(cond, then, els.as_deref(), expected, *span),
            Expr::Match {
                subject,
                arms,
                span,
            } => self.check_match_expr(subject, arms, expected, *span),
            Expr::Block(b) => self.check_block(b, expected),
        }
    }

    pub(super) fn emit_lit(&mut self, ty: Ty, lit: Lit, span: Span) -> (Reg, Ty, bool) {
        let r = self.new_reg(ty.clone());
        self.emit(Inst::Lit(r, lit), span);
        (r, ty, false)
    }

    fn emit_bin(&mut self, ty: Ty, op: BinOp, a: Reg, b: Reg, span: Span) -> ExprRes {
        let dst = self.new_reg(ty.clone());
        self.emit(Inst::Bin(dst, op, a, b), span);
        Ok((dst, ty, false))
    }

    fn check_name_expr(&mut self, id: &Ident, expected: Option<&Ty>) -> ExprRes {
        if let Some(loc) = self.lookup_local(&id.name) {
            return Ok((loc.reg, loc.ty.clone(), false));
        }
        if let Some(c) = self.env.find_const(&id.name) {
            return Ok(self.emit_lit(c.ty.clone(), c.lit.clone(), id.span));
        }
        if let Some(f) = self.env.find_func(&id.name) {
            let fn_ty = Ty::Fn {
                params: f.params.clone(),
                ret: Box::new(f.ret.clone()),
                layers: f.layers,
            };
            let fid = f.id;
            let reg_ty = if let Some(exp) = expected {
                check_assignable(self.env, &fn_ty, exp, id.span)?;
                exp.clone()
            } else {
                fn_ty
            };
            return Ok(self.emit_lit(reg_ty, Lit::Fn(fid), id.span));
        }
        if resolve_builtin(&id.name).is_some() {
            return err(
                id.span,
                format!("built-in function '{}' cannot be used as a value", id.name),
            );
        }
        err(id.span, format!("unknown name '{}'", id.name))
    }

    fn check_variant_expr(&mut self, name: &Ident, args: &[Expr], span: Span) -> ExprRes {
        let Some(ve) = self.env.find_variant(&name.name) else {
            return err(name.span, format!("unknown variant '{}'", name.name));
        };
        let (tid, tag, f_tys) = (ve.type_id, ve.tag, ve.fields.clone());
        let (exp, got, vname) = (f_tys.len(), args.len(), &name.name);
        if got != exp {
            return err(
                span,
                format!("variant '{vname}' expects {exp} arguments, found {got}"),
            );
        }
        let mut regs = Vec::with_capacity(args.len());
        for (a, fty) in args.iter().zip(f_tys.iter()) {
            regs.push(self.check_coerced(a, fty, expr_span(a))?);
        }
        let ty = Ty::Named(tid);
        let dst = self.new_reg(ty.clone());
        self.emit(Inst::NewVariant(dst, tid, tag, regs), span);
        Ok((dst, ty, false))
    }

    fn check_record_expr(&mut self, name: &Ident, fields: &[RecordField], span: Span) -> ExprRes {
        let Some(tid) = self.env.find_type(&name.name) else {
            return err(name.span, format!("unknown type '{}'", name.name));
        };
        let TypeKind::Record(decl_fields) = self.env.types[tid as usize].kind.clone() else {
            return err(name.span, format!("'{}' is not a record type", name.name));
        };
        let mut slots: Vec<Option<Reg>> = alloc::vec![None; decl_fields.len()];
        for rf in fields {
            let Some((idx, (_, fty))) = decl_fields
                .iter()
                .enumerate()
                .find(|(_, (n, _))| n == &rf.name.name)
            else {
                return err(
                    rf.name.span,
                    format!("unknown field '{}' on '{}'", rf.name.name, name.name),
                );
            };
            if slots[idx].is_some() {
                return err(rf.name.span, format!("duplicate field '{}'", rf.name.name));
            }
            slots[idx] = Some(self.check_coerced(&rf.value, fty, rf.span)?);
        }
        let mut ordered = Vec::with_capacity(decl_fields.len());
        for (i, slot) in slots.into_iter().enumerate() {
            let Some(r) = slot else {
                return err(
                    span,
                    format!("missing field '{}' in '{}'", decl_fields[i].0, name.name),
                );
            };
            ordered.push(r);
        }
        let ty = Ty::Named(tid);
        let dst = self.new_reg(ty.clone());
        self.emit(Inst::NewRecord(dst, tid, ordered), span);
        Ok((dst, ty, false))
    }

    fn check_list_expr(&mut self, items: &[Expr], expected: Option<&Ty>, span: Span) -> ExprRes {
        let exp_elem = match expected {
            Some(Ty::Array(el)) => Some((**el).clone()),
            _ => items.iter().find_map(|it| self.peek_expr_ty(it, 0)),
        };
        if items.is_empty() {
            let Some(el_ty) = exp_elem else {
                return err(
                    span,
                    "cannot infer type for empty list '[]'; add a type annotation",
                );
            };
            let ty = Ty::Array(Box::new(el_ty));
            let dst = self.new_reg(ty.clone());
            self.emit(Inst::NewArray(dst, Vec::new()), span);
            return Ok((dst, ty, false));
        }
        let (mut elem_ty, mut regs) = (exp_elem, Vec::with_capacity(items.len()));
        for it in items {
            let (r, t, _) = self.check_expr(it, elem_ty.as_ref())?;
            if let Some(ref et) = elem_ty {
                regs.push(self.coerce_if_needed(r, t, Some(et), expr_span(it))?.0);
            } else {
                elem_ty = Some(t);
                regs.push(r);
            }
        }
        let ty = Ty::Array(Box::new(elem_ty.unwrap_or(Ty::Unit)));
        let dst = self.new_reg(ty.clone());
        self.emit(Inst::NewArray(dst, regs), span);
        Ok((dst, ty, false))
    }

    fn check_binary_expr(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span) -> ExprRes {
        if matches!(op, BinOp::And | BinOp::Or) {
            return Ok((
                self.lower_logic(op == BinOp::And, lhs, rhs, span)?,
                Ty::Bool,
                false,
            ));
        }
        let hint = self
            .peek_expr_ty(lhs, 0)
            .or_else(|| self.peek_expr_ty(rhs, 0));
        let (l_reg, l_ty, _) = self.check_expr(lhs, hint.as_ref())?;
        let (r_reg, r_ty, _) = self.check_expr(rhs, Some(&l_ty))?;
        if l_ty != r_ty {
            return err(
                span,
                format!("binary operator {op:?} type mismatch: {l_ty:?} vs {r_ty:?}"),
            );
        }
        match op {
            BinOp::Eq | BinOp::Ne => {
                if !is_eq_type(&l_ty) {
                    return err(span, format!("equality is not supported on {l_ty:?}"));
                }
                Ok((
                    self.lower_cmp_eq(op, l_reg, r_reg, &l_ty, span),
                    Ty::Bool,
                    false,
                ))
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if !matches!(l_ty, Ty::Int | Ty::Byte | Ty::Str) {
                    return err(span, format!("ordering is not supported on {l_ty:?}"));
                }
                self.emit_bin(Ty::Bool, op, l_reg, r_reg, span)
            }
            BinOp::Add => match &l_ty {
                Ty::Int | Ty::Byte => self.emit_bin(l_ty, BinOp::Add, l_reg, r_reg, span),
                Ty::Str | Ty::Array(_) => {
                    let dst = self.new_reg(l_ty.clone());
                    self.emit(
                        Inst::Builtin(dst, Builtin::Concat, alloc::vec![l_reg, r_reg]),
                        span,
                    );
                    Ok((dst, l_ty, false))
                }
                _ => err(span, format!("'+' is not supported on {l_ty:?}")),
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if !matches!(l_ty, Ty::Int | Ty::Byte) {
                    return err(
                        span,
                        format!("arithmetic {op:?} is not supported on {l_ty:?}"),
                    );
                }
                self.emit_bin(l_ty, op, l_reg, r_reg, span)
            }
            _ => {
                if l_ty != Ty::Int {
                    return err(span, format!("operator {op:?} requires 'int'"));
                }
                self.emit_bin(Ty::Int, op, l_reg, r_reg, span)
            }
        }
    }

    fn check_index_expr(&mut self, base: &Expr, index: &Expr, span: Span) -> ExprRes {
        let (b_reg, b_ty, _) = self.check_expr(base, None)?;
        if let Expr::Range { start, end, .. } = index {
            if !matches!(b_ty, Ty::Array(_) | Ty::Str) {
                return err(span, "slicing requires an array or 'str'");
            }
            let s_reg = self.expect_ty(start, &Ty::Int, span, "slice bounds must be 'int'")?;
            let e_reg = self.expect_ty(end, &Ty::Int, span, "slice bounds must be 'int'")?;
            let dst = self.new_reg(b_ty.clone());
            self.emit(
                Inst::Builtin(dst, Builtin::Slice, alloc::vec![b_reg, s_reg, e_reg]),
                span,
            );
            Ok((dst, b_ty, false))
        } else {
            let Ty::Array(elem_ty) = b_ty else {
                return err(span, "indexing requires an array");
            };
            let i_reg = self.expect_ty(
                index,
                &Ty::Int,
                expr_span(index),
                "array index must be 'int'",
            )?;
            let dst = self.new_reg((*elem_ty).clone());
            self.emit(Inst::GetIndex(dst, b_reg, i_reg), span);
            Ok((dst, *elem_ty, false))
        }
    }

    fn check_if_expr(
        &mut self,
        cond: &Expr,
        then: &Block,
        els: Option<&Expr>,
        exp: Option<&Ty>,
        span: Span,
    ) -> ExprRes {
        let c_reg = self.expect_ty(
            cond,
            &Ty::Bool,
            expr_span(cond),
            "'if' condition must be 'bool'",
        )?;
        let Some(el_expr) = els else {
            let (l_then, l_end) = (self.new_label(), self.new_label());
            self.emit(Inst::Branch(c_reg, l_then, l_end), span);
            self.emit(Inst::Label(l_then), span);
            let (_, t_ty, t_div) = self.check_block(then, Some(&Ty::Unit))?;
            if !t_div && t_ty != Ty::Unit {
                return err(then.span, "'if' without 'else' must have type 'unit'");
            }
            self.emit(Inst::Label(l_end), span);
            return Ok(self.emit_lit(Ty::Unit, Lit::Unit, span));
        };
        let hint = exp
            .cloned()
            .or_else(|| self.peek_block_ty(then, 0))
            .or_else(|| self.peek_expr_ty(el_expr, 0));
        let (l_then, l_else) = (self.new_label(), self.new_label());
        let (l_end, res_reg) = (self.new_label(), self.new_reg(Ty::Unit));
        self.emit(Inst::Branch(c_reg, l_then, l_else), span);
        self.emit(Inst::Label(l_then), span);
        let (t_reg, t_ty, t_div) = self.check_block(then, hint.as_ref())?;
        self.emit(Inst::Move(res_reg, t_reg), then.span);
        self.emit(Inst::Jump(l_end), then.span);
        self.emit(Inst::Label(l_else), span);
        let el_exp = hint.as_ref().or(if !t_div { Some(&t_ty) } else { None });
        let (e_reg, e_ty, e_div) = self.check_expr(el_expr, el_exp)?;
        self.emit(Inst::Move(res_reg, e_reg), expr_span(el_expr));
        self.emit(Inst::Label(l_end), span);
        let final_ty = match (t_div, e_div) {
            (false, false) if t_ty != e_ty => {
                return err(
                    span,
                    format!("'if' branches have different types: {t_ty:?} vs {e_ty:?}"),
                )
            }
            (false, _) => t_ty,
            (true, false) => e_ty,
            (true, true) => hint.unwrap_or(Ty::Unit),
        };
        self.regs[res_reg as usize] = final_ty.clone();
        if t_div && !e_div {
            self.regs[t_reg as usize] = final_ty.clone();
        } else if !t_div && e_div {
            self.regs[e_reg as usize] = final_ty.clone();
        }
        Ok((res_reg, final_ty, t_div && e_div))
    }
}
