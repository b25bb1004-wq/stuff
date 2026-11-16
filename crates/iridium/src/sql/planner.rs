//! Query planner and EXPLAIN formatting for Iridium SQL.

use crate::sql::ast::{BinaryOp, Expr, ExprKind};
use crate::sql::catalog::{IndexDef, TableDef};
use crate::sql::expr::{ColTarget, EvalCtx};
use crate::Error;

#[derive(Debug, Clone, Copy)]
pub(crate) enum RangeOp {
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RangeTerm<'a> {
    Cmp(RangeOp, &'a Expr),
    Between(&'a Expr, &'a Expr),
}

#[derive(Debug, Clone)]
pub(crate) enum TableAccessPlan<'a> {
    Scan,
    Rowid {
        expr: &'a Expr,
    },
    Index {
        index: &'a IndexDef,
        eq_terms: Vec<(usize, &'a Expr)>,
        range: Option<(usize, RangeTerm<'a>)>,
    },
}

struct ExtractedTerms<'a> {
    eqs: Vec<(ColTarget, &'a Expr)>,
    ranges: Vec<(ColTarget, RangeTerm<'a>)>,
}

pub(crate) fn plan_table_access<'a>(
    table: &'a TableDef,
    table_idx: usize,
    indexes: Vec<&'a IndexDef>,
    filter_expr: Option<&'a Expr>,
    ctx: &EvalCtx<'a>,
) -> Result<TableAccessPlan<'a>, Error> {
    let Some(expr) = filter_expr else {
        return Ok(TableAccessPlan::Scan);
    };
    let mut raw_terms = Vec::new();
    collect_and_terms(expr, &mut raw_terms);

    let mut terms = ExtractedTerms {
        eqs: Vec::new(),
        ranges: Vec::new(),
    };
    for t in raw_terms {
        extract_term(t, ctx, table_idx, &mut terms)?;
    }

    for &(target, rhs) in &terms.eqs {
        let is_rowid = match target {
            ColTarget::Rowid => true,
            ColTarget::Column(ci) => table.ipk_col == Some(ci),
        };
        if is_rowid {
            return Ok(TableAccessPlan::Rowid { expr: rhs });
        }
    }

    for idx in indexes {
        let mut eq_terms = Vec::new();
        let mut range = None;
        for &col_i in &idx.col_indices {
            if let Some(&(_, eq_expr)) = terms
                .eqs
                .iter()
                .find(|(t, _)| *t == ColTarget::Column(col_i))
            {
                eq_terms.push((col_i, eq_expr));
            } else {
                if let Some(&(_, r)) = terms
                    .ranges
                    .iter()
                    .find(|(t, _)| *t == ColTarget::Column(col_i))
                {
                    range = Some((col_i, r));
                }
                break;
            }
        }
        if !eq_terms.is_empty() || range.is_some() {
            return Ok(TableAccessPlan::Index {
                index: idx,
                eq_terms,
                range,
            });
        }
    }

    Ok(TableAccessPlan::Scan)
}

pub(crate) fn format_plan(table: &TableDef, plan: &TableAccessPlan<'_>) -> String {
    match plan {
        TableAccessPlan::Scan => format!("SCAN {}", table.name),
        TableAccessPlan::Rowid { .. } => format!("SEARCH {} USING ROWID", table.name),
        TableAccessPlan::Index {
            index,
            eq_terms,
            range,
        } => {
            let mut parts = Vec::with_capacity(eq_terms.len() + 1);
            for &(col_i, _) in eq_terms {
                let cname = &table.columns[col_i].name;
                parts.push(format!("{cname}=?"));
            }
            if let Some((col_i, r)) = range {
                let cname = &table.columns[*col_i].name;
                let s = match r {
                    RangeTerm::Cmp(RangeOp::Lt, _) => format!("{cname}<?"),
                    RangeTerm::Cmp(RangeOp::Le, _) => format!("{cname}<=?"),
                    RangeTerm::Cmp(RangeOp::Gt, _) => format!("{cname}>?"),
                    RangeTerm::Cmp(RangeOp::Ge, _) => format!("{cname}>=?"),
                    RangeTerm::Between(_, _) => format!("{cname}>? AND {cname}<?"),
                };
                parts.push(s);
            }
            format!(
                "SEARCH {} USING INDEX {} ({})",
                table.name,
                index.name,
                parts.join(" AND ")
            )
        }
    }
}

fn collect_and_terms<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    let e = unwrap_parens(expr);
    if let ExprKind::Binary {
        op: BinaryOp::And,
        left,
        right,
    } = &e.kind
    {
        collect_and_terms(left, out);
        collect_and_terms(right, out);
    } else {
        out.push(e);
    }
}

fn unwrap_parens(mut expr: &Expr) -> &Expr {
    while let ExprKind::Paren(inner) = &expr.kind {
        expr = inner;
    }
    expr
}

fn extract_term<'a>(
    term: &'a Expr,
    ctx: &EvalCtx<'a>,
    table_idx: usize,
    out: &mut ExtractedTerms<'a>,
) -> Result<(), Error> {
    match &term.kind {
        ExprKind::Binary { op, left, right } => match op {
            BinaryOp::Eq => {
                if let Some(target) = as_table_col(left, ctx, table_idx)? {
                    if expr_uses_only_outer(right, ctx, table_idx)? {
                        out.eqs.push((target, right));
                        return Ok(());
                    }
                }
                if let Some(target) = as_table_col(right, ctx, table_idx)? {
                    if expr_uses_only_outer(left, ctx, table_idx)? {
                        out.eqs.push((target, left));
                    }
                }
                Ok(())
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let rop = to_range_op(*op);
                if let Some(target) = as_table_col(left, ctx, table_idx)? {
                    if expr_uses_only_outer(right, ctx, table_idx)? {
                        out.ranges.push((target, RangeTerm::Cmp(rop, right)));
                        return Ok(());
                    }
                }
                if let Some(target) = as_table_col(right, ctx, table_idx)? {
                    if expr_uses_only_outer(left, ctx, table_idx)? {
                        out.ranges
                            .push((target, RangeTerm::Cmp(flip_range_op(rop), left)));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        },
        ExprKind::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
            if let Some(target) = as_table_col(expr, ctx, table_idx)? {
                if expr_uses_only_outer(low, ctx, table_idx)?
                    && expr_uses_only_outer(high, ctx, table_idx)?
                {
                    out.ranges.push((target, RangeTerm::Between(low, high)));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn to_range_op(op: BinaryOp) -> RangeOp {
    match op {
        BinaryOp::Lt => RangeOp::Lt,
        BinaryOp::Le => RangeOp::Le,
        BinaryOp::Gt => RangeOp::Gt,
        BinaryOp::Ge => RangeOp::Ge,
        _ => RangeOp::Lt,
    }
}

fn flip_range_op(op: RangeOp) -> RangeOp {
    match op {
        RangeOp::Lt => RangeOp::Gt,
        RangeOp::Le => RangeOp::Ge,
        RangeOp::Gt => RangeOp::Lt,
        RangeOp::Ge => RangeOp::Le,
    }
}

fn as_table_col(
    expr: &Expr,
    ctx: &EvalCtx<'_>,
    table_idx: usize,
) -> Result<Option<ColTarget>, Error> {
    let e = unwrap_parens(expr);
    if let ExprKind::Column { table, name, pos } = &e.kind {
        let (idx, target) = ctx.resolve_col(table.as_deref(), name, *pos)?;
        if idx == table_idx {
            return Ok(Some(target));
        }
    }
    Ok(None)
}

fn expr_uses_only_outer(expr: &Expr, ctx: &EvalCtx<'_>, table_idx: usize) -> Result<bool, Error> {
    match &expr.kind {
        ExprKind::Null
        | ExprKind::IntLit(_)
        | ExprKind::RealLit(_)
        | ExprKind::Text(_)
        | ExprKind::Blob(_) => Ok(true),
        ExprKind::Column { table, name, pos } => {
            let (idx, _) = ctx.resolve_col(table.as_deref(), name, *pos)?;
            Ok(idx < table_idx)
        }
        ExprKind::Paren(inner)
        | ExprKind::Unary { expr: inner, .. }
        | ExprKind::IsNull { expr: inner, .. } => expr_uses_only_outer(inner, ctx, table_idx),
        ExprKind::Binary { left, right, .. } => Ok(expr_uses_only_outer(left, ctx, table_idx)?
            && expr_uses_only_outer(right, ctx, table_idx)?),
        ExprKind::Between {
            expr: e, low, high, ..
        } => Ok(expr_uses_only_outer(e, ctx, table_idx)?
            && expr_uses_only_outer(low, ctx, table_idx)?
            && expr_uses_only_outer(high, ctx, table_idx)?),
        ExprKind::InList { expr: e, list, .. } => {
            if !expr_uses_only_outer(e, ctx, table_idx)? {
                return Ok(false);
            }
            for item in list {
                if !expr_uses_only_outer(item, ctx, table_idx)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        ExprKind::Like {
            expr: e, pattern, ..
        } => Ok(expr_uses_only_outer(e, ctx, table_idx)?
            && expr_uses_only_outer(pattern, ctx, table_idx)?),
        ExprKind::Case {
            operand,
            whens,
            else_expr,
        } => {
            if let Some(op) = operand {
                if !expr_uses_only_outer(op, ctx, table_idx)? {
                    return Ok(false);
                }
            }
            for (w, t) in whens {
                if !expr_uses_only_outer(w, ctx, table_idx)?
                    || !expr_uses_only_outer(t, ctx, table_idx)?
                {
                    return Ok(false);
                }
            }
            if let Some(el) = else_expr {
                if !expr_uses_only_outer(el, ctx, table_idx)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        ExprKind::Call { args, .. } => match args {
            crate::sql::ast::CallArgs::Star => Ok(true),
            crate::sql::ast::CallArgs::List(list) => {
                for item in list {
                    if !expr_uses_only_outer(item, ctx, table_idx)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        },
    }
}
