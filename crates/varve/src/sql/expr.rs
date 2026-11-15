//! Expression evaluation.

use std::cmp::Ordering;

use crate::sql::ast::{BinaryOp, Expr, ExprKind, UnaryOp};
use crate::sql::catalog::TableDef;
use crate::sql::codec::cmp_values;
pub(crate) use crate::sql::functions::to_bool;
use crate::sql::functions::{
    cmp_sql, eval_call, like_match, three_val_and, three_val_not, three_val_or, to_int_coerce,
    value_to_text,
};
pub(crate) use crate::sql::validate::{contains_agg, is_agg_name, validate_expr};
use crate::sql::Value;
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColTarget {
    Column(usize),
    Rowid,
}

pub(crate) struct EvalCtx<'a> {
    pub table: Option<(&'a TableDef, &'a str)>,
}

pub(crate) struct RowData<'a> {
    pub rowid: i64,
    pub cols: &'a [Value],
}

impl<'a> EvalCtx<'a> {
    pub fn resolve_col(&self, tbl: Option<&str>, name: &str) -> Result<ColTarget, Error> {
        let Some((table, alias)) = self.table else {
            return Err(Error::NoSuchColumn);
        };
        if let Some(t) = tbl {
            if !t.eq_ignore_ascii_case(alias) {
                return Err(Error::NoSuchColumn);
            }
        }
        if let Some(idx) = table
            .columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
        {
            return Ok(ColTarget::Column(idx));
        }
        if !table.has_rowid_col && name.eq_ignore_ascii_case("rowid") {
            return Ok(ColTarget::Rowid);
        }
        Err(Error::NoSuchColumn)
    }
}

pub(crate) fn eval_row(
    expr: &Expr,
    ctx: &EvalCtx<'_>,
    row: Option<&RowData<'_>>,
) -> Result<Value, Error> {
    let rows = match row {
        Some(r) => std::slice::from_ref(r),
        None => &[],
    };
    eval_group(expr, ctx, rows)
}

pub(crate) fn eval_group(
    expr: &Expr,
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
) -> Result<Value, Error> {
    match &expr.kind {
        ExprKind::Null => Ok(Value::Null),
        ExprKind::IntLit(digits) => {
            let n = digits.parse::<i64>().map_err(|_| Error::Overflow)?;
            Ok(Value::Int(n))
        }
        ExprKind::Text(s) => Ok(Value::Text(s.clone())),
        ExprKind::Blob(b) => Ok(Value::Blob(b.clone())),
        ExprKind::Column { table, name, .. } => {
            let target = ctx.resolve_col(table.as_deref(), name)?;
            let Some(row) = group.first() else {
                return Ok(Value::Null);
            };
            match target {
                ColTarget::Rowid => Ok(Value::Int(row.rowid)),
                ColTarget::Column(idx) => Ok(row.cols.get(idx).cloned().unwrap_or(Value::Null)),
            }
        }
        ExprKind::Paren(inner) => eval_group(inner, ctx, group),
        ExprKind::Unary { op, expr: inner } => eval_unary(*op, inner, ctx, group),
        ExprKind::Binary { op, left, right } => eval_binary(*op, left, right, ctx, group),
        ExprKind::IsNull {
            expr: inner,
            negated,
        } => {
            let v = eval_group(inner, ctx, group)?;
            let is_null = matches!(v, Value::Null);
            let res = if *negated { !is_null } else { is_null };
            Ok(Value::Int(i64::from(res)))
        }
        ExprKind::Between {
            expr: e,
            low,
            high,
            negated,
        } => {
            let val = eval_group(e, ctx, group)?;
            let lo = eval_group(low, ctx, group)?;
            let hi = eval_group(high, ctx, group)?;
            let ge = cmp_sql(&val, &lo, BinaryOp::Ge);
            let le = cmp_sql(&val, &hi, BinaryOp::Le);
            let in_range = three_val_and(&ge, &le);
            if *negated {
                Ok(three_val_not(&in_range))
            } else {
                Ok(in_range)
            }
        }
        ExprKind::InList {
            expr: e,
            list,
            negated,
        } => {
            if list.is_empty() {
                return Ok(Value::Int(i64::from(*negated)));
            }
            let lhs = eval_group(e, ctx, group)?;
            if matches!(lhs, Value::Null) {
                return Ok(Value::Null);
            }
            let mut saw_null = false;
            for item in list {
                let rhs = eval_group(item, ctx, group)?;
                if matches!(rhs, Value::Null) {
                    saw_null = true;
                } else if cmp_values(&lhs, &rhs) == Ordering::Equal {
                    return Ok(Value::Int(i64::from(!*negated)));
                }
            }
            if saw_null {
                Ok(Value::Null)
            } else {
                Ok(Value::Int(i64::from(*negated)))
            }
        }
        ExprKind::Like {
            expr: e,
            pattern,
            negated,
        } => {
            let val = eval_group(e, ctx, group)?;
            let pat = eval_group(pattern, ctx, group)?;
            if matches!(val, Value::Null) || matches!(pat, Value::Null) {
                return Ok(Value::Null);
            }
            let text_chars: Vec<char> = value_to_text(&val).chars().collect();
            let pat_chars: Vec<char> = value_to_text(&pat).chars().collect();
            let matched = like_match(&text_chars, &pat_chars);
            let res = if *negated { !matched } else { matched };
            Ok(Value::Int(i64::from(res)))
        }
        ExprKind::Case {
            operand,
            whens,
            else_expr,
        } => eval_case(operand.as_deref(), whens, else_expr.as_deref(), ctx, group),
        ExprKind::Call { name, args, pos } => eval_call(name, args, *pos, ctx, group),
    }
}

fn eval_unary(
    op: UnaryOp,
    inner: &Expr,
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
) -> Result<Value, Error> {
    if op == UnaryOp::Neg {
        if let ExprKind::IntLit(digits) = &inner.kind {
            if digits == "9223372036854775808" {
                return Ok(Value::Int(i64::MIN));
            }
        }
    }
    let v = eval_group(inner, ctx, group)?;
    match op {
        UnaryOp::Pos => Ok(v),
        UnaryOp::Not => Ok(three_val_not(&v)),
        UnaryOp::Neg => {
            if matches!(v, Value::Null) {
                return Ok(Value::Null);
            }
            let n = to_int_coerce(&v)?;
            n.checked_neg().map(Value::Int).ok_or(Error::Overflow)
        }
    }
}

fn eval_binary(
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
) -> Result<Value, Error> {
    match op {
        BinaryOp::And => {
            let l = eval_group(left, ctx, group)?;
            if to_bool(&l) == Some(false) {
                return Ok(Value::Int(0));
            }
            let r = eval_group(right, ctx, group)?;
            Ok(three_val_and(&l, &r))
        }
        BinaryOp::Or => {
            let l = eval_group(left, ctx, group)?;
            if to_bool(&l) == Some(true) {
                return Ok(Value::Int(1));
            }
            let r = eval_group(right, ctx, group)?;
            Ok(three_val_or(&l, &r))
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let l = eval_group(left, ctx, group)?;
            let r = eval_group(right, ctx, group)?;
            Ok(cmp_sql(&l, &r, op))
        }
        BinaryOp::Concat => {
            let l = eval_group(left, ctx, group)?;
            let r = eval_group(right, ctx, group)?;
            if matches!(l, Value::Null) || matches!(r, Value::Null) {
                return Ok(Value::Null);
            }
            let mut s = value_to_text(&l);
            s.push_str(&value_to_text(&r));
            Ok(Value::Text(s))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
            let l = eval_group(left, ctx, group)?;
            let r = eval_group(right, ctx, group)?;
            if matches!(l, Value::Null) || matches!(r, Value::Null) {
                return Ok(Value::Null);
            }
            let a = to_int_coerce(&l)?;
            let b = to_int_coerce(&r)?;
            match op {
                BinaryOp::Add => a.checked_add(b).map(Value::Int).ok_or(Error::Overflow),
                BinaryOp::Sub => a.checked_sub(b).map(Value::Int).ok_or(Error::Overflow),
                BinaryOp::Mul => a.checked_mul(b).map(Value::Int).ok_or(Error::Overflow),
                BinaryOp::Div => {
                    if b == 0 {
                        Ok(Value::Null)
                    } else {
                        a.checked_div(b).map(Value::Int).ok_or(Error::Overflow)
                    }
                }
                BinaryOp::Mod => {
                    if b == 0 {
                        Ok(Value::Null)
                    } else if b == -1 {
                        Ok(Value::Int(0))
                    } else {
                        Ok(Value::Int(a % b))
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

fn eval_case(
    operand: Option<&Expr>,
    whens: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
) -> Result<Value, Error> {
    if let Some(base) = operand {
        let base_val = eval_group(base, ctx, group)?;
        for (w, t) in whens {
            let w_val = eval_group(w, ctx, group)?;
            if !matches!(base_val, Value::Null)
                && !matches!(w_val, Value::Null)
                && cmp_values(&base_val, &w_val) == Ordering::Equal
            {
                return eval_group(t, ctx, group);
            }
        }
    } else {
        for (w, t) in whens {
            let w_val = eval_group(w, ctx, group)?;
            if to_bool(&w_val) == Some(true) {
                return eval_group(t, ctx, group);
            }
        }
    }
    match else_expr {
        Some(el) => eval_group(el, ctx, group),
        None => Ok(Value::Null),
    }
}
