//! PNG encoder (RGBA8, colour type 6, bit depth 8, adaptive row filter, zlib level 6).

use alloc::vec;
use alloc::vec::Vec;

use crate::canvas::Canvas;
use crate::flate::{crc32, zlib_compress};

use super::decode::PNG_SIG;
use super::filter::filter_row_best;

fn write_chunk(out: &mut Vec<u8>, ctype: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(ctype);
    out.extend_from_slice(data);
    let crc = crc32(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encode `c` as an RGBA8 (colour type 6, bit depth 8, non-interlaced) PNG.
pub fn encode_png(c: &Canvas) -> Vec<u8> {
    let w = c.width as usize;
    let h = c.height as usize;
    let row_bytes = w.saturating_mul(4);

    let mut raw = Vec::with_capacity(h.saturating_mul(row_bytes.saturating_add(1)));
    let mut prev = vec![0u8; row_bytes];
    let mut cur = vec![0u8; row_bytes];
    let mut best = Vec::with_capacity(row_bytes);
    let mut cand = Vec::with_capacity(row_bytes);

    for y in 0..h {
        let row_start = y.saturating_mul(w);
        for x in 0..w {
            let p = c
                .pixels
                .get(row_start + x)
                .copied()
                .unwrap_or(crate::color::Rgba(0, 0, 0, 0));
            let base = x * 4;
            cur[base] = p.0;
            cur[base + 1] = p.1;
            cur[base + 2] = p.2;
            cur[base + 3] = p.3;
        }
        filter_row_best(&cur, &prev, 4, &mut best, &mut cand, &mut raw);
        core::mem::swap(&mut prev, &mut cur);
    }

    let compressed = zlib_compress(&raw, 6);

    let mut out = Vec::with_capacity(8 + 25 + 12 + compressed.len() + 12);
    out.extend_from_slice(&PNG_SIG);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&c.width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&c.height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    ihdr[10] = 0;
    ihdr[11] = 0;
    ihdr[12] = 0;
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &compressed);
    write_chunk(&mut out, b"IEND", &[]);
    out
}
