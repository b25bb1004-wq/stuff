//! Wire framing and row serialization for the Iridium network protocol.

use std::io::{self, Read, Write};

use crate::sql::{Rows, Value};

/// Maximum wire frame payload size (1 MiB).
pub(crate) const MAX_FRAME_LEN: usize = 1024 * 1024;

/// Encode `Rows` into the binary wire format.
pub fn encode_rows(r: &Rows) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&r.changes.to_be_bytes());
    let col_count = u32::try_from(r.columns.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&col_count.to_be_bytes());
    for col in &r.columns {
        let len = u32::try_from(col.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(col.as_bytes());
    }
    let row_count = u32::try_from(r.rows.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&row_count.to_be_bytes());
    for row in &r.rows {
        for val in row {
            match val {
                Value::Null => out.push(0),
                Value::Int(n) => {
                    out.push(1);
                    out.extend_from_slice(&n.to_be_bytes());
                }
                Value::Text(s) => {
                    out.push(2);
                    let len = u32::try_from(s.len()).unwrap_or(u32::MAX);
                    out.extend_from_slice(&len.to_be_bytes());
                    out.extend_from_slice(s.as_bytes());
                }
                Value::Blob(b) => {
                    out.push(3);
                    let len = u32::try_from(b.len()).unwrap_or(u32::MAX);
                    out.extend_from_slice(&len.to_be_bytes());
                    out.extend_from_slice(b);
                }
            }
        }
    }
    out
}

/// Decode `Rows` from the binary wire format, returning `None` on malformed input.
pub fn decode_rows(b: &[u8]) -> Option<Rows> {
    let mut cur = Cursor::new(b);
    let changes = cur.read_u64()?;
    let col_count = cur.read_u32()? as usize;
    if col_count > cur.remaining() / 4 {
        return None;
    }
    let mut columns = Vec::with_capacity(col_count);
    for _ in 0..col_count {
        let len = cur.read_u32()? as usize;
        let bytes = cur.read_bytes(len)?;
        let name = std::str::from_utf8(bytes).ok()?.to_owned();
        columns.push(name);
    }
    let row_count = cur.read_u32()? as usize;
    if row_count > 0 && col_count == 0 {
        return None;
    }
    if col_count > 0 && row_count > cur.remaining() / col_count {
        return None;
    }
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let mut row = Vec::with_capacity(col_count);
        for _ in 0..col_count {
            let val = decode_value(&mut cur)?;
            row.push(val);
        }
        rows.push(row);
    }
    if cur.remaining() != 0 {
        return None;
    }
    Some(Rows {
        columns,
        rows,
        changes,
    })
}

fn decode_value(cur: &mut Cursor<'_>) -> Option<Value> {
    match cur.read_u8()? {
        0 => Some(Value::Null),
        1 => Some(Value::Int(cur.read_i64()?)),
        2 => {
            let len = cur.read_u32()? as usize;
            let bytes = cur.read_bytes(len)?;
            let text = std::str::from_utf8(bytes).ok()?.to_owned();
            Some(Value::Text(text))
        }
        3 => {
            let len = cur.read_u32()? as usize;
            let bytes = cur.read_bytes(len)?;
            Some(Value::Blob(bytes.to_vec()))
        }
        _ => None,
    }
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(len)?;
        let slice = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    fn read_u8(&mut self) -> Option<u8> {
        let b = self.read_bytes(1)?;
        Some(*b.first()?)
    }

    fn read_u32(&mut self) -> Option<u32> {
        let b = self.read_bytes(4)?;
        let arr: [u8; 4] = b.try_into().ok()?;
        Some(u32::from_be_bytes(arr))
    }

    fn read_u64(&mut self) -> Option<u64> {
        let b = self.read_bytes(8)?;
        let arr: [u8; 8] = b.try_into().ok()?;
        Some(u64::from_be_bytes(arr))
    }

    fn read_i64(&mut self) -> Option<i64> {
        let b = self.read_bytes(8)?;
        let arr: [u8; 8] = b.try_into().ok()?;
        Some(i64::from_be_bytes(arr))
    }
}

/// Read one length-prefixed frame; returns `Ok(None)` on clean EOF at frame boundary.
pub(crate) fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut hdr = [0u8; 4];
    let mut read = 0;
    while read < 4 {
        match r.read(&mut hdr[read..]) {
            Ok(0) => {
                if read == 0 {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated frame header",
                ));
            }
            Ok(n) => read += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_be_bytes(hdr) as usize;
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds 1 MiB limit",
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// Write one length-prefixed frame and flush the stream.
pub(crate) fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds 1 MiB limit",
        ));
    }
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()
}
