//! DML execution (INSERT, UPDATE, DELETE) with statement-level atomicity.

use std::collections::BTreeSet;
use std::ops::Bound;

use crate::sql::ast::{ColType, DeleteStmt, InsertStmt, UpdateStmt};
use crate::sql::catalog::{Catalog, IndexDef, TableDef};
use crate::sql::codec::{
    decode_record, decode_rowid_key, encode_key, encode_record, extract_trailing_rowid,
};
use crate::sql::expr::{eval_row, to_bool, validate_expr, EvalCtx, RowData};
use crate::sql::lexer::parse_err;
use crate::sql::{Rows, Value};
use crate::{Error, Pager, Storage, Tree};

const MAX_KEY_BYTES: usize = 1024;
const MAX_VAL_BYTES: usize = 1 << 20;

type PlannedRow = (i64, Vec<u8>, Vec<u8>, Vec<Vec<u8>>);

pub(crate) fn exec_insert<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    stmt: &InsertStmt,
) -> Result<Rows, Error> {
    let table = cat.table(&stmt.table)?.clone();
    let indexes: Vec<IndexDef> = cat
        .table_indexes(&table.name)
        .into_iter()
        .cloned()
        .collect();

    let col_map = resolve_insert_columns(&table, stmt.columns.as_deref())?;
    let empty_ctx = EvalCtx { table: None };
    for row_exprs in &stmt.rows {
        if row_exprs.len() != col_map.len() {
            return Err(parse_err(
                0,
                "INSERT row value count does not match columns",
            ));
        }
        for expr in row_exprs {
            validate_expr(expr, &empty_ctx, false)?;
        }
    }

    let mut cur_max = get_max_rowid(pager, &table)?;
    let table_tree = Tree::open(table.root);
    let mut planned_rowids = BTreeSet::new();
    let mut planned_unique: Vec<BTreeSet<Vec<u8>>> = vec![BTreeSet::new(); indexes.len()];
    let mut planned_rows: Vec<PlannedRow> = Vec::with_capacity(stmt.rows.len());

    for row_exprs in &stmt.rows {
        let mut row = vec![Value::Null; table.columns.len()];
        for (&col_idx, expr) in col_map.iter().zip(row_exprs.iter()) {
            row[col_idx] = eval_row(expr, &empty_ctx, None)?;
        }
        validate_row_types_and_not_null(&table, &row, false)?;

        let rowid = if let Some(ipk) = table.ipk_col {
            match row[ipk] {
                Value::Null => {
                    let next = next_rowid(cur_max)?;
                    cur_max = Some(next);
                    row[ipk] = Value::Int(next);
                    next
                }
                Value::Int(r) => {
                    let rkey = encode_key(&[Value::Int(r)]);
                    if !planned_rowids.contains(&r) && table_tree.get(pager, None, &rkey)?.is_none()
                    {
                        cur_max = Some(cur_max.map_or(r, |m| m.max(r)));
                        r
                    } else {
                        return Err(Error::Constraint(format!(
                            "UNIQUE constraint failed: {}.{}",
                            table.name, table.columns[ipk].name
                        )));
                    }
                }
                _ => return Err(Error::Type),
            }
        } else {
            let next = next_rowid(cur_max)?;
            cur_max = Some(next);
            next
        };

        if !planned_rowids.insert(rowid) {
            let cname = table
                .ipk_col
                .map_or("rowid", |i| table.columns[i].name.as_str());
            return Err(Error::Constraint(format!(
                "UNIQUE constraint failed: {}.{}",
                table.name, cname
            )));
        }

        let row_key = encode_key(&[Value::Int(rowid)]);
        let row_rec = encode_record(&row);
        if row_key.len() > MAX_KEY_BYTES || row_rec.len() > MAX_VAL_BYTES {
            return Err(Error::TooLarge);
        }

        let mut idx_keys = Vec::with_capacity(indexes.len());
        for (idx_i, idx) in indexes.iter().enumerate() {
            let (full_key, unique_prefix) = build_index_key(idx, &row, rowid)?;
            if let Some(prefix) = unique_prefix {
                if !planned_unique[idx_i].insert(prefix.clone()) {
                    return Err(Error::Constraint(format!(
                        "UNIQUE constraint failed: {}",
                        idx.name
                    )));
                }
                if index_prefix_exists(pager, idx.root, &prefix, &BTreeSet::new())? {
                    return Err(Error::Constraint(format!(
                        "UNIQUE constraint failed: {}",
                        idx.name
                    )));
                }
            }
            idx_keys.push(full_key);
        }

        planned_rows.push((rowid, row_key, row_rec, idx_keys));
    }

    let changes = planned_rows.len() as u64;
    for (_rowid, row_key, row_rec, idx_keys) in planned_rows {
        table_tree.insert(pager, &row_key, &row_rec)?;
        for (idx, ikey) in indexes.iter().zip(idx_keys.iter()) {
            Tree::open(idx.root).insert(pager, ikey, &[])?;
        }
    }
    if let Ok(t) = cat.table_mut(&stmt.table) {
        t.max_rowid = cur_max;
    }
    Ok(Rows {
        changes,
        ..Default::default()
    })
}

pub(crate) fn exec_update<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    stmt: &UpdateStmt,
) -> Result<Rows, Error> {
    let table = cat.table(&stmt.table)?.clone();
    let indexes: Vec<IndexDef> = cat
        .table_indexes(&table.name)
        .into_iter()
        .cloned()
        .collect();
    let ctx = EvalCtx {
        table: Some((&table, &table.name)),
    };

    let mut assign_cols = Vec::with_capacity(stmt.assignments.len());
    for (cname, _pos, expr) in &stmt.assignments {
        let idx = table
            .columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(cname))
            .ok_or(Error::NoSuchColumn)?;
        validate_expr(expr, &ctx, false)?;
        assign_cols.push((idx, expr));
    }
    if let Some(w) = &stmt.where_clause {
        validate_expr(w, &ctx, false)?;
    }

    let table_tree = Tree::open(table.root);
    let matches = scan_matching_rows(pager, &table_tree, &ctx, stmt.where_clause.as_ref())?;

    let updated_old_rowids: BTreeSet<i64> = matches.iter().map(|(r, _)| *r).collect();
    let mut planned_new_rowids = BTreeSet::new();
    let mut planned_unique: Vec<BTreeSet<Vec<u8>>> = vec![BTreeSet::new(); indexes.len()];
    let mut updates = Vec::with_capacity(matches.len());

    for (old_rowid, old_row) in &matches {
        let mut new_row = old_row.clone();
        let row_data = RowData {
            rowid: *old_rowid,
            cols: old_row,
        };
        for &(col_idx, expr) in &assign_cols {
            new_row[col_idx] = eval_row(expr, &ctx, Some(&row_data))?;
        }
        validate_row_types_and_not_null(&table, &new_row, true)?;

        let new_rowid = if let Some(ipk) = table.ipk_col {
            match new_row[ipk] {
                Value::Int(r) => r,
                _ => return Err(Error::Type),
            }
        } else {
            *old_rowid
        };

        if !planned_new_rowids.insert(new_rowid) {
            let cname = table
                .ipk_col
                .map_or("rowid", |i| table.columns[i].name.as_str());
            return Err(Error::Constraint(format!(
                "UNIQUE constraint failed: {}.{}",
                table.name, cname
            )));
        }
        if !updated_old_rowids.contains(&new_rowid) {
            let rk = encode_key(&[Value::Int(new_rowid)]);
            if table_tree.get(pager, None, &rk)?.is_some() {
                let cname = table
                    .ipk_col
                    .map_or("rowid", |i| table.columns[i].name.as_str());
                return Err(Error::Constraint(format!(
                    "UNIQUE constraint failed: {}.{}",
                    table.name, cname
                )));
            }
        }

        let new_row_key = encode_key(&[Value::Int(new_rowid)]);
        let new_row_rec = encode_record(&new_row);
        if new_row_key.len() > MAX_KEY_BYTES || new_row_rec.len() > MAX_VAL_BYTES {
            return Err(Error::TooLarge);
        }

        let mut old_idx_keys = Vec::with_capacity(indexes.len());
        let mut new_idx_keys = Vec::with_capacity(indexes.len());
        for (idx_i, idx) in indexes.iter().enumerate() {
            let (old_ik, _) = build_index_key(idx, old_row, *old_rowid)?;
            let (new_ik, unique_prefix) = build_index_key(idx, &new_row, new_rowid)?;
            if let Some(prefix) = unique_prefix {
                if !planned_unique[idx_i].insert(prefix.clone()) {
                    return Err(Error::Constraint(format!(
                        "UNIQUE constraint failed: {}",
                        idx.name
                    )));
                }
                if index_prefix_exists(pager, idx.root, &prefix, &updated_old_rowids)? {
                    return Err(Error::Constraint(format!(
                        "UNIQUE constraint failed: {}",
                        idx.name
                    )));
                }
            }
            old_idx_keys.push(old_ik);
            new_idx_keys.push(new_ik);
        }

        updates.push((
            encode_key(&[Value::Int(*old_rowid)]),
            new_row_key,
            new_row_rec,
            old_idx_keys,
            new_idx_keys,
        ));
    }

    let changes = updates.len() as u64;
    for (old_rk, _, _, old_iks, _) in &updates {
        table_tree.remove(pager, old_rk)?;
        for (idx, ik) in indexes.iter().zip(old_iks.iter()) {
            Tree::open(idx.root).remove(pager, ik)?;
        }
    }
    for (_, new_rk, new_rec, _, new_iks) in updates {
        table_tree.insert(pager, &new_rk, &new_rec)?;
        for (idx, ik) in indexes.iter().zip(new_iks.iter()) {
            Tree::open(idx.root).insert(pager, ik, &[])?;
        }
    }
    if changes > 0 {
        if let Ok(t) = cat.table_mut(&stmt.table) {
            t.max_rowid = None;
        }
    }
    Ok(Rows {
        changes,
        ..Default::default()
    })
}

pub(crate) fn exec_delete<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    stmt: &DeleteStmt,
) -> Result<Rows, Error> {
    let table = cat.table(&stmt.table)?.clone();
    let indexes: Vec<IndexDef> = cat
        .table_indexes(&table.name)
        .into_iter()
        .cloned()
        .collect();
    let ctx = EvalCtx {
        table: Some((&table, &table.name)),
    };
    if let Some(w) = &stmt.where_clause {
        validate_expr(w, &ctx, false)?;
    }

    let table_tree = Tree::open(table.root);
    let matches = scan_matching_rows(pager, &table_tree, &ctx, stmt.where_clause.as_ref())?;
    let changes = matches.len() as u64;

    for (rowid, row) in matches {
        let rk = encode_key(&[Value::Int(rowid)]);
        table_tree.remove(pager, &rk)?;
        for idx in &indexes {
            let (ik, _) = build_index_key(idx, &row, rowid)?;
            Tree::open(idx.root).remove(pager, &ik)?;
        }
    }
    if changes > 0 {
        if let Ok(t) = cat.table_mut(&stmt.table) {
            t.max_rowid = None;
        }
    }
    Ok(Rows {
        changes,
        ..Default::default()
    })
}

fn resolve_insert_columns(
    table: &TableDef,
    cols: Option<&[(String, usize)]>,
) -> Result<Vec<usize>, Error> {
    let Some(cols) = cols else {
        return Ok((0..table.columns.len()).collect());
    };
    if cols.is_empty() {
        return Err(parse_err(0, "INSERT column list cannot be empty"));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(cols.len());
    for (name, pos) in cols {
        let idx = table
            .columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
            .ok_or(Error::NoSuchColumn)?;
        if !seen.insert(idx) {
            return Err(parse_err(*pos, format!("duplicate column name {name}")));
        }
        out.push(idx);
    }
    Ok(out)
}

fn validate_row_types_and_not_null(
    table: &TableDef,
    row: &[Value],
    is_update: bool,
) -> Result<(), Error> {
    for (col, val) in table.columns.iter().zip(row.iter()) {
        match (val, col.ty) {
            (Value::Null, _) => {
                if col.not_null || (is_update && col.is_ipk) {
                    return Err(Error::Constraint(format!(
                        "NOT NULL constraint failed: {}.{}",
                        table.name, col.name
                    )));
                }
            }
            (Value::Int(_), ColType::Integer)
            | (Value::Text(_), ColType::Text)
            | (Value::Blob(_), ColType::Blob) => {}
            _ => return Err(Error::Type),
        }
    }
    Ok(())
}

fn get_max_rowid<S: Storage>(pager: &mut Pager<S>, table: &TableDef) -> Result<Option<i64>, Error> {
    if let Some(cached) = table.max_rowid {
        return Ok(if cached == 0 { None } else { Some(cached) });
    }
    let tree = Tree::open(table.root);
    let mut max_id: Option<i64> = None;
    let mut scan_err = None;
    tree.scan(
        pager,
        None,
        Bound::Unbounded,
        &mut |k, _| match decode_rowid_key(k) {
            Ok(id) => {
                max_id = Some(max_id.map_or(id, |m| m.max(id)));
                true
            }
            Err(e) => {
                scan_err = Some(e);
                false
            }
        },
    )?;
    if let Some(e) = scan_err {
        return Err(e);
    }
    Ok(max_id)
}

fn next_rowid(cur_max: Option<i64>) -> Result<i64, Error> {
    match cur_max {
        None => Ok(1),
        Some(m) => m.checked_add(1).ok_or(Error::Overflow),
    }
}

fn build_index_key(
    idx: &IndexDef,
    row: &[Value],
    rowid: i64,
) -> Result<(Vec<u8>, Option<Vec<u8>>), Error> {
    let mut vals = Vec::with_capacity(idx.col_indices.len() + 1);
    let mut has_null = false;
    for &ci in &idx.col_indices {
        let v = row.get(ci).cloned().unwrap_or(Value::Null);
        if matches!(v, Value::Null) {
            has_null = true;
        }
        vals.push(v);
    }
    let unique_prefix = if idx.unique && !has_null {
        Some(encode_key(&vals))
    } else {
        None
    };
    vals.push(Value::Int(rowid));
    let full = encode_key(&vals);
    if full.len() > MAX_KEY_BYTES {
        return Err(Error::TooLarge);
    }
    Ok((full, unique_prefix))
}

fn index_prefix_exists<S: Storage>(
    pager: &mut Pager<S>,
    idx_root: u64,
    prefix: &[u8],
    ignored_rowids: &BTreeSet<i64>,
) -> Result<bool, Error> {
    let tree = Tree::open(idx_root);
    let mut conflict = false;
    let mut scan_err = None;
    tree.scan(pager, None, Bound::Included(prefix), &mut |k, _| {
        if !k.starts_with(prefix) || k.len() != prefix.len() + 9 {
            return false;
        }
        match extract_trailing_rowid(k) {
            Ok(rowid) => {
                if !ignored_rowids.contains(&rowid) {
                    conflict = true;
                    return false;
                }
                true
            }
            Err(e) => {
                scan_err = Some(e);
                false
            }
        }
    })?;
    if let Some(e) = scan_err {
        return Err(e);
    }
    Ok(conflict)
}

fn scan_matching_rows<S: Storage>(
    pager: &mut Pager<S>,
    table_tree: &Tree,
    ctx: &EvalCtx<'_>,
    where_clause: Option<&crate::sql::ast::Expr>,
) -> Result<Vec<(i64, Vec<Value>)>, Error> {
    let mut out = Vec::new();
    let mut scan_err = None;
    table_tree.scan(pager, None, Bound::Unbounded, &mut |k, v| {
        let res = (|| -> Result<(), Error> {
            let rowid = decode_rowid_key(k)?;
            let cols = decode_record(v)?;
            let keep = match where_clause {
                None => true,
                Some(w) => {
                    let rd = RowData { rowid, cols: &cols };
                    to_bool(&eval_row(w, ctx, Some(&rd))?) == Some(true)
                }
            };
            if keep {
                out.push((rowid, cols));
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
    Ok(out)
}
