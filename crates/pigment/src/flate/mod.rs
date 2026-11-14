//! DEFLATE (RFC 1951), zlib (RFC 1950), CRC-32, and Adler-32.

mod checksum;
mod deflate;
mod huffman;
mod inflate;

#[cfg(test)]
mod tests;

use alloc::vec::Vec;

pub use checksum::{adler32, crc32};
pub use deflate::deflate;
pub use inflate::inflate;

/// Errors returned by DEFLATE and zlib decompression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlateError {
    /// Input ended before the compressed stream or trailer finished.
    Truncated,
    /// Malformed header, Huffman table, block, or backreference distance.
    Invalid,
    /// Adler-32 trailer mismatch in a zlib stream.
    Checksum,
    /// Decompressed output would exceed `max_out`.
    TooLarge,
}

/// zlib wrapper (RFC 1950): header check, no preset dictionary, Adler-32 trailer checked.
pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, FlateError> {
    let hdr = data.get(..2).ok_or(FlateError::Truncated)?;
    let cmf = hdr[0];
    let flg = hdr[1];
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err(FlateError::Invalid);
    }
    let cm = cmf & 0x0F;
    let cinfo = cmf >> 4;
    if cm != 8 || cinfo > 7 {
        return Err(FlateError::Invalid);
    }
    if flg & 0x20 != 0 {
        return Err(FlateError::Invalid);
    }

    let (out, used) = inflate::inflate_with_len(&data[2..], max_out)?;
    let trailer_start = 2 + used;
    let trailer = data
        .get(trailer_start..trailer_start + 4)
        .ok_or(FlateError::Truncated)?;
    let expected = u32::from_be_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    if adler32(&out) != expected {
        return Err(FlateError::Checksum);
    }
    Ok(out)
}

/// zlib wrapper (RFC 1950) compression using `deflate` at `level`.
pub fn zlib_compress(data: &[u8], level: u8) -> Vec<u8> {
    let flevel = match level {
        0..=1 => 0u8,
        2..=5 => 1,
        6 => 2,
        _ => 3,
    };
    let cmf = 0x78u8;
    let mut flg = flevel << 6;
    let rem = (u16::from(cmf) * 256 + u16::from(flg)) % 31;
    if rem != 0 {
        flg += (31 - rem) as u8;
    }

    let body = deflate(data, level);
    let mut out = Vec::with_capacity(2 + body.len() + 4);
    out.push(cmf);
    out.push(flg);
    out.extend_from_slice(&body);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}
