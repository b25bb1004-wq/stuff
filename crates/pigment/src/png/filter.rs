//! PNG scanline filters (None, Sub, Up, Average, Paeth).

use alloc::vec::Vec;

use super::PngError;

/// Paeth predictor as specified in RFC 2083 section 6.6.
#[inline]
pub(crate) fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a = i32::from(a);
    let b = i32::from(b);
    let c = i32::from(c);
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

/// Reconstruct one filtered scanline `filt` into `out` given `prev` reconstructed row.
pub(crate) fn unfilter_row(
    filter: u8,
    filt: &[u8],
    prev: &[u8],
    out: &mut [u8],
    bpp: usize,
) -> Result<(), PngError> {
    if filt.len() != out.len() || prev.len() != out.len() {
        return Err(PngError::Data);
    }
    match filter {
        0 => out.copy_from_slice(filt),
        1 => {
            for x in 0..filt.len() {
                let left = if x >= bpp { out[x - bpp] } else { 0 };
                out[x] = filt[x].wrapping_add(left);
            }
        }
        2 => {
            for x in 0..filt.len() {
                out[x] = filt[x].wrapping_add(prev[x]);
            }
        }
        3 => {
            for x in 0..filt.len() {
                let left = if x >= bpp { out[x - bpp] } else { 0 };
                let avg = ((u16::from(left) + u16::from(prev[x])) / 2) as u8;
                out[x] = filt[x].wrapping_add(avg);
            }
        }
        4 => {
            for x in 0..filt.len() {
                let (left, up_left) = if x >= bpp {
                    (out[x - bpp], prev[x - bpp])
                } else {
                    (0, 0)
                };
                out[x] = filt[x].wrapping_add(paeth(left, prev[x], up_left));
            }
        }
        _ => return Err(PngError::Data),
    }
    Ok(())
}

/// Apply all 5 PNG filters to `cur` and append the one with smallest sum of `|signed byte|`.
pub(crate) fn filter_row_best(
    cur: &[u8],
    prev: &[u8],
    bpp: usize,
    best: &mut Vec<u8>,
    cand: &mut Vec<u8>,
    out: &mut Vec<u8>,
) {
    let n = cur.len();
    best.resize(n, 0);
    cand.resize(n, 0);
    let mut best_score = u64::MAX;
    let mut best_filter = 0u8;

    for f in 0..=4u8 {
        let mut score = 0u64;
        for x in 0..n {
            let pred = match f {
                0 => 0,
                1 => {
                    if x >= bpp {
                        cur[x - bpp]
                    } else {
                        0
                    }
                }
                2 => prev[x],
                3 => {
                    let left = if x >= bpp { cur[x - bpp] } else { 0 };
                    ((u16::from(left) + u16::from(prev[x])) / 2) as u8
                }
                _ => {
                    let (left, up_left) = if x >= bpp {
                        (cur[x - bpp], prev[x - bpp])
                    } else {
                        (0, 0)
                    };
                    paeth(left, prev[x], up_left)
                }
            };
            let b = cur[x].wrapping_sub(pred);
            cand[x] = b;
            score += u64::from((b as i8).unsigned_abs());
        }
        if f == 0 || score < best_score {
            best_score = score;
            best_filter = f;
            core::mem::swap(best, cand);
        }
    }

    out.push(best_filter);
    out.extend_from_slice(best);
}
