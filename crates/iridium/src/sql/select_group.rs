//! Projection, grouping, HAVING, and ORDER BY evaluation for SELECT.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::sql::ast::{Expr, ExprKind, SelectStmt};
use crate::sql::codec::{cmp_values, encode_key};
use crate::sql::expr::{eval_group, eval_row, to_bool, EvalCtx, RowData, TableRowRef};
use crate::sql::lexer::parse_err;
use crate::sql::Value;
use crate::Error;

pub(crate) type ResultEntry = (Vec<Value>, Vec<Value>);

#[derive(Clone)]
pub(crate) struct OwnedTableRow {
    pub rowid: Option<i64>,
    pub cols: Vec<Value>,
}

pub(crate) type OwnedJoinedRow = Vec<OwnedTableRow>;

pub(crate) enum ProjKind {
    TableCol { table_idx: usize, col_idx: usize },
    Expr(Expr),
}

pub(crate) struct ProjectedCol {
    pub name: String,
    pub alias: Option<String>,
    pub kind: ProjKind,
}

pub(crate) fn to_refs(row: &[OwnedTableRow]) -> Vec<TableRowRef<'_>> {
    row.iter()
        .map(|r| TableRowRef {
            rowid: r.rowid,
            cols: &r.cols,
        })
        .collect()
}

pub(crate) fn exec_ungrouped(
    stmt: &SelectStmt,
    proj: &[ProjectedCol],
    ctx: &EvalCtx<'_>,
    rows: &[OwnedJoinedRow],
) -> Result<Vec<ResultEntry>, Error> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let refs = to_refs(row);
        let rd = RowData { tables: &refs };
        let group = std::slice::from_ref(&rd);
        let mut out_row = Vec::with_capacity(proj.len());
        for p in proj {
            out_row.push(eval_proj(p, ctx, group)?);
        }
        let order_keys = eval_order_keys(stmt, proj, ctx, group, &out_row)?;
        out.push((out_row, order_keys));
    }
    Ok(out)
}

pub(crate) fn exec_grouped(
    stmt: &SelectStmt,
    proj: &[ProjectedCol],
    ctx: &EvalCtx<'_>,
    rows: &[OwnedJoinedRow],
) -> Result<Vec<ResultEntry>, Error> {
    let ref_rows: Vec<Vec<TableRowRef<'_>>> = rows.iter().map(|r| to_refs(r)).collect();
    let groups: Vec<Vec<RowData<'_>>> = if stmt.group_by.is_empty() {
        let rds: Vec<RowData<'_>> = ref_rows.iter().map(|t| RowData { tables: t }).collect();
        vec![rds]
    } else {
        let mut map: BTreeMap<Vec<u8>, Vec<RowData<'_>>> = BTreeMap::new();
        for refs in &ref_rows {
            let rd = RowData { tables: refs };
            let mut key_vals = Vec::with_capacity(stmt.group_by.len());
            for g in &stmt.group_by {
                key_vals.push(eval_row(g, ctx, Some(&rd))?);
            }
            map.entry(encode_key(&key_vals)).or_default().push(rd);
        }
        map.into_values().collect()
    };

    let mut out = Vec::with_capacity(groups.len());
    for group in &groups {
        if let Some(h) = &stmt.having {
            if to_bool(&eval_group(h, ctx, group)?) != Some(true) {
                continue;
            }
        }
        let mut out_row = Vec::with_capacity(proj.len());
        for p in proj {
            out_row.push(eval_proj(p, ctx, group)?);
        }
        let order_keys = eval_order_keys(stmt, proj, ctx, group, &out_row)?;
        out.push((out_row, order_keys));
    }
    Ok(out)
}

pub(crate) fn compare_order_keys(a: &[Value], b: &[Value], descs: &[bool]) -> Ordering {
    for ((va, vb), &desc) in a.iter().zip(b.iter()).zip(descs.iter()) {
        let ord = cmp_values(va, vb);
        if ord != Ordering::Equal {
            return if desc { ord.reverse() } else { ord };
        }
    }
    Ordering::Equal
}

fn eval_proj(p: &ProjectedCol, ctx: &EvalCtx<'_>, group: &[RowData<'_>]) -> Result<Value, Error> {
    match &p.kind {
        ProjKind::TableCol { table_idx, col_idx } => {
            let Some(row) = group.first() else {
                return Ok(Value::Null);
            };
            Ok(row
                .tables
                .get(*table_idx)
                .and_then(|t| t.cols.get(*col_idx))
                .cloned()
                .unwrap_or(Value::Null))
        }
        ProjKind::Expr(e) => eval_group(e, ctx, group),
    }
}

fn eval_order_keys(
    stmt: &SelectStmt,
    proj: &[ProjectedCol],
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
    out_row: &[Value],
) -> Result<Vec<Value>, Error> {
    let mut keys = Vec::with_capacity(stmt.order_by.len());
    for item in &stmt.order_by {
        if let ExprKind::IntLit(digits) = &item.expr.kind {
            let idx = digits
                .parse::<usize>()
                .map_err(|_| parse_err(item.expr.span.start, "ORDER BY index out of range"))?;
            let val = out_row
                .get(idx - 1)
                .cloned()
                .ok_or_else(|| parse_err(item.expr.span.start, "ORDER BY index out of range"))?;
            keys.push(val);
            continue;
        }
        if let ExprKind::Column {
            table: None, name, ..
        } = &item.expr.kind
        {
            if let Some(pos) = proj.iter().position(|p| {
                p.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(name))
            }) {
                keys.push(out_row[pos].clone());
                continue;
            }
        }
        keys.push(eval_group(&item.expr, ctx, group)?);
    }
    Ok(keys)
}
