//! Single-table SELECT execution: scans, WHERE, GROUP BY, HAVING, DISTINCT, ORDER BY, LIMIT.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use crate::sql::ast::{Expr, ExprKind, SelectItem, SelectStmt, Span};
use crate::sql::catalog::Catalog;
use crate::sql::codec::{cmp_values, decode_record, decode_rowid_key, encode_key};
use crate::sql::expr::{
    contains_agg, eval_group, eval_row, to_bool, validate_expr, EvalCtx, RowData,
};
use crate::sql::lexer::parse_err;
use crate::sql::{Rows, Value};
use crate::{Error, Pager, Storage, Tree};

type ResultEntry = (Vec<Value>, Vec<Value>);

struct OwnedRow {
    rowid: i64,
    cols: Vec<Value>,
}

struct ProjectedCol {
    name: String,
    alias: Option<String>,
    expr: Expr,
}

pub(crate) fn exec_select<S: Storage>(
    pager: &mut Pager<S>,
    cat: &Catalog,
    stmt: &SelectStmt,
    raw_sql: &str,
) -> Result<Rows, Error> {
    if let Some(from) = &stmt.from {
        if !from.joins.is_empty() {
            return Err(Error::Misuse);
        }
    }

    let table_info = match &stmt.from {
        Some(from) => {
            let tbl = cat.table(&from.table.name)?;
            let alias = from.table.alias.as_deref().unwrap_or(&tbl.name);
            Some((tbl, alias))
        }
        None => None,
    };
    let ctx = EvalCtx { table: table_info };

    let proj = build_projections(stmt, &ctx, raw_sql)?;
    let columns: Vec<String> = proj.iter().map(|p| p.name.clone()).collect();

    if let Some(w) = &stmt.where_clause {
        validate_expr(w, &ctx, false)?;
    }
    for g in &stmt.group_by {
        validate_expr(g, &ctx, false)?;
    }
    if let Some(h) = &stmt.having {
        validate_expr(h, &ctx, true)?;
    }
    for p in &proj {
        validate_expr(&p.expr, &ctx, true)?;
    }

    let is_agg = !stmt.group_by.is_empty()
        || stmt.having.is_some()
        || proj.iter().any(|p| contains_agg(&p.expr))
        || stmt.order_by.iter().any(|o| contains_agg(&o.expr));

    validate_order_by(stmt, &proj, &ctx, is_agg)?;

    let (limit, offset) = eval_limit_offset(stmt)?;

    let filtered_rows = scan_and_filter(pager, table_info.map(|(t, _)| t.root), &ctx, stmt)?;

    let mut result_entries: Vec<ResultEntry> = if is_agg {
        exec_grouped(stmt, &proj, &ctx, &filtered_rows)?
    } else {
        exec_ungrouped(stmt, &proj, &ctx, &filtered_rows)?
    };

    if stmt.distinct {
        let mut seen = BTreeSet::new();
        result_entries.retain(|(row, _)| seen.insert(encode_key(row)));
    }

    if !stmt.order_by.is_empty() {
        let descs: Vec<bool> = stmt.order_by.iter().map(|o| o.desc).collect();
        result_entries.sort_by(|a, b| compare_order_keys(&a.1, &b.1, &descs));
    }

    let mut rows: Vec<Vec<Value>> = result_entries.into_iter().map(|(r, _)| r).collect();
    if offset > 0 {
        if offset >= rows.len() {
            rows.clear();
        } else {
            rows.drain(..offset);
        }
    }
    if let Some(lim) = limit {
        rows.truncate(lim);
    }

    Ok(Rows {
        columns,
        rows,
        changes: 0,
    })
}

fn build_projections(
    stmt: &SelectStmt,
    ctx: &EvalCtx<'_>,
    raw_sql: &str,
) -> Result<Vec<ProjectedCol>, Error> {
    let mut out = Vec::new();
    for item in &stmt.items {
        match item {
            SelectItem::Star {
                table: tbl_qual,
                pos,
            } => {
                let Some((tbl, alias)) = ctx.table else {
                    return Err(parse_err(*pos, "no tables used for *"));
                };
                if let Some(q) = tbl_qual {
                    if !q.eq_ignore_ascii_case(alias) {
                        return Err(Error::NoSuchTable);
                    }
                }
                for col in &tbl.columns {
                    out.push(ProjectedCol {
                        name: col.name.clone(),
                        alias: None,
                        expr: Expr {
                            kind: ExprKind::Column {
                                table: None,
                                name: col.name.clone(),
                                pos: *pos,
                            },
                            span: Span {
                                start: *pos,
                                end: *pos + 1,
                            },
                        },
                    });
                }
            }
            SelectItem::Expr { expr, alias } => {
                let name = if let Some(a) = alias {
                    a.clone()
                } else if let ExprKind::Column { name, .. } = &expr.kind {
                    if let Some((tbl, _)) = ctx.table {
                        if let Some(col) = tbl
                            .columns
                            .iter()
                            .find(|c| c.name.eq_ignore_ascii_case(name))
                        {
                            col.name.clone()
                        } else {
                            name.clone()
                        }
                    } else {
                        name.clone()
                    }
                } else {
                    raw_sql
                        .get(expr.span.start..expr.span.end)
                        .unwrap_or("")
                        .trim()
                        .to_owned()
                };
                out.push(ProjectedCol {
                    name,
                    alias: alias.clone(),
                    expr: expr.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn validate_order_by(
    stmt: &SelectStmt,
    proj: &[ProjectedCol],
    ctx: &EvalCtx<'_>,
    is_agg: bool,
) -> Result<(), Error> {
    for item in &stmt.order_by {
        if let ExprKind::IntLit(digits) = &item.expr.kind {
            let pos = digits
                .parse::<usize>()
                .map_err(|_| parse_err(item.expr.span.start, "ORDER BY index out of range"))?;
            if pos == 0 || pos > proj.len() {
                return Err(parse_err(
                    item.expr.span.start,
                    "ORDER BY index out of range",
                ));
            }
            continue;
        }
        if let ExprKind::Column {
            table: None, name, ..
        } = &item.expr.kind
        {
            if proj.iter().any(|p| {
                p.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(name))
            }) {
                continue;
            }
        }
        validate_expr(&item.expr, ctx, is_agg)?;
    }
    Ok(())
}

fn eval_limit_offset(stmt: &SelectStmt) -> Result<(Option<usize>, usize), Error> {
    let empty_ctx = EvalCtx { table: None };
    let limit = match &stmt.limit {
        None => None,
        Some(expr) => {
            validate_expr(expr, &empty_ctx, false)?;
            let v = eval_row(expr, &empty_ctx, None)?;
            match parse_limit_int(&v)? {
                None => None,
                Some(n) if n < 0 => None,
                Some(n) => Some(usize::try_from(n).unwrap_or(usize::MAX)),
            }
        }
    };
    let offset = match &stmt.offset {
        None => 0,
        Some(expr) => {
            validate_expr(expr, &empty_ctx, false)?;
            let v = eval_row(expr, &empty_ctx, None)?;
            match parse_limit_int(&v)? {
                None => 0,
                Some(n) if n <= 0 => 0,
                Some(n) => usize::try_from(n).unwrap_or(usize::MAX),
            }
        }
    };
    Ok((limit, offset))
}

fn parse_limit_int(v: &Value) -> Result<Option<i64>, Error> {
    match v {
        Value::Null => Ok(None),
        Value::Int(n) => Ok(Some(*n)),
        Value::Text(s) => s.trim().parse::<i64>().map(Some).map_err(|_| Error::Type),
        Value::Blob(_) => Err(Error::Type),
    }
}

fn scan_and_filter<S: Storage>(
    pager: &mut Pager<S>,
    table_root: Option<u64>,
    ctx: &EvalCtx<'_>,
    stmt: &SelectStmt,
) -> Result<Vec<OwnedRow>, Error> {
    let Some(root) = table_root else {
        let empty_row = OwnedRow {
            rowid: 0,
            cols: Vec::new(),
        };
        if let Some(w) = &stmt.where_clause {
            let rd = RowData {
                rowid: 0,
                cols: &[],
            };
            if to_bool(&eval_row(w, ctx, Some(&rd))?) != Some(true) {
                return Ok(Vec::new());
            }
        }
        return Ok(vec![empty_row]);
    };

    let tree = Tree::open(root);
    let mut rows = Vec::new();
    let mut scan_err = None;
    tree.scan(pager, None, Bound::Unbounded, &mut |k, v| {
        let res = (|| -> Result<(), Error> {
            let rowid = decode_rowid_key(k)?;
            let cols = decode_record(v)?;
            let keep = match &stmt.where_clause {
                None => true,
                Some(w) => {
                    let rd = RowData { rowid, cols: &cols };
                    to_bool(&eval_row(w, ctx, Some(&rd))?) == Some(true)
                }
            };
            if keep {
                rows.push(OwnedRow { rowid, cols });
            }
            Ok(())
        })();
        if let Err(e) = res {
            scan_err = Some(e);
            return false;
        }
        true
    })?;
    if let Some(e) = scan_err {
        return Err(e);
    }
    Ok(rows)
}

fn exec_ungrouped(
    stmt: &SelectStmt,
    proj: &[ProjectedCol],
    ctx: &EvalCtx<'_>,
    rows: &[OwnedRow],
) -> Result<Vec<ResultEntry>, Error> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let rd = RowData {
            rowid: row.rowid,
            cols: &row.cols,
        };
        let mut out_row = Vec::with_capacity(proj.len());
        for p in proj {
            out_row.push(eval_row(&p.expr, ctx, Some(&rd))?);
        }
        let order_keys = eval_order_keys(stmt, proj, ctx, std::slice::from_ref(&rd), &out_row)?;
        out.push((out_row, order_keys));
    }
    Ok(out)
}

fn exec_grouped(
    stmt: &SelectStmt,
    proj: &[ProjectedCol],
    ctx: &EvalCtx<'_>,
    rows: &[OwnedRow],
) -> Result<Vec<ResultEntry>, Error> {
    let groups: Vec<Vec<RowData<'_>>> = if stmt.group_by.is_empty() {
        let rds: Vec<RowData<'_>> = rows
            .iter()
            .map(|r| RowData {
                rowid: r.rowid,
                cols: &r.cols,
            })
            .collect();
        vec![rds]
    } else {
        let mut map: BTreeMap<Vec<u8>, Vec<RowData<'_>>> = BTreeMap::new();
        for r in rows {
            let rd = RowData {
                rowid: r.rowid,
                cols: &r.cols,
            };
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
            out_row.push(eval_group(&p.expr, ctx, group)?);
        }
        let order_keys = eval_order_keys(stmt, proj, ctx, group, &out_row)?;
        out.push((out_row, order_keys));
    }
    Ok(out)
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

fn compare_order_keys(a: &[Value], b: &[Value], descs: &[bool]) -> Ordering {
    for ((va, vb), &desc) in a.iter().zip(b.iter()).zip(descs.iter()) {
        let ord = cmp_values(va, vb);
        if ord != Ordering::Equal {
            return if desc { ord.reverse() } else { ord };
        }
    }
    Ordering::Equal
}
