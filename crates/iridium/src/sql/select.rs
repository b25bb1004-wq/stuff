//! SELECT and EXPLAIN execution: validation, planning, nested-loop joins, and final pipeline.

use std::collections::BTreeSet;

use crate::sql::ast::{ExprKind, JoinKind, SelectItem, SelectStmt};
use crate::sql::catalog::Catalog;
use crate::sql::codec::encode_key;
use crate::sql::expr::{
    contains_agg, eval_row, to_bool, validate_expr, ColTarget, EvalCtx, RowData, TableRowRef,
    TableScope,
};
use crate::sql::lexer::parse_err;
use crate::sql::planner::{format_plan, plan_table_access, TableAccessPlan};
use crate::sql::probe::probe_table;
use crate::sql::select_group::{
    compare_order_keys, exec_grouped, exec_ungrouped, to_refs, OwnedJoinedRow, OwnedTableRow,
    ProjKind, ProjectedCol, ResultEntry,
};
use crate::sql::{Rows, Value};
use crate::{Error, Pager, Storage};

struct PreparedSelect<'a> {
    ctx: EvalCtx<'a>,
    proj: Vec<ProjectedCol>,
    columns: Vec<String>,
    plans: Vec<TableAccessPlan<'a>>,
    is_agg: bool,
    limit: Option<usize>,
    offset: usize,
}

pub(crate) fn exec_explain(cat: &Catalog, stmt: &SelectStmt, raw_sql: &str) -> Result<Rows, Error> {
    let prep = prepare_select(cat, stmt, raw_sql)?;
    let rows = prep
        .ctx
        .tables
        .iter()
        .zip(prep.plans.iter())
        .map(|(scope, plan)| vec![Value::Text(format_plan(scope.def, plan))])
        .collect();
    Ok(Rows {
        columns: vec!["plan".to_owned()],
        rows,
        changes: 0,
    })
}

pub(crate) fn exec_select<S: Storage>(
    pager: &mut Pager<S>,
    cat: &Catalog,
    stmt: &SelectStmt,
    raw_sql: &str,
) -> Result<Rows, Error> {
    let prep = prepare_select(cat, stmt, raw_sql)?;
    let joined_rows = build_joined_rows(pager, stmt, &prep.ctx, &prep.plans)?;

    let mut result_entries: Vec<ResultEntry> = if prep.is_agg {
        exec_grouped(stmt, &prep.proj, &prep.ctx, &joined_rows)?
    } else {
        exec_ungrouped(stmt, &prep.proj, &prep.ctx, &joined_rows)?
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
    if prep.offset > 0 {
        if prep.offset >= rows.len() {
            rows.clear();
        } else {
            rows.drain(..prep.offset);
        }
    }
    if let Some(lim) = prep.limit {
        rows.truncate(lim);
    }

    Ok(Rows {
        columns: prep.columns,
        rows,
        changes: 0,
    })
}

fn prepare_select<'a>(
    cat: &'a Catalog,
    stmt: &'a SelectStmt,
    raw_sql: &str,
) -> Result<PreparedSelect<'a>, Error> {
    let mut scopes = Vec::new();
    if let Some(from) = &stmt.from {
        let tbl = cat.table(&from.table.name)?;
        let alias = from.table.alias.as_deref().unwrap_or(&tbl.name);
        scopes.push(TableScope { def: tbl, alias });
        for join in &from.joins {
            let jtbl = cat.table(&join.table.name)?;
            let jalias = join.table.alias.as_deref().unwrap_or(&jtbl.name);
            scopes.push(TableScope {
                def: jtbl,
                alias: jalias,
            });
        }
    }
    let ctx = EvalCtx { tables: scopes };

    if let Some(from) = &stmt.from {
        for (j_idx, join) in from.joins.iter().enumerate() {
            let on_ctx = EvalCtx {
                tables: ctx.tables[..=j_idx + 1].to_vec(),
            };
            validate_expr(&join.on, &on_ctx, false)?;
        }
    }

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
        if let ProjKind::Expr(e) = &p.kind {
            validate_expr(e, &ctx, true)?;
        }
    }

    let is_agg = !stmt.group_by.is_empty()
        || stmt.having.is_some()
        || proj
            .iter()
            .any(|p| matches!(&p.kind, ProjKind::Expr(e) if contains_agg(e)))
        || stmt.order_by.iter().any(|o| contains_agg(&o.expr));

    validate_order_by(stmt, &proj, &ctx, is_agg)?;
    let (limit, offset) = eval_limit_offset(stmt)?;

    let mut plans = Vec::with_capacity(ctx.tables.len());
    if let Some(from) = &stmt.from {
        let t0 = ctx.tables[0].def;
        plans.push(plan_table_access(
            t0,
            0,
            cat.table_indexes(&t0.name),
            stmt.where_clause.as_ref(),
            &ctx,
        )?);
        for (j_idx, join) in from.joins.iter().enumerate() {
            let tbl_idx = j_idx + 1;
            let on_ctx = EvalCtx {
                tables: ctx.tables[..=tbl_idx].to_vec(),
            };
            let tj = ctx.tables[tbl_idx].def;
            plans.push(plan_table_access(
                tj,
                tbl_idx,
                cat.table_indexes(&tj.name),
                Some(&join.on),
                &on_ctx,
            )?);
        }
    }

    Ok(PreparedSelect {
        ctx,
        proj,
        columns,
        plans,
        is_agg,
        limit,
        offset,
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
                if ctx.tables.is_empty() {
                    return Err(parse_err(*pos, "no tables used for *"));
                }
                let mut matched_any = false;
                for (table_idx, scope) in ctx.tables.iter().enumerate() {
                    if let Some(q) = tbl_qual {
                        if !q.eq_ignore_ascii_case(scope.alias) {
                            continue;
                        }
                    }
                    matched_any = true;
                    for (col_idx, col) in scope.def.columns.iter().enumerate() {
                        out.push(ProjectedCol {
                            name: col.name.clone(),
                            alias: None,
                            kind: ProjKind::TableCol { table_idx, col_idx },
                        });
                    }
                }
                if !matched_any {
                    return Err(Error::NoSuchTable);
                }
            }
            SelectItem::Expr { expr, alias } => {
                let name = if let Some(a) = alias {
                    a.clone()
                } else if let ExprKind::Column { table, name, pos } = &expr.kind {
                    let (tbl_idx, target) = ctx.resolve_col(table.as_deref(), name, *pos)?;
                    match target {
                        ColTarget::Column(col_idx) => {
                            ctx.tables[tbl_idx].def.columns[col_idx].name.clone()
                        }
                        ColTarget::Rowid => name.clone(),
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
                    kind: ProjKind::Expr(expr.clone()),
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
    let empty_ctx = EvalCtx::empty();
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

fn build_joined_rows<S: Storage>(
    pager: &mut Pager<S>,
    stmt: &SelectStmt,
    ctx: &EvalCtx<'_>,
    plans: &[TableAccessPlan<'_>],
) -> Result<Vec<OwnedJoinedRow>, Error> {
    let Some(from) = &stmt.from else {
        if let Some(w) = &stmt.where_clause {
            let rd = RowData { tables: &[] };
            if to_bool(&eval_row(w, ctx, Some(&rd))?) != Some(true) {
                return Ok(Vec::new());
            }
        }
        return Ok(vec![Vec::new()]);
    };

    let t0_rows = probe_table(pager, ctx.tables[0].def, 0, &plans[0], ctx, &[])?;
    let mut current: Vec<OwnedJoinedRow> = t0_rows
        .into_iter()
        .map(|(rowid, cols)| {
            vec![OwnedTableRow {
                rowid: Some(rowid),
                cols,
            }]
        })
        .collect();

    for (j_idx, join) in from.joins.iter().enumerate() {
        if current.is_empty() {
            break;
        }
        let tbl_idx = j_idx + 1;
        let scope_ctx = EvalCtx {
            tables: ctx.tables[..=tbl_idx].to_vec(),
        };
        let right_def = ctx.tables[tbl_idx].def;
        let plan = &plans[tbl_idx];
        let cached_scan = if matches!(plan, TableAccessPlan::Scan) {
            Some(probe_table(
                pager,
                right_def,
                tbl_idx,
                plan,
                &scope_ctx,
                &[],
            )?)
        } else {
            None
        };

        let mut next = Vec::new();
        for left in &current {
            let left_refs = to_refs(left);
            let probed;
            let cand_slice = if let Some(ref rows) = cached_scan {
                rows.as_slice()
            } else {
                probed = probe_table(pager, right_def, tbl_idx, plan, &scope_ctx, &left_refs)?;
                &probed
            };

            let mut matched = false;
            for (r_rowid, r_cols) in cand_slice {
                let mut combined_refs = Vec::with_capacity(tbl_idx + 1);
                combined_refs.extend_from_slice(&left_refs);
                combined_refs.push(TableRowRef {
                    rowid: Some(*r_rowid),
                    cols: r_cols,
                });
                let rd = RowData {
                    tables: &combined_refs,
                };
                if to_bool(&eval_row(&join.on, &scope_ctx, Some(&rd))?) == Some(true) {
                    matched = true;
                    let mut new_row = left.clone();
                    new_row.push(OwnedTableRow {
                        rowid: Some(*r_rowid),
                        cols: r_cols.clone(),
                    });
                    next.push(new_row);
                }
            }
            if !matched && join.kind == JoinKind::Left {
                let mut new_row = left.clone();
                new_row.push(OwnedTableRow {
                    rowid: None,
                    cols: vec![Value::Null; right_def.columns.len()],
                });
                next.push(new_row);
            }
        }
        current = next;
    }

    if let Some(w) = &stmt.where_clause {
        let mut filtered = Vec::with_capacity(current.len());
        for row in current {
            let refs = to_refs(&row);
            let rd = RowData { tables: &refs };
            if to_bool(&eval_row(w, ctx, Some(&rd))?) == Some(true) {
                filtered.push(row);
            }
        }
        current = filtered;
    }

    Ok(current)
}
