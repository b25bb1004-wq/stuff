//! Scalar and aggregate SQL functions, type coercions, 3VL, and LIKE matching.

use std::cmp::Ordering;

use crate::sql::ast::{BinaryOp, CallArgs};
use crate::sql::codec::cmp_values;
use crate::sql::expr::{eval_group, eval_row, is_agg_name, EvalCtx, RowData};
use crate::sql::lexer::parse_err;
use crate::sql::Value;
use crate::Error;

pub(crate) fn eval_call(
    name: &str,
    args: &CallArgs,
    pos: usize,
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
) -> Result<Value, Error> {
    if is_agg_name(name) {
        return eval_agg(name, args, ctx, group);
    }
    let CallArgs::List(list) = args else {
        return Err(parse_err(pos, "unexpected *"));
    };
    if name.eq_ignore_ascii_case("coalesce") {
        for arg in list {
            let v = eval_group(arg, ctx, group)?;
            if !matches!(v, Value::Null) {
                return Ok(v);
            }
        }
        return Ok(Value::Null);
    }
    let mut vals = Vec::with_capacity(list.len());
    for arg in list {
        vals.push(eval_group(arg, ctx, group)?);
    }
    if name.eq_ignore_ascii_case("typeof") {
        let ty = match vals[0] {
            Value::Null => "null",
            Value::Int(_) => "integer",
            Value::Text(_) => "text",
            Value::Blob(_) => "blob",
        };
        return Ok(Value::Text(ty.to_owned()));
    }
    if name.eq_ignore_ascii_case("abs") {
        return match &vals[0] {
            Value::Null => Ok(Value::Null),
            v => to_int_coerce(v)?
                .checked_abs()
                .map(Value::Int)
                .ok_or(Error::Overflow),
        };
    }
    if name.eq_ignore_ascii_case("length") {
        return Ok(match &vals[0] {
            Value::Null => Value::Null,
            Value::Text(s) => Value::Int(s.chars().count() as i64),
            Value::Blob(b) => Value::Int(b.len() as i64),
            Value::Int(n) => Value::Int(n.to_string().len() as i64),
        });
    }
    if name.eq_ignore_ascii_case("lower") {
        return Ok(match &vals[0] {
            Value::Null => Value::Null,
            v => Value::Text(value_to_text(v).to_ascii_lowercase()),
        });
    }
    if name.eq_ignore_ascii_case("upper") {
        return Ok(match &vals[0] {
            Value::Null => Value::Null,
            v => Value::Text(value_to_text(v).to_ascii_uppercase()),
        });
    }
    if name.eq_ignore_ascii_case("substr") {
        return eval_substr(&vals);
    }
    Err(parse_err(pos, format!("unknown function {name}")))
}

fn eval_substr(vals: &[Value]) -> Result<Value, Error> {
    if vals.iter().any(|v| matches!(v, Value::Null)) {
        return Ok(Value::Null);
    }
    let start = i128::from(to_int_coerce(&vals[1])?);
    let len_opt = if vals.len() == 3 {
        Some(i128::from(to_int_coerce(&vals[2])?))
    } else {
        None
    };

    match &vals[0] {
        Value::Blob(b) => {
            let (lo, hi) = substr_bounds(b.len() as i128, start, len_opt);
            Ok(Value::Blob(b[lo..hi].to_vec()))
        }
        v => {
            let chars: Vec<char> = value_to_text(v).chars().collect();
            let (lo, hi) = substr_bounds(chars.len() as i128, start, len_opt);
            let out: String = chars[lo..hi].iter().collect();
            Ok(Value::Text(out))
        }
    }
}

fn substr_bounds(n: i128, start: i128, len_opt: Option<i128>) -> (usize, usize) {
    match len_opt {
        None => {
            let anchor = if start > 0 {
                start - 1
            } else if start == 0 {
                0
            } else {
                n + start
            };
            (anchor.clamp(0, n) as usize, n as usize)
        }
        Some(len) => {
            let anchor = if start > 0 {
                start - 1
            } else if start == 0 {
                -1
            } else {
                n + start
            };
            let (raw_lo, raw_hi) = if len >= 0 {
                (anchor, anchor + len)
            } else {
                (anchor + len, anchor)
            };
            let lo = raw_lo.clamp(0, n) as usize;
            let hi = raw_hi.clamp(0, n) as usize;
            (lo, hi)
        }
    }
}

fn eval_agg(
    name: &str,
    args: &CallArgs,
    ctx: &EvalCtx<'_>,
    group: &[RowData<'_>],
) -> Result<Value, Error> {
    if name.eq_ignore_ascii_case("count") {
        return match args {
            CallArgs::Star => Ok(Value::Int(group.len() as i64)),
            CallArgs::List(list) => {
                let mut count = 0i64;
                for row in group {
                    let v = eval_row(&list[0], ctx, Some(row))?;
                    if !matches!(v, Value::Null) {
                        count += 1;
                    }
                }
                Ok(Value::Int(count))
            }
        };
    }
    let CallArgs::List(list) = args else {
        return Ok(Value::Null);
    };
    let expr = &list[0];
    if name.eq_ignore_ascii_case("sum") {
        let mut total: Option<i64> = None;
        for row in group {
            let v = eval_row(expr, ctx, Some(row))?;
            if !matches!(v, Value::Null) {
                let n = to_int_coerce(&v)?;
                total = Some(match total {
                    None => n,
                    Some(acc) => acc.checked_add(n).ok_or(Error::Overflow)?,
                });
            }
        }
        return Ok(total.map_or(Value::Null, Value::Int));
    }
    let want_min = name.eq_ignore_ascii_case("min");
    let mut best: Option<Value> = None;
    for row in group {
        let v = eval_row(expr, ctx, Some(row))?;
        if matches!(v, Value::Null) {
            continue;
        }
        best = Some(match best {
            None => v,
            Some(cur) => {
                let ord = cmp_values(&v, &cur);
                if (want_min && ord == Ordering::Less) || (!want_min && ord == Ordering::Greater) {
                    v
                } else {
                    cur
                }
            }
        });
    }
    Ok(best.unwrap_or(Value::Null))
}

pub(crate) fn to_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Null => None,
        Value::Int(n) => Some(*n != 0),
        Value::Text(s) => Some(parse_leading_i64(s).unwrap_or(1) != 0),
        Value::Blob(b) => {
            let s = std::str::from_utf8(b).unwrap_or("");
            Some(parse_leading_i64(s).unwrap_or(1) != 0)
        }
    }
}

pub(crate) fn three_val_not(v: &Value) -> Value {
    match to_bool(v) {
        None => Value::Null,
        Some(b) => Value::Int(i64::from(!b)),
    }
}

pub(crate) fn three_val_and(l: &Value, r: &Value) -> Value {
    match (to_bool(l), to_bool(r)) {
        (Some(false), _) | (_, Some(false)) => Value::Int(0),
        (Some(true), Some(true)) => Value::Int(1),
        _ => Value::Null,
    }
}

pub(crate) fn three_val_or(l: &Value, r: &Value) -> Value {
    match (to_bool(l), to_bool(r)) {
        (Some(true), _) | (_, Some(true)) => Value::Int(1),
        (Some(false), Some(false)) => Value::Int(0),
        _ => Value::Null,
    }
}

pub(crate) fn cmp_sql(l: &Value, r: &Value, op: BinaryOp) -> Value {
    if matches!(l, Value::Null) || matches!(r, Value::Null) {
        return Value::Null;
    }
    let ord = cmp_values(l, r);
    let b = match op {
        BinaryOp::Eq => ord == Ordering::Equal,
        BinaryOp::Ne => ord != Ordering::Equal,
        BinaryOp::Lt => ord == Ordering::Less,
        BinaryOp::Le => ord != Ordering::Greater,
        BinaryOp::Gt => ord == Ordering::Greater,
        BinaryOp::Ge => ord != Ordering::Less,
        _ => false,
    };
    Value::Int(i64::from(b))
}

pub(crate) fn to_int_coerce(v: &Value) -> Result<i64, Error> {
    match v {
        Value::Int(n) => Ok(*n),
        Value::Text(s) => parse_leading_i64(s),
        Value::Blob(b) => match std::str::from_utf8(b) {
            Ok(s) => parse_leading_i64(s),
            Err(_) => Ok(0),
        },
        Value::Null => Ok(0),
    }
}

fn parse_leading_i64(s: &str) -> Result<i64, Error> {
    let trimmed = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let bytes = trimmed.as_bytes();
    let mut i = 0usize;
    if bytes.first() == Some(&b'+') || bytes.first() == Some(&b'-') {
        i += 1;
    }
    let digit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        return Ok(0);
    }
    trimmed[..i].parse::<i64>().map_err(|_| Error::Overflow)
}

pub(crate) fn value_to_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Int(n) => n.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

pub(crate) fn like_match(text: &[char], pat: &[char]) -> bool {
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (None, 0usize);
    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == '_' || pat[pi].eq_ignore_ascii_case(&text[ti])) {
            ti += 1;
            pi += 1;
        } else if pi < pat.len() && pat[pi] == '%' {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '%' {
        pi += 1;
    }
    pi == pat.len()
}
