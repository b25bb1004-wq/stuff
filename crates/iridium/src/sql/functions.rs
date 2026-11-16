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
            Value::Real(_) => "real",
            Value::Text(_) => "text",
            Value::Blob(_) => "blob",
        };
        return Ok(Value::Text(ty.to_owned()));
    }
    if name.eq_ignore_ascii_case("abs") {
        return match &vals[0] {
            Value::Null => Ok(Value::Null),
            Value::Real(f) => Ok(Value::Real(f.abs())),
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
            Value::Real(f) => Value::Int(real_to_text(*f).len() as i64),
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
        // Integer sum stays exact; any REAL input switches the result to REAL.
        let mut int_sum: Option<i64> = Some(0);
        let mut real_sum = 0.0f64;
        let mut any = false;
        let mut any_real = false;
        for row in group {
            match eval_row(expr, ctx, Some(row))? {
                Value::Null => {}
                Value::Real(f) => {
                    any = true;
                    any_real = true;
                    real_sum += f;
                }
                v => {
                    any = true;
                    let n = to_int_coerce(&v)?;
                    real_sum += n as f64;
                    int_sum = int_sum.and_then(|acc| acc.checked_add(n));
                }
            }
        }
        if !any {
            return Ok(Value::Null);
        }
        if any_real {
            return Ok(real_value(real_sum));
        }
        return int_sum.map(Value::Int).ok_or(Error::Overflow);
    }
    if name.eq_ignore_ascii_case("avg") {
        let mut int_sum: i128 = 0;
        let mut real_sum = 0.0f64;
        let mut count = 0u64;
        let mut any_real = false;
        for row in group {
            match eval_row(expr, ctx, Some(row))? {
                Value::Null => {}
                Value::Real(f) => {
                    any_real = true;
                    real_sum += f;
                    count += 1;
                }
                v => {
                    let n = to_int_coerce(&v)?;
                    int_sum += i128::from(n);
                    real_sum += n as f64;
                    count += 1;
                }
            }
        }
        if count == 0 {
            return Ok(Value::Null);
        }
        let total = if any_real { real_sum } else { int_sum as f64 };
        return Ok(real_value(total / count as f64));
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

/// Wrap a float result; NaN (e.g. inf - inf) becomes NULL, as in SQLite.
pub(crate) fn real_value(f: f64) -> Value {
    if f.is_nan() {
        Value::Null
    } else {
        Value::Real(f)
    }
}

/// Render a REAL with its shortest round-trip form, keeping ".0" on whole numbers.
pub(crate) fn real_to_text(f: f64) -> String {
    let s = format!("{f}");
    if f.is_finite() && !s.contains(['.', 'e', 'E']) {
        format!("{s}.0")
    } else {
        s
    }
}

/// Numeric value of an operand: INTEGER and REAL as-is, other text via integer coercion.
pub(crate) fn to_real_coerce(v: &Value) -> Result<f64, Error> {
    match v {
        Value::Real(f) => Ok(*f),
        other => Ok(to_int_coerce(other)? as f64),
    }
}

pub(crate) fn to_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Null => None,
        Value::Int(n) => Some(*n != 0),
        Value::Real(f) => Some(*f != 0.0),
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
        Value::Real(f) => {
            if f.is_nan() {
                Ok(0)
            } else if *f >= 9_223_372_036_854_775_808.0 || *f < -9_223_372_036_854_775_808.0 {
                Err(Error::Overflow)
            } else {
                Ok(f.trunc() as i64)
            }
        }
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
        Value::Real(f) => real_to_text(*f),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

const MAX_LIKE_STEPS: usize = 10_000_000;

pub(crate) fn like_match(text: &[char], pat: &[char]) -> Result<bool, Error> {
    let parts: Vec<&[char]> = pat.split(|&c| c == '%').collect();
    if parts.len() == 1 {
        return Ok(chars_match(text, pat));
    }
    let first = parts[0];
    if text.len() < first.len() || !chars_match(&text[..first.len()], first) {
        return Ok(false);
    }
    let mut start = first.len();
    let last = parts[parts.len() - 1];
    if text.len() - start < last.len() || !chars_match(&text[text.len() - last.len()..], last) {
        return Ok(false);
    }
    let end = text.len() - last.len();
    let mut steps = 0usize;
    for &seg in &parts[1..parts.len() - 1] {
        if seg.is_empty() {
            continue;
        }
        let found = if seg.contains(&'_') {
            find_segment_with_wildcard(&text[start..end], seg, &mut steps)?
        } else {
            find_segment_kmp(&text[start..end], seg)
        };
        let Some(pos) = found else {
            return Ok(false);
        };
        start += pos + seg.len();
    }
    Ok(true)
}

fn chars_match(text: &[char], pat: &[char]) -> bool {
    text.len() == pat.len()
        && text
            .iter()
            .zip(pat.iter())
            .all(|(&tc, &pc)| pc == '_' || pc.eq_ignore_ascii_case(&tc))
}

fn find_segment_kmp(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let mut lps = vec![0usize; needle.len()];
    let mut len = 0usize;
    let mut i = 1usize;
    while i < needle.len() {
        if needle[i].eq_ignore_ascii_case(&needle[len]) {
            len += 1;
            lps[i] = len;
            i += 1;
        } else if len != 0 {
            len = lps[len - 1];
        } else {
            lps[i] = 0;
            i += 1;
        }
    }
    let mut hi = 0usize;
    let mut ni = 0usize;
    while hi < haystack.len() {
        if needle[ni].eq_ignore_ascii_case(&haystack[hi]) {
            hi += 1;
            ni += 1;
            if ni == needle.len() {
                return Some(hi - ni);
            }
        } else if ni != 0 {
            ni = lps[ni - 1];
        } else {
            hi += 1;
        }
    }
    None
}

fn find_segment_with_wildcard(
    haystack: &[char],
    needle: &[char],
    steps: &mut usize,
) -> Result<Option<usize>, Error> {
    if needle.len() > haystack.len() {
        return Ok(None);
    }
    for pos in 0..=haystack.len() - needle.len() {
        let mut matched = true;
        for (j, &pc) in needle.iter().enumerate() {
            *steps += 1;
            if *steps > MAX_LIKE_STEPS {
                return Err(Error::TooLarge);
            }
            if pc != '_' && !pc.eq_ignore_ascii_case(&haystack[pos + j]) {
                matched = false;
                break;
            }
        }
        if matched {
            return Ok(Some(pos));
        }
    }
    Ok(None)
}
