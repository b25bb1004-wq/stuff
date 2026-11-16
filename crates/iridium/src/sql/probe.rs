//! B+tree rowid and index probe execution for planned table accesses.

use std::cmp::Ordering;
use std::ops::Bound;

use crate::sql::ast::ColType;
use crate::sql::catalog::TableDef;
use crate::sql::codec::{
    cmp_values, decode_record, decode_rowid_key, encode_key, encode_single_key,
    extract_trailing_rowid,
};
use crate::sql::expr::{eval_row, EvalCtx, RowData, TableRowRef};
use crate::sql::planner::{RangeOp, RangeTerm, TableAccessPlan};
use crate::sql::Value;
use crate::{Error, Pager, Storage, Tree};

const MAX_KEY_BYTES: usize = 1024;

pub(crate) fn probe_table<S: Storage>(
    pager: &mut Pager<S>,
    table: &TableDef,
    table_idx: usize,
    plan: &TableAccessPlan<'_>,
    ctx: &EvalCtx<'_>,
    outer_rows: &[TableRowRef<'_>],
) -> Result<Vec<(i64, Vec<Value>)>, Error> {
    let outer_ctx = EvalCtx {
        tables: ctx.tables[..table_idx].to_vec(),
    };
    let outer_rd = RowData { tables: outer_rows };
    let table_tree = Tree::open(table.root);

    match plan {
        TableAccessPlan::Scan => scan_all(pager, &table_tree),
        TableAccessPlan::Rowid { expr } => {
            let val = eval_row(expr, &outer_ctx, Some(&outer_rd))?;
            let rowid = match val {
                Value::Int(r) => r,
                Value::Real(f) => match exact_int(f) {
                    Some(r) => r,
                    None => return Ok(Vec::new()),
                },
                _ => return Ok(Vec::new()),
            };
            let key = encode_key(&[Value::Int(rowid)]);
            match table_tree.get(pager, None, &key)? {
                Some(rec) => Ok(vec![(rowid, decode_record(&rec)?)]),
                None => Ok(Vec::new()),
            }
        }
        TableAccessPlan::Index {
            index,
            eq_terms,
            range,
        } => {
            let mut eq_vals = Vec::with_capacity(eq_terms.len());
            for &(col, expr) in eq_terms {
                let v = eval_row(expr, &outer_ctx, Some(&outer_rd))?;
                if matches!(v, Value::Null) {
                    return Ok(Vec::new());
                }
                match fit_probe(table, col, v) {
                    Some(v) => eq_vals.push(v),
                    None => return scan_all(pager, &table_tree),
                }
            }
            let prefix_eq = encode_key(&eq_vals);
            if prefix_eq.len() > MAX_KEY_BYTES - 9 {
                return Ok(Vec::new());
            }

            let Some(rowids) = scan_index(
                pager, index.root, &prefix_eq, *range, table, &outer_ctx, &outer_rd,
            )?
            else {
                return scan_all(pager, &table_tree);
            };
            let mut out = Vec::with_capacity(rowids.len());
            for rowid in rowids {
                let rk = encode_key(&[Value::Int(rowid)]);
                let rec = table_tree.get(pager, None, &rk)?.ok_or(Error::Corrupt)?;
                out.push((rowid, decode_record(&rec)?));
            }
            Ok(out)
        }
    }
}

fn scan_all<S: Storage>(
    pager: &mut Pager<S>,
    table_tree: &Tree,
) -> Result<Vec<(i64, Vec<Value>)>, Error> {
    let mut rows = Vec::new();
    let mut scan_err = None;
    table_tree.scan(pager, None, Bound::Unbounded, &mut |k, v| {
        match (|| -> Result<(i64, Vec<Value>), Error> {
            Ok((decode_rowid_key(k)?, decode_record(v)?))
        })() {
            Ok(pair) => {
                rows.push(pair);
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
    Ok(rows)
}

/// Index keys sort by storage class, so a probe value must be in its column's class.
/// Returns `None` when it cannot be (the caller then scans the whole table).
fn fit_probe(table: &TableDef, col: usize, v: Value) -> Option<Value> {
    let ty = table.columns.get(col)?.ty;
    match (ty, &v) {
        (ColType::Real, Value::Int(n)) => {
            let f = *n as f64;
            (f as i128 == i128::from(*n)).then_some(Value::Real(f))
        }
        (ColType::Integer, Value::Real(_)) => None,
        _ => Some(v),
    }
}

fn exact_int(f: f64) -> Option<i64> {
    if f.fract() == 0.0 && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&f)
    {
        Some(f as i64)
    } else {
        None
    }
}

/// Rowids from one index range; `None` means the probe cannot be used and the table must be scanned.
fn scan_index<S: Storage>(
    pager: &mut Pager<S>,
    idx_root: u64,
    prefix_eq: &[u8],
    range: Option<(usize, RangeTerm<'_>)>,
    table: &TableDef,
    outer_ctx: &EvalCtx<'_>,
    outer_rd: &RowData<'_>,
) -> Result<Option<Vec<i64>>, Error> {
    let idx_tree = Tree::open(idx_root);
    let mut rowids = Vec::new();
    let mut scan_err = None;

    match range {
        None => {
            idx_tree.scan(pager, None, Bound::Included(prefix_eq), &mut |k, _| {
                if !k.starts_with(prefix_eq) {
                    return false;
                }
                push_rowid(k, &mut rowids, &mut scan_err)
            })?;
        }
        Some((col, RangeTerm::Cmp(op, expr))) => {
            let val = eval_row(expr, outer_ctx, Some(outer_rd))?;
            if matches!(val, Value::Null) {
                return Ok(Some(Vec::new()));
            }
            let Some(val) = fit_probe(table, col, val) else {
                return Ok(None);
            };
            let mut p_val = prefix_eq.to_vec();
            encode_single_key(&val, &mut p_val);
            scan_cmp_range(
                pager,
                &idx_tree,
                prefix_eq,
                &p_val,
                op,
                &mut rowids,
                &mut scan_err,
            )?;
        }
        Some((col, RangeTerm::Between(lo_expr, hi_expr))) => {
            let lo_val = eval_row(lo_expr, outer_ctx, Some(outer_rd))?;
            let hi_val = eval_row(hi_expr, outer_ctx, Some(outer_rd))?;
            if matches!(lo_val, Value::Null)
                || matches!(hi_val, Value::Null)
                || cmp_values(&lo_val, &hi_val) == Ordering::Greater
            {
                return Ok(Some(Vec::new()));
            }
            let (Some(lo_val), Some(hi_val)) =
                (fit_probe(table, col, lo_val), fit_probe(table, col, hi_val))
            else {
                return Ok(None);
            };
            let mut p_lo = prefix_eq.to_vec();
            encode_single_key(&lo_val, &mut p_lo);
            let mut p_hi = prefix_eq.to_vec();
            encode_single_key(&hi_val, &mut p_hi);
            scan_between_range(
                pager,
                &idx_tree,
                prefix_eq,
                &p_lo,
                &p_hi,
                &mut rowids,
                &mut scan_err,
            )?;
        }
    }

    if let Some(e) = scan_err {
        return Err(e);
    }
    Ok(Some(rowids))
}

fn scan_cmp_range<S: Storage>(
    pager: &mut Pager<S>,
    idx_tree: &Tree,
    prefix_eq: &[u8],
    p_val: &[u8],
    op: RangeOp,
    rowids: &mut Vec<i64>,
    scan_err: &mut Option<Error>,
) -> Result<(), Error> {
    match op {
        RangeOp::Gt => {
            let (lo_bound, check_skip) = if p_val.len() < MAX_KEY_BYTES {
                let mut b = p_val.to_vec();
                b.push(0xFF);
                (b, false)
            } else {
                (p_val[..MAX_KEY_BYTES].to_vec(), true)
            };
            idx_tree.scan(pager, None, Bound::Included(&lo_bound), &mut |k, _| {
                if !k.starts_with(prefix_eq) {
                    return false;
                }
                if check_skip && (k <= p_val || k.starts_with(p_val)) {
                    return true;
                }
                push_rowid(k, rowids, scan_err)
            })
        }
        RangeOp::Ge => {
            let truncated = p_val.len() > MAX_KEY_BYTES;
            let lo_bound = if truncated {
                &p_val[..MAX_KEY_BYTES]
            } else {
                p_val
            };
            idx_tree.scan(pager, None, Bound::Included(lo_bound), &mut |k, _| {
                if !k.starts_with(prefix_eq) {
                    return false;
                }
                if truncated && k < p_val {
                    return true;
                }
                push_rowid(k, rowids, scan_err)
            })
        }
        RangeOp::Lt | RangeOp::Le => {
            let mut lo_bound = prefix_eq.to_vec();
            lo_bound.push(0x06);
            let inclusive = matches!(op, RangeOp::Le);
            idx_tree.scan(pager, None, Bound::Included(&lo_bound), &mut |k, _| {
                if !k.starts_with(prefix_eq) {
                    return false;
                }
                let in_upper = if inclusive {
                    k.starts_with(p_val) || k <= p_val
                } else {
                    k < p_val
                };
                if !in_upper {
                    return false;
                }
                push_rowid(k, rowids, scan_err)
            })
        }
    }
}

fn scan_between_range<S: Storage>(
    pager: &mut Pager<S>,
    idx_tree: &Tree,
    prefix_eq: &[u8],
    p_lo: &[u8],
    p_hi: &[u8],
    rowids: &mut Vec<i64>,
    scan_err: &mut Option<Error>,
) -> Result<(), Error> {
    let truncated = p_lo.len() > MAX_KEY_BYTES;
    let lo_bound = if truncated {
        &p_lo[..MAX_KEY_BYTES]
    } else {
        p_lo
    };
    idx_tree.scan(pager, None, Bound::Included(lo_bound), &mut |k, _| {
        if !k.starts_with(prefix_eq) {
            return false;
        }
        if truncated && k < p_lo {
            return true;
        }
        if !k.starts_with(p_hi) && k > p_hi {
            return false;
        }
        push_rowid(k, rowids, scan_err)
    })
}

fn push_rowid(k: &[u8], rowids: &mut Vec<i64>, scan_err: &mut Option<Error>) -> bool {
    match extract_trailing_rowid(k) {
        Ok(rid) => {
            rowids.push(rid);
            true
        }
        Err(e) => {
            *scan_err = Some(e);
            false
        }
    }
}
