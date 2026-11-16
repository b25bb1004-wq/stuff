//! Binary row record and order-preserving key encoding for Iridium SQL.

use std::cmp::Ordering;

use crate::sql::Value;
use crate::Error;

const KEY_TAG_NULL: u8 = 0x05;
const KEY_TAG_INT: u8 = 0x10;
// Between INT and TEXT, so key order keeps numbers before text and blobs.
const KEY_TAG_REAL: u8 = 0x18;
const KEY_TAG_TEXT: u8 = 0x20;
const KEY_TAG_BLOB: u8 = 0x30;

const REC_TAG_NULL: u8 = 0;
const REC_TAG_INT: u8 = 1;
const REC_TAG_TEXT: u8 = 2;
const REC_TAG_BLOB: u8 = 3;
const REC_TAG_REAL: u8 = 4;

/// Map an f64 to u64 so that unsigned order matches numeric order (-0.0 is stored as 0.0).
fn real_key_bits(f: f64) -> u64 {
    let f = if f == 0.0 { 0.0 } else { f };
    let b = f.to_bits();
    if b >> 63 == 1 {
        !b
    } else {
        b | (1u64 << 63)
    }
}

fn real_from_key_bits(k: u64) -> f64 {
    let b = if k >> 63 == 1 { k & !(1u64 << 63) } else { !k };
    f64::from_bits(b)
}

/// Compare an integer with a float exactly (no rounding of the integer).
pub(crate) fn cmp_int_real(i: i64, f: f64) -> Ordering {
    // Every i64 is below 2^63; every f64 at or below -2^63 - 1 is below i64::MIN.
    if f >= 9_223_372_036_854_775_808.0 {
        return Ordering::Less;
    }
    if f < -9_223_372_036_854_775_808.0 {
        return Ordering::Greater;
    }
    let t = f.trunc();
    match i.cmp(&(t as i64)) {
        Ordering::Equal => {
            let frac = f - t;
            if frac > 0.0 {
                Ordering::Less
            } else if frac < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        other => other,
    }
}

/// Encode a tuple of values into a byte string whose `memcmp` order matches SQL order.
pub fn encode_key(values: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        encode_single_key(v, &mut out);
    }
    out
}

pub(crate) fn encode_single_key(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.push(KEY_TAG_NULL),
        Value::Int(n) => {
            out.push(KEY_TAG_INT);
            let biased = (*n as u64) ^ (1u64 << 63);
            out.extend_from_slice(&biased.to_be_bytes());
        }
        Value::Real(f) => {
            out.push(KEY_TAG_REAL);
            out.extend_from_slice(&real_key_bits(*f).to_be_bytes());
        }
        Value::Text(s) => {
            out.push(KEY_TAG_TEXT);
            encode_escaped_bytes(s.as_bytes(), out);
        }
        Value::Blob(b) => {
            out.push(KEY_TAG_BLOB);
            encode_escaped_bytes(b, out);
        }
    }
}

fn encode_escaped_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        if b == 0x00 {
            out.push(0x00);
            out.push(0xFF);
        } else {
            out.push(b);
        }
    }
    out.push(0x00);
    out.push(0x00);
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn decode_key(bytes: &[u8]) -> Result<Vec<Value>, Error> {
    let mut pos = 0usize;
    let mut out = Vec::new();
    while pos < bytes.len() {
        let tag = *bytes.get(pos).ok_or(Error::Corrupt)?;
        pos += 1;
        match tag {
            KEY_TAG_NULL => out.push(Value::Null),
            KEY_TAG_INT => {
                let slice: [u8; 8] = bytes
                    .get(pos..pos + 8)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(Error::Corrupt)?;
                pos += 8;
                let biased = u64::from_be_bytes(slice);
                out.push(Value::Int((biased ^ (1u64 << 63)) as i64));
            }
            KEY_TAG_REAL => {
                let slice: [u8; 8] = bytes
                    .get(pos..pos + 8)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(Error::Corrupt)?;
                pos += 8;
                let f = real_from_key_bits(u64::from_be_bytes(slice));
                if f.is_nan() {
                    return Err(Error::Corrupt);
                }
                out.push(Value::Real(f));
            }
            KEY_TAG_TEXT => {
                let raw = decode_escaped_bytes(bytes, &mut pos)?;
                let s = String::from_utf8(raw).map_err(|_| Error::Corrupt)?;
                out.push(Value::Text(s));
            }
            KEY_TAG_BLOB => {
                let raw = decode_escaped_bytes(bytes, &mut pos)?;
                out.push(Value::Blob(raw));
            }
            _ => return Err(Error::Corrupt),
        }
    }
    Ok(out)
}

pub(crate) fn decode_rowid_key(bytes: &[u8]) -> Result<i64, Error> {
    if bytes.len() != 9 || bytes[0] != KEY_TAG_INT {
        return Err(Error::Corrupt);
    }
    let slice: [u8; 8] = bytes
        .get(1..9)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::Corrupt)?;
    Ok((u64::from_be_bytes(slice) ^ (1u64 << 63)) as i64)
}

pub(crate) fn extract_trailing_rowid(key: &[u8]) -> Result<i64, Error> {
    if key.len() < 9 {
        return Err(Error::Corrupt);
    }
    decode_rowid_key(&key[key.len() - 9..])
}

fn decode_escaped_bytes(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, Error> {
    let mut raw = Vec::new();
    loop {
        let b = *bytes.get(*pos).ok_or(Error::Corrupt)?;
        *pos += 1;
        if b != 0x00 {
            raw.push(b);
        } else {
            let next = *bytes.get(*pos).ok_or(Error::Corrupt)?;
            *pos += 1;
            if next == 0xFF {
                raw.push(0x00);
            } else if next == 0x00 {
                return Ok(raw);
            } else {
                return Err(Error::Corrupt);
            }
        }
    }
}

/// Encode a row's column values into a compact binary record.
pub fn encode_record(values: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    encode_varint(values.len() as u64, &mut out);
    for v in values {
        match v {
            Value::Null => out.push(REC_TAG_NULL),
            Value::Int(n) => {
                out.push(REC_TAG_INT);
                let zz = (n.wrapping_shl(1) ^ (n >> 63)) as u64;
                encode_varint(zz, &mut out);
            }
            Value::Real(f) => {
                out.push(REC_TAG_REAL);
                out.extend_from_slice(&f.to_bits().to_be_bytes());
            }
            Value::Text(s) => {
                out.push(REC_TAG_TEXT);
                encode_varint(s.len() as u64, &mut out);
                out.extend_from_slice(s.as_bytes());
            }
            Value::Blob(b) => {
                out.push(REC_TAG_BLOB);
                encode_varint(b.len() as u64, &mut out);
                out.extend_from_slice(b);
            }
        }
    }
    out
}

/// Decode a binary row record back into its column values.
pub fn decode_record(bytes: &[u8]) -> Result<Vec<Value>, Error> {
    let (count, mut pos) = decode_varint(bytes)?;
    let remaining = bytes.len().saturating_sub(pos);
    if count > remaining as u64 {
        return Err(Error::Corrupt);
    }
    let count = count as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = *bytes.get(pos).ok_or(Error::Corrupt)?;
        pos += 1;
        match tag {
            REC_TAG_NULL => out.push(Value::Null),
            REC_TAG_INT => {
                let (zz, used) = decode_varint(&bytes[pos..])?;
                pos += used;
                let n = ((zz >> 1) as i64) ^ -((zz & 1) as i64);
                out.push(Value::Int(n));
            }
            REC_TAG_REAL => {
                let slice: [u8; 8] = bytes
                    .get(pos..pos + 8)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(Error::Corrupt)?;
                pos += 8;
                let f = f64::from_bits(u64::from_be_bytes(slice));
                if f.is_nan() {
                    return Err(Error::Corrupt);
                }
                out.push(Value::Real(f));
            }
            REC_TAG_TEXT => {
                let (len, used) = decode_varint(&bytes[pos..])?;
                pos += used;
                let rem = bytes.len().saturating_sub(pos);
                if len > rem as u64 {
                    return Err(Error::Corrupt);
                }
                let len = len as usize;
                let slice = bytes.get(pos..pos + len).ok_or(Error::Corrupt)?;
                pos += len;
                let s = std::str::from_utf8(slice).map_err(|_| Error::Corrupt)?;
                out.push(Value::Text(s.to_owned()));
            }
            REC_TAG_BLOB => {
                let (len, used) = decode_varint(&bytes[pos..])?;
                pos += used;
                let rem = bytes.len().saturating_sub(pos);
                if len > rem as u64 {
                    return Err(Error::Corrupt);
                }
                let len = len as usize;
                let slice = bytes.get(pos..pos + len).ok_or(Error::Corrupt)?;
                pos += len;
                out.push(Value::Blob(slice.to_vec()));
            }
            _ => return Err(Error::Corrupt),
        }
    }
    if pos != bytes.len() {
        return Err(Error::Corrupt);
    }
    Ok(out)
}

pub(crate) fn cmp_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Int(x), Value::Real(y)) => cmp_int_real(*x, *y),
        (Value::Real(x), Value::Int(y)) => cmp_int_real(*y, *x).reverse(),
        (Value::Int(_) | Value::Real(_), _) => Ordering::Less,
        (_, Value::Int(_) | Value::Real(_)) => Ordering::Greater,
        (Value::Text(x), Value::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Text(_), Value::Blob(_)) => Ordering::Less,
        (Value::Blob(_), Value::Text(_)) => Ordering::Greater,
        (Value::Blob(x), Value::Blob(y)) => x.cmp(y),
    }
}

pub(crate) fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

pub(crate) fn decode_varint(slice: &[u8]) -> Result<(u64, usize), Error> {
    let mut val = 0u64;
    for i in 0..10 {
        let &b = slice.get(i).ok_or(Error::Corrupt)?;
        if i == 9 {
            if b != 1 {
                return Err(Error::Corrupt);
            }
            val |= 1u64 << 63;
            return Ok((val, 10));
        }
        if b & 0x80 == 0 {
            if i > 0 && b == 0 {
                return Err(Error::Corrupt);
            }
            val |= u64::from(b) << (i * 7);
            return Ok((val, i + 1));
        }
        val |= u64::from(b & 0x7f) << (i * 7);
    }
    Err(Error::Corrupt)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn key_order_matches_sql_order() {
        let vals = [
            Value::Null,
            Value::Int(i64::MIN),
            Value::Int(-1000),
            Value::Int(-1),
            Value::Int(0),
            Value::Int(1),
            Value::Int(i64::MAX),
            Value::Text(String::new()),
            Value::Text("\0".into()),
            Value::Text("\0\0".into()),
            Value::Text("\0a".into()),
            Value::Text("a".into()),
            Value::Text("ab".into()),
            Value::Text("b".into()),
            Value::Blob(vec![]),
            Value::Blob(vec![0x00]),
            Value::Blob(vec![0x00, 0x00]),
            Value::Blob(vec![0x00, 0xff]),
            Value::Blob(vec![0x01]),
        ];
        for i in 0..vals.len() {
            for j in 0..vals.len() {
                let ki = encode_key(std::slice::from_ref(&vals[i]));
                let kj = encode_key(std::slice::from_ref(&vals[j]));
                assert_eq!(ki.cmp(&kj), cmp_values(&vals[i], &vals[j]));
            }
            let k = encode_key(std::slice::from_ref(&vals[i]));
            assert_eq!(decode_key(&k).unwrap(), vec![vals[i].clone()]);
        }
    }

    #[test]
    fn record_round_trip_extremes() {
        let row = vec![
            Value::Null,
            Value::Int(0),
            Value::Int(-1),
            Value::Int(1),
            Value::Int(i64::MIN),
            Value::Int(i64::MAX),
            Value::Text("hello\0world".into()),
            Value::Blob(vec![0, 1, 255, 0]),
        ];
        let enc = encode_record(&row);
        assert_eq!(decode_record(&enc).unwrap(), row);
    }

    #[test]
    fn hostile_record_bytes_never_panic() {
        assert_eq!(decode_record(&[]), Err(Error::Corrupt));
        assert_eq!(decode_record(&[0x01, 0x09]), Err(Error::Corrupt));
        assert_eq!(
            decode_record(&[0xff, 0xff, 0xff, 0xff, 0x0f]),
            Err(Error::Corrupt)
        );
        assert_eq!(
            decode_record(&[0x01, 0x02, 0x02, 0xff, 0xfe]),
            Err(Error::Corrupt)
        );
        assert_eq!(decode_record(&[0x01, 0x00, 0x00]), Err(Error::Corrupt));
    }
}
