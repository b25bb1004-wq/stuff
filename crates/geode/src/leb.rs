//! LEB128 writers and a bounds-checked byte reader shared by the decoder.

use crate::decode::DecodeError;
use alloc::{string::String, vec::Vec};

/// Append `v` as unsigned LEB128.
pub(crate) fn put_uint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let low = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Append `v` as zig-zag signed LEB128.
pub(crate) fn put_sint(out: &mut Vec<u8>, v: i64) {
    put_uint(out, ((v as u64) << 1) ^ ((v >> 63) as u64));
}

/// Cursor over hostile input. Every read is bounds-checked; nothing indexes past the end.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start reading at the first byte.
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    /// Bytes not yet read.
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    /// One raw byte.
    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        let b = self
            .bytes
            .get(self.pos)
            .copied()
            .ok_or(DecodeError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    /// The next `n` raw bytes.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let s = self
            .bytes
            .get(self.pos..end)
            .ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    /// Unsigned LEB128 that must fit in `bits` (16, 32 or 64) and be minimal.
    pub(crate) fn uint(&mut self, bits: u32) -> Result<u64, DecodeError> {
        let max_bytes = bits.div_ceil(7);
        let mut value: u64 = 0;
        for i in 0..max_bytes {
            let byte = self.u8()?;
            let payload = u64::from(byte & 0x7f);
            let shift = 7 * i;
            let more = byte & 0x80 != 0;
            if i + 1 == max_bytes {
                // The last byte the type allows: no continuation, and the payload must fit.
                let spare = bits - shift;
                if more || (spare < 7 && payload >> spare != 0) {
                    return Err(DecodeError::Overlong);
                }
            }
            value |= payload << shift;
            if !more {
                // A zero group after the first one is a redundant encoding.
                if i > 0 && payload == 0 {
                    return Err(DecodeError::Overlong);
                }
                return Ok(value);
            }
        }
        Err(DecodeError::Overlong)
    }

    /// Zig-zag signed LEB128 in 64 bits.
    pub(crate) fn sint(&mut self) -> Result<i64, DecodeError> {
        let u = self.uint(64)?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    /// A 16-bit value (registers, counts, field indexes).
    pub(crate) fn uint16(&mut self) -> Result<u16, DecodeError> {
        Ok(self.uint(16)? as u16)
    }

    /// A 32-bit value (constant, function, import, tag and jump indexes).
    pub(crate) fn uint32(&mut self) -> Result<u32, DecodeError> {
        Ok(self.uint(32)? as u32)
    }

    /// An element count. Each element takes at least one byte, so it must not exceed the bytes left.
    pub(crate) fn count(&mut self) -> Result<usize, DecodeError> {
        let n = self.uint(32)?;
        if n > self.remaining() as u64 {
            return Err(DecodeError::TooLarge);
        }
        Ok(n as usize)
    }

    /// A length-prefixed byte string.
    pub(crate) fn blob(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.count()?;
        self.take(n)
    }

    /// A length-prefixed UTF-8 string.
    pub(crate) fn string(&mut self) -> Result<String, DecodeError> {
        let b = self.blob()?;
        let s = core::str::from_utf8(b).map_err(|_| DecodeError::BadUtf8)?;
        Ok(String::from(s))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use assay::{forall, Rng};

    fn read_u(bytes: &[u8], bits: u32) -> Result<u64, DecodeError> {
        Reader::new(bytes).uint(bits)
    }

    #[test]
    fn unsigned_edge_values_round_trip() {
        for v in [0u64, 127, 128, u64::from(u32::MAX), u64::from(u16::MAX)] {
            let mut out = Vec::new();
            put_uint(&mut out, v);
            let bits = if v <= u64::from(u16::MAX) { 16 } else { 32 };
            assert_eq!(read_u(&out, bits), Ok(v));
        }
        let mut out = Vec::new();
        put_uint(&mut out, u64::MAX);
        assert_eq!(read_u(&out, 64), Ok(u64::MAX));
    }

    #[test]
    fn signed_edge_values_round_trip() {
        for v in [0i64, 1, -1, 63, -64, 64, -65, i64::MIN, i64::MAX] {
            let mut out = Vec::new();
            put_sint(&mut out, v);
            let mut r = Reader::new(&out);
            assert_eq!(r.sint(), Ok(v));
            assert_eq!(r.remaining(), 0);
        }
    }

    #[test]
    fn random_values_round_trip() {
        forall(3000, 11, |r: &mut Rng| {
            let u = r.u64();
            let mut out = Vec::new();
            put_uint(&mut out, u);
            assert_eq!(read_u(&out, 64), Ok(u));
            let s = u as i64;
            let mut out = Vec::new();
            put_sint(&mut out, s);
            assert_eq!(Reader::new(&out).sint(), Ok(s));
        });
    }

    #[test]
    fn overlong_encodings_are_rejected() {
        // A redundant zero group.
        assert_eq!(read_u(&[0x80, 0x00], 32), Err(DecodeError::Overlong));
        assert_eq!(
            read_u(&[0x80, 0x80, 0x80, 0x80, 0x00], 32),
            Err(DecodeError::Overlong)
        );
        // Longer than the type allows.
        assert_eq!(
            read_u(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00], 32),
            Err(DecodeError::Overlong)
        );
        assert_eq!(
            read_u(&[0xff, 0xff, 0xff, 0xff, 0xff, 0x01], 32),
            Err(DecodeError::Overlong)
        );
        // Values too big for the type.
        assert_eq!(read_u(&[0xff, 0xff, 0x04], 16), Err(DecodeError::Overlong));
        assert_eq!(read_u(&[0xff, 0xff, 0x03], 16), Ok(65535));
        assert_eq!(
            read_u(&[0xff, 0xff, 0xff, 0xff, 0x1f], 32),
            Err(DecodeError::Overlong)
        );
        let mut ten = vec![0xffu8; 9];
        ten.push(0x02);
        assert_eq!(read_u(&ten, 64), Err(DecodeError::Overlong));
        ten[9] = 0x01;
        assert_eq!(read_u(&ten, 64), Ok(u64::MAX));
    }

    #[test]
    fn truncated_leb_is_rejected() {
        assert_eq!(read_u(&[0x80], 32), Err(DecodeError::Truncated));
        assert_eq!(read_u(&[], 16), Err(DecodeError::Truncated));
    }
}
