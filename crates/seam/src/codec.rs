//! Little-endian reading and length helpers shared by the encodings.

use crate::DecodeError;

/// A bounds-checked cursor over input bytes. Every read either succeeds or reports `Truncated`.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `bytes`.
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    /// The next `n` bytes.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    /// The next `N` bytes as an array.
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let slice = self.take(N)?;
        <[u8; N]>::try_from(slice).map_err(|_| DecodeError::Truncated)
    }

    /// A little-endian `u64`.
    pub(crate) fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// A little-endian `u32`.
    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// Succeeds only when every input byte has been read.
    pub(crate) fn finish(&self) -> Result<(), DecodeError> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}

/// A length as the `u32` written on the wire. Lengths beyond `u32` cannot occur for valid data.
pub(crate) fn len_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A `u32` count or length as `usize`.
pub(crate) fn to_usize(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}
