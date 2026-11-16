//! Expression validation, column binding, and aggregate detection.

use crate::sql::ast::{CallArgs, Expr, ExprKind};
use crate::sql::expr::EvalCtx;
use crate::sql::lexer::parse_err;
use crate::Error;

pub(crate) fn validate_expr(expr: &Expr, ctx: &EvalCtx<'_>, allow_agg: bool) -> Result<(), Error> {
    match &expr.kind {
        ExprKind::Null
        | ExprKind::IntLit(_)
        | ExprKind::RealLit(_)
        | ExprKind::Text(_)
        | ExprKind::Blob(_) => Ok(()),
        ExprKind::Column { table, name, pos } => {
            ctx.resolve_col(table.as_deref(), name, *pos)?;
            Ok(())
        }
        ExprKind::Paren(inner) | ExprKind::IsNull { expr: inner, .. } => {
            validate_expr(inner, ctx, allow_agg)
        }
        ExprKind::Unary { expr: inner, .. } => validate_expr(inner, ctx, allow_agg),
        ExprKind::Binary { left, right, .. } => {
            validate_expr(left, ctx, allow_agg)?;
            validate_expr(right, ctx, allow_agg)
        }
        ExprKind::Between {
            expr: e, low, high, ..
        } => {
            validate_expr(e, ctx, allow_agg)?;
            validate_expr(low, ctx, allow_agg)?;
            validate_expr(high, ctx, allow_agg)
        }
        ExprKind::InList { expr: e, list, .. } => {
            validate_expr(e, ctx, allow_agg)?;
            for item in list {
                validate_expr(item, ctx, allow_agg)?;
            }
            Ok(())
        }
        ExprKind::Like {
            expr: e, pattern, ..
        } => {
            validate_expr(e, ctx, allow_agg)?;
            validate_expr(pattern, ctx, allow_agg)
        }
        ExprKind::Case {
            operand,
            whens,
            else_expr,
        } => {
            if let Some(op) = operand {
                validate_expr(op, ctx, allow_agg)?;
            }
            for (w, t) in whens {
                validate_expr(w, ctx, allow_agg)?;
                validate_expr(t, ctx, allow_agg)?;
            }
            if let Some(el) = else_expr {
                validate_expr(el, ctx, allow_agg)?;
            }
            Ok(())
        }
        ExprKind::Call { name, args, pos } => validate_call(name, args, *pos, ctx, allow_agg),
    }
}

fn validate_call(
    name: &str,
    args: &CallArgs,
    pos: usize,
    ctx: &EvalCtx<'_>,
    allow_agg: bool,
) -> Result<(), Error> {
    if is_agg_name(name) {
        if !allow_agg {
            return Err(parse_err(pos, "aggregate function not allowed here"));
        }
        match args {
            CallArgs::Star => {
                if !name.eq_ignore_ascii_case("count") {
                    return Err(parse_err(pos, "only count(*) accepts *"));
                }
                Ok(())
            }
            CallArgs::List(list) => {
                if list.len() != 1 {
                    return Err(parse_err(pos, "aggregate expects 1 argument"));
                }
                validate_expr(&list[0], ctx, false)
            }
        }
    } else {
        let CallArgs::List(list) = args else {
            return Err(parse_err(pos, "* not allowed in scalar function"));
        };
        let valid_arity = if name.eq_ignore_ascii_case("abs")
            || name.eq_ignore_ascii_case("length")
            || name.eq_ignore_ascii_case("lower")
            || name.eq_ignore_ascii_case("upper")
            || name.eq_ignore_ascii_case("typeof")
        {
            list.len() == 1
        } else if name.eq_ignore_ascii_case("coalesce") {
            list.len() >= 2
        } else if name.eq_ignore_ascii_case("substr") {
            list.len() == 2 || list.len() == 3
        } else {
            return Err(parse_err(pos, format!("unknown function {name}")));
        };
        if !valid_arity {
            return Err(parse_err(
                pos,
                format!("wrong number of arguments to {name}"),
            ));
        }
        for arg in list {
            validate_expr(arg, ctx, allow_agg)?;
        }
        Ok(())
    }
}

pub(crate) fn is_agg_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("count")
        || name.eq_ignore_ascii_case("sum")
        || name.eq_ignore_ascii_case("avg")
        || name.eq_ignore_ascii_case("min")
        || name.eq_ignore_ascii_case("max")
}

pub(crate) fn contains_agg(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Null
        | ExprKind::IntLit(_)
        | ExprKind::RealLit(_)
        | ExprKind::Text(_)
        | ExprKind::Blob(_)
        | ExprKind::Column { .. } => false,
        ExprKind::Paren(e) | ExprKind::Unary { expr: e, .. } | ExprKind::IsNull { expr: e, .. } => {
            contains_agg(e)
        }
        ExprKind::Binary { left, right, .. } => contains_agg(left) || contains_agg(right),
        ExprKind::Between {
            expr: e, low, high, ..
        } => contains_agg(e) || contains_agg(low) || contains_agg(high),
        ExprKind::InList { expr: e, list, .. } => contains_agg(e) || list.iter().any(contains_agg),
        ExprKind::Like {
            expr: e, pattern, ..
        } => contains_agg(e) || contains_agg(pattern),
        ExprKind::Case {
            operand,
            whens,
            else_expr,
        } => {
            operand.as_deref().is_some_and(contains_agg)
                || whens
                    .iter()
                    .any(|(w, t)| contains_agg(w) || contains_agg(t))
                || else_expr.as_deref().is_some_and(contains_agg)
        }
        ExprKind::Call { name, args, .. } => {
            if is_agg_name(name) {
                true
            } else if let CallArgs::List(list) = args {
                list.iter().any(contains_agg)
            } else {
                false
            }
        }
    }
}
