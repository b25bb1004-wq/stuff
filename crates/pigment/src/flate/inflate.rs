//! RFC 1951 DEFLATE decompressor.

use alloc::vec::Vec;

use super::huffman::{
    HuffDecoder, BL_ORDER, DIST_BASE, DIST_EXTRA, FIXED_DIST_LENS, FIXED_LIT_LENS, LEN_BASE,
    LEN_EXTRA,
};
use super::FlateError;

/// LSB-first bitstream reader over a byte slice.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        }
    }

    #[inline]
    fn fill(&mut self) {
        while self.cnt <= 56 {
            let Some(&b) = self.data.get(self.pos) else {
                break;
            };
            self.buf |= u64::from(b) << self.cnt;
            self.cnt += 8;
            self.pos += 1;
        }
    }

    #[inline]
    fn read_bits(&mut self, n: u8) -> Result<u32, FlateError> {
        if n == 0 {
            return Ok(0);
        }
        if self.cnt < n {
            self.fill();
        }
        if self.cnt < n {
            return Err(FlateError::Truncated);
        }
        let mask = (1u64 << n) - 1;
        let val = (self.buf & mask) as u32;
        self.buf >>= n;
        self.cnt -= n;
        Ok(val)
    }

    /// Align to the next byte boundary and return the byte offset in `data`.
    fn align_byte(&mut self) -> usize {
        let drop = self.cnt & 7;
        self.buf >>= drop;
        self.cnt -= drop;
        let unread_bytes = usize::from(self.cnt / 8);
        self.pos = self.pos.saturating_sub(unread_bytes);
        self.buf = 0;
        self.cnt = 0;
        self.pos
    }

    #[inline]
    fn decode_sym(&mut self, dec: &HuffDecoder) -> Result<u16, FlateError> {
        if dec.max_len == 0 {
            return Err(FlateError::Invalid);
        }
        if self.cnt < 15 {
            self.fill();
        }
        if self.cnt == 0 {
            return Err(FlateError::Truncated);
        }
        let entry = dec.fast[(self.buf & 511) as usize];
        if entry != 0 {
            let len = (entry >> 16) as u8;
            if self.cnt < len {
                return Err(FlateError::Truncated);
            }
            self.buf >>= len;
            self.cnt -= len;
            return Ok((entry & 0xFFFF) as u16);
        }
        let mut code: i32 = 0;
        let mut len: usize = 1;
        while len <= usize::from(dec.max_len) {
            if self.cnt == 0 {
                self.fill();
                if self.cnt == 0 {
                    return Err(FlateError::Truncated);
                }
            }
            code = (code << 1) | ((self.buf & 1) as i32);
            self.buf >>= 1;
            self.cnt -= 1;
            let count = i32::from(dec.count[len]);
            let first = i32::from(dec.first_code[len]);
            if code >= first && code - first < count {
                let idx = usize::from(dec.first_sym[len]) + (code - first) as usize;
                return dec.syms.get(idx).copied().ok_or(FlateError::Invalid);
            }
            len += 1;
        }
        Err(FlateError::Invalid)
    }
}

/// Grow `out` safely without exceeding `max_out`.
#[inline]
fn ensure_cap(out: &mut Vec<u8>, extra: usize, max_out: usize) -> Result<(), FlateError> {
    let needed = out.len().checked_add(extra).ok_or(FlateError::TooLarge)?;
    if needed > max_out {
        return Err(FlateError::TooLarge);
    }
    if needed > out.capacity() {
        let target = out
            .capacity()
            .saturating_mul(2)
            .max(needed)
            .max(256)
            .min(max_out);
        out.try_reserve_exact(target - out.len())
            .map_err(|_| FlateError::TooLarge)?;
    }
    Ok(())
}

/// Raw DEFLATE (RFC 1951). Output longer than `max_out` -> `Err(TooLarge)` (bomb guard).
pub fn inflate(data: &[u8], max_out: usize) -> Result<Vec<u8>, FlateError> {
    let (out, _) = inflate_with_len(data, max_out)?;
    Ok(out)
}

/// Raw DEFLATE returning both the output bytes and the number of input bytes consumed.
pub(crate) fn inflate_with_len(
    data: &[u8],
    max_out: usize,
) -> Result<(Vec<u8>, usize), FlateError> {
    let mut br = BitReader::new(data);
    let mut out = Vec::new();

    loop {
        let bfinal = br.read_bits(1)?;
        let btype = br.read_bits(2)?;
        match btype {
            0 => decode_stored(&mut br, &mut out, max_out)?,
            1 => {
                let lit_dec = HuffDecoder::build(&FIXED_LIT_LENS, false)?;
                let dist_dec = HuffDecoder::build(&FIXED_DIST_LENS, false)?;
                decode_huffman(&mut br, &lit_dec, &dist_dec, &mut out, max_out)?;
            }
            2 => {
                let (lit_dec, dist_dec) = read_dynamic_trees(&mut br)?;
                decode_huffman(&mut br, &lit_dec, &dist_dec, &mut out, max_out)?;
            }
            _ => return Err(FlateError::Invalid),
        }
        if bfinal == 1 {
            break;
        }
    }

    let consumed = br.align_byte();
    Ok((out, consumed))
}

fn decode_stored(
    br: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    max_out: usize,
) -> Result<(), FlateError> {
    let pos = br.align_byte();
    let hdr = br.data.get(pos..pos + 4).ok_or(FlateError::Truncated)?;
    let len = u16::from_le_bytes([hdr[0], hdr[1]]);
    let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
    if len ^ 0xFFFF != nlen {
        return Err(FlateError::Invalid);
    }
    let start = pos + 4;
    let end = start + usize::from(len);
    let payload = br.data.get(start..end).ok_or(FlateError::Truncated)?;
    ensure_cap(out, usize::from(len), max_out)?;
    out.extend_from_slice(payload);
    br.pos = end;
    Ok(())
}

fn read_dynamic_trees(br: &mut BitReader<'_>) -> Result<(HuffDecoder, HuffDecoder), FlateError> {
    let hlit = br.read_bits(5)? as usize + 257;
    if hlit > 286 {
        return Err(FlateError::Invalid);
    }
    let hdist = br.read_bits(5)? as usize + 1;
    let hclen = br.read_bits(4)? as usize + 4;

    let mut bl_lens = [0u8; 19];
    for &idx in BL_ORDER.iter().take(hclen) {
        bl_lens[idx] = br.read_bits(3)? as u8;
    }
    let bl_dec = HuffDecoder::build(&bl_lens, false)?;

    let total = hlit + hdist;
    let mut lens = [0u8; 286 + 32];
    let mut i = 0usize;
    while i < total {
        let sym = br.decode_sym(&bl_dec)?;
        match sym {
            0..=15 => {
                lens[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(FlateError::Invalid);
                }
                let prev = lens[i - 1];
                let rep = 3 + br.read_bits(2)? as usize;
                if i + rep > total {
                    return Err(FlateError::Invalid);
                }
                lens[i..i + rep].fill(prev);
                i += rep;
            }
            17 => {
                let rep = 3 + br.read_bits(3)? as usize;
                if i + rep > total {
                    return Err(FlateError::Invalid);
                }
                lens[i..i + rep].fill(0);
                i += rep;
            }
            18 => {
                let rep = 11 + br.read_bits(7)? as usize;
                if i + rep > total {
                    return Err(FlateError::Invalid);
                }
                lens[i..i + rep].fill(0);
                i += rep;
            }
            _ => return Err(FlateError::Invalid),
        }
    }

    if lens[256] == 0 {
        return Err(FlateError::Invalid);
    }
    let lit_dec = HuffDecoder::build(&lens[..hlit], false)?;
    let dist_dec = HuffDecoder::build(&lens[hlit..total], true)?;
    Ok((lit_dec, dist_dec))
}

fn decode_huffman(
    br: &mut BitReader<'_>,
    lit_dec: &HuffDecoder,
    dist_dec: &HuffDecoder,
    out: &mut Vec<u8>,
    max_out: usize,
) -> Result<(), FlateError> {
    loop {
        let sym = br.decode_sym(lit_dec)?;
        if sym < 256 {
            ensure_cap(out, 1, max_out)?;
            out.push(sym as u8);
            continue;
        }
        if sym == 256 {
            return Ok(());
        }
        if sym > 285 {
            return Err(FlateError::Invalid);
        }
        let lidx = usize::from(sym - 257);
        let len = usize::from(LEN_BASE[lidx]) + br.read_bits(LEN_EXTRA[lidx])? as usize;

        let dsym = br.decode_sym(dist_dec)?;
        if dsym > 29 {
            return Err(FlateError::Invalid);
        }
        let didx = usize::from(dsym);
        let dist = usize::from(DIST_BASE[didx]) + br.read_bits(DIST_EXTRA[didx])? as usize;
        if dist > out.len() {
            return Err(FlateError::Invalid);
        }

        ensure_cap(out, len, max_out)?;
        let start = out.len() - dist;
        for j in 0..len {
            let b = *out.get(start + j).ok_or(FlateError::Invalid)?;
            out.push(b);
        }
    }
}
