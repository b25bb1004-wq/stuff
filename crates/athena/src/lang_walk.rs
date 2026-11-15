//! AST block, statement, expression, pattern, and type walkers for `athena::lang`.

use std::collections::BTreeMap;

use logos::ast::{Block, Expr, Ident, Pattern, Stmt, Type};

use crate::lang::{
    entry_to_resolved, lookup_value, span_contains, span_range, Entry, Resolved, TopEnv,
};
use crate::lang_fmt::fmt_type;

pub(crate) fn visit_block(
    block: &Block,
    offset: usize,
    top: &TopEnv,
    scopes: &mut Vec<BTreeMap<String, Entry>>,
) -> Option<Resolved> {
    if !span_contains(block.span, offset) {
        return None;
    }
    scopes.push(BTreeMap::new());
    let mut found = None;
    for stmt in &block.stmts {
        if let Some(r) = visit_stmt(stmt, offset, top, scopes) {
            found = Some(r);
            break;
        }
    }
    scopes.pop();
    found
}

fn let_hover(mutable: bool, name: &Ident, ty: Option<&Type>) -> String {
    let kw = if mutable { "var" } else { "let" };
    match ty {
        Some(t) => format!("{kw} {}: {}", name.name, fmt_type(t)),
        None => format!("{kw} {}", name.name),
    }
}

fn visit_stmt(
    stmt: &Stmt,
    offset: usize,
    top: &TopEnv,
    scopes: &mut Vec<BTreeMap<String, Entry>>,
) -> Option<Resolved> {
    match stmt {
        Stmt::Let {
            mutable,
            name,
            ty,
            value,
            span,
        } => {
            let entry = Entry {
                def_span: Some(span_range(name.span)),
                hover: let_hover(*mutable, name, ty.as_ref()),
            };
            if span_contains(*span, offset) {
                if span_contains(name.span, offset) {
                    return Some(entry_to_resolved(&entry));
                }
                if let Some(t) = ty {
                    if let Some(r) = visit_type(t, offset, top) {
                        return Some(r);
                    }
                }
                if let Some(r) = visit_expr(value, offset, top, scopes) {
                    return Some(r);
                }
            }
            if let Some(scope) = scopes.last_mut() {
                scope.insert(name.name.clone(), entry);
            }
            None
        }
        Stmt::Return { value, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            value
                .as_ref()
                .and_then(|v| visit_expr(v, offset, top, scopes))
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => None,
        Stmt::While { cond, body, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(cond, offset, top, scopes).or_else(|| visit_block(body, offset, top, scopes))
        }
        Stmt::For {
            var,
            iter,
            body,
            span,
        } => {
            if !span_contains(*span, offset) {
                return None;
            }
            if let Some(r) = visit_expr(iter, offset, top, scopes) {
                return Some(r);
            }
            let entry = Entry {
                def_span: Some(span_range(var.span)),
                hover: format!("let {}", var.name),
            };
            if span_contains(var.span, offset) {
                return Some(entry_to_resolved(&entry));
            }
            let mut loop_scope = BTreeMap::new();
            loop_scope.insert(var.name.clone(), entry);
            scopes.push(loop_scope);
            let res = visit_block(body, offset, top, scopes);
            scopes.pop();
            res
        }
        Stmt::Assert { cond, msg, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(cond, offset, top, scopes).or_else(|| {
                msg.as_ref()
                    .and_then(|m| visit_expr(m, offset, top, scopes))
            })
        }
        Stmt::Assign {
            target,
            value,
            span,
        } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(target, offset, top, scopes)
                .or_else(|| visit_expr(value, offset, top, scopes))
        }
        Stmt::Expr { expr, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(expr, offset, top, scopes)
        }
    }
}

fn lit_resolved(kind: &str) -> Resolved {
    Resolved {
        def_span: None,
        hover: Some(String::from(kind)),
    }
}

pub(crate) fn visit_expr(
    expr: &Expr,
    offset: usize,
    top: &TopEnv,
    scopes: &mut Vec<BTreeMap<String, Entry>>,
) -> Option<Resolved> {
    match expr {
        Expr::Int { span, .. } => span_contains(*span, offset).then(|| lit_resolved("int literal")),
        Expr::Byte { span, .. } => {
            span_contains(*span, offset).then(|| lit_resolved("byte literal"))
        }
        Expr::Str { span, .. } => span_contains(*span, offset).then(|| lit_resolved("str literal")),
        Expr::Bool { span, .. } => {
            span_contains(*span, offset).then(|| lit_resolved("bool literal"))
        }
        Expr::None { .. } | Expr::Unit { .. } => None,
        Expr::Some { value, span } => {
            span_contains(*span, offset).then(|| visit_expr(value, offset, top, scopes))?
        }
        Expr::Name(id) => span_contains(id.span, offset)
            .then(|| lookup_value(&id.name, top, scopes))
            .flatten(),
        Expr::Variant { name, args, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            if span_contains(name.span, offset) {
                return top.variants.get(&name.name).map(entry_to_resolved);
            }
            args.iter().find_map(|a| visit_expr(a, offset, top, scopes))
        }
        Expr::Record { name, fields, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            let type_entry = top.types.get(&name.name);
            if span_contains(name.span, offset) {
                return type_entry.map(|te| entry_to_resolved(&te.entry));
            }
            for f in fields {
                if span_contains(f.name.span, offset) {
                    return type_entry
                        .and_then(|te| te.fields.get(&f.name.name))
                        .map(entry_to_resolved);
                }
                if let Some(r) = visit_expr(&f.value, offset, top, scopes) {
                    return Some(r);
                }
            }
            None
        }
        Expr::List { items, span } | Expr::Tuple { items, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            items
                .iter()
                .find_map(|item| visit_expr(item, offset, top, scopes))
        }
        Expr::Repeat { value, count, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(value, offset, top, scopes)
                .or_else(|| visit_expr(count, offset, top, scopes))
        }
        Expr::Unary { operand, span, .. } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(operand, offset, top, scopes)
        }
        Expr::Binary { lhs, rhs, span, .. } | Expr::Pipe { lhs, rhs, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(lhs, offset, top, scopes).or_else(|| visit_expr(rhs, offset, top, scopes))
        }
        Expr::Range { start, end, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(start, offset, top, scopes).or_else(|| visit_expr(end, offset, top, scopes))
        }
        Expr::Call { callee, args, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(callee, offset, top, scopes)
                .or_else(|| args.iter().find_map(|a| visit_expr(a, offset, top, scopes)))
        }
        Expr::Index { base, index, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(base, offset, top, scopes).or_else(|| visit_expr(index, offset, top, scopes))
        }
        Expr::Field { base, span, .. } | Expr::TupleField { base, span, .. } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(base, offset, top, scopes)
        }
        Expr::If {
            cond,
            then,
            els,
            span,
        } => {
            if !span_contains(*span, offset) {
                return None;
            }
            visit_expr(cond, offset, top, scopes)
                .or_else(|| visit_block(then, offset, top, scopes))
                .or_else(|| {
                    els.as_ref()
                        .and_then(|e| visit_expr(e, offset, top, scopes))
                })
        }
        Expr::Match {
            subject,
            arms,
            span,
        } => {
            if !span_contains(*span, offset) {
                return None;
            }
            if let Some(r) = visit_expr(subject, offset, top, scopes) {
                return Some(r);
            }
            for arm in arms {
                if !span_contains(arm.span, offset) {
                    continue;
                }
                let mut arm_scope = BTreeMap::new();
                if let Some(r) = visit_pattern(&arm.pattern, offset, top, &mut arm_scope) {
                    return Some(r);
                }
                scopes.push(arm_scope);
                let res = visit_expr(&arm.body, offset, top, scopes);
                scopes.pop();
                if res.is_some() {
                    return res;
                }
            }
            None
        }
        Expr::Block(block) => visit_block(block, offset, top, scopes),
    }
}

fn visit_pattern(
    pat: &Pattern,
    offset: usize,
    top: &TopEnv,
    arm_scope: &mut BTreeMap<String, Entry>,
) -> Option<Resolved> {
    match pat {
        Pattern::Wild { .. } | Pattern::None { .. } => None,
        Pattern::Int { span, .. } => {
            span_contains(*span, offset).then(|| lit_resolved("int literal"))
        }
        Pattern::Byte { span, .. } => {
            span_contains(*span, offset).then(|| lit_resolved("byte literal"))
        }
        Pattern::Str { span, .. } => {
            span_contains(*span, offset).then(|| lit_resolved("str literal"))
        }
        Pattern::Bool { span, .. } => {
            span_contains(*span, offset).then(|| lit_resolved("bool literal"))
        }
        Pattern::Bind(id) => {
            let entry = Entry {
                def_span: Some(span_range(id.span)),
                hover: format!("let {}", id.name),
            };
            arm_scope.insert(id.name.clone(), entry.clone());
            span_contains(id.span, offset).then(|| entry_to_resolved(&entry))
        }
        Pattern::Some { inner, .. } => visit_pattern(inner, offset, top, arm_scope),
        Pattern::Variant { name, args, .. } => {
            if span_contains(name.span, offset) {
                return top.variants.get(&name.name).map(entry_to_resolved);
            }
            for a in args {
                if let Some(r) = visit_pattern(a, offset, top, arm_scope) {
                    return Some(r);
                }
            }
            None
        }
        Pattern::Tuple { items, .. } => {
            for item in items {
                if let Some(r) = visit_pattern(item, offset, top, arm_scope) {
                    return Some(r);
                }
            }
            None
        }
    }
}

pub(crate) fn visit_type(ty: &Type, offset: usize, top: &TopEnv) -> Option<Resolved> {
    match ty {
        Type::Int { .. }
        | Type::Bool { .. }
        | Type::Byte { .. }
        | Type::Str { .. }
        | Type::Unit { .. } => None,
        Type::Named(id) => span_contains(id.span, offset)
            .then(|| {
                top.types
                    .get(&id.name)
                    .map(|te| entry_to_resolved(&te.entry))
            })
            .flatten(),
        Type::List { elem, span } => {
            span_contains(*span, offset).then(|| visit_type(elem, offset, top))?
        }
        Type::Tuple { items, span } => {
            if !span_contains(*span, offset) {
                return None;
            }
            items.iter().find_map(|t| visit_type(t, offset, top))
        }
        Type::Option { inner, span } => {
            span_contains(*span, offset).then(|| visit_type(inner, offset, top))?
        }
        Type::Fn {
            params,
            ret,
            uses,
            span,
        } => {
            if !span_contains(*span, offset) {
                return None;
            }
            for p in params {
                if let Some(r) = visit_type(p, offset, top) {
                    return Some(r);
                }
            }
            if let Some(r) = visit_type(ret, offset, top) {
                return Some(r);
            }
            for u in uses {
                if span_contains(u.span, offset) {
                    return top.layers.get(&u.name).map(entry_to_resolved);
                }
            }
            None
        }
    }
}
