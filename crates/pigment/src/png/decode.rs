//! PNG decoder supporting colour types 0, 2, 3, 4, 6, bit depths 1/2/4/8/16, tRNS, and Adam7.

use alloc::vec;
use alloc::vec::Vec;

use crate::canvas::Canvas;
use crate::color::Rgba;
use crate::flate::{crc32, zlib_decompress};

use super::filter::unfilter_row;
use super::PngError;

/// Standard 8-byte PNG signature.
pub(crate) const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Adam7 pass parameters: `(x0, y0, dx, dy)`.
const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

#[derive(Clone, Copy)]
struct Ihdr {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
}

#[derive(Clone)]
enum Trns {
    Grey(u16),
    Rgb(u16, u16, u16),
    Palette(Vec<u8>),
}

fn channels(color_type: u8) -> u32 {
    match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => 0,
    }
}

fn parse_ihdr(d: &[u8], max_pixels: u64) -> Result<Ihdr, PngError> {
    if d.len() != 13 {
        return Err(PngError::Header);
    }
    let width = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    let height = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    if width == 0 || height == 0 || width > 0x7FFF_FFFF || height > 0x7FFF_FFFF {
        return Err(PngError::Header);
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > max_pixels {
        return Err(PngError::TooLarge);
    }
    let bit_depth = d[8];
    let color_type = d[9];
    let compression = d[10];
    let filter_method = d[11];
    let interlace = d[12];
    if compression != 0 || filter_method != 0 || interlace > 1 {
        return Err(PngError::Header);
    }
    let valid_depth = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(bit_depth, 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !valid_depth {
        return Err(PngError::Header);
    }
    Ok(Ihdr {
        width,
        height,
        bit_depth,
        color_type,
        interlace,
    })
}

fn pass_dims(w: u32, h: u32, x0: u32, y0: u32, dx: u32, dy: u32) -> (u32, u32) {
    let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
    let ph = if h > y0 { (h - y0).div_ceil(dy) } else { 0 };
    (pw, ph)
}

fn row_bytes_u64(width: u32, color_type: u8, bit_depth: u8) -> u64 {
    let bpp_bits = u64::from(width) * u64::from(channels(color_type)) * u64::from(bit_depth);
    bpp_bits.div_ceil(8)
}

fn expected_raw_len(hdr: Ihdr) -> Result<usize, PngError> {
    let total: u64 = if hdr.interlace == 0 {
        let rb = row_bytes_u64(hdr.width, hdr.color_type, hdr.bit_depth);
        u64::from(hdr.height)
            .checked_mul(rb + 1)
            .ok_or(PngError::TooLarge)?
    } else {
        let mut sum = 0u64;
        for &(x0, y0, dx, dy) in &ADAM7 {
            let (pw, ph) = pass_dims(hdr.width, hdr.height, x0, y0, dx, dy);
            if pw == 0 || ph == 0 {
                continue;
            }
            let rb = row_bytes_u64(pw, hdr.color_type, hdr.bit_depth);
            let pass_bytes = u64::from(ph)
                .checked_mul(rb + 1)
                .ok_or(PngError::TooLarge)?;
            sum = sum.checked_add(pass_bytes).ok_or(PngError::TooLarge)?;
        }
        sum
    };
    usize::try_from(total).map_err(|_| PngError::TooLarge)
}

/// Any valid PNG -> straight-alpha RGBA8 canvas. `max_pixels` bounds `width * height`.
pub fn decode_png(data: &[u8], max_pixels: u64) -> Result<Canvas, PngError> {
    if data.get(..8) != Some(&PNG_SIG) {
        return Err(PngError::Signature);
    }

    let mut pos = 8usize;
    let mut ihdr: Option<Ihdr> = None;
    let mut plte: Option<Vec<(u8, u8, u8)>> = None;
    let mut trns: Option<Trns> = None;
    let mut idat_slices: Vec<&[u8]> = Vec::new();
    let mut idat_done = false;
    let mut iend_seen = false;

    while pos < data.len() {
        let len_bytes = data.get(pos..pos + 4).ok_or(PngError::Chunk)?;
        let len = u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]);
        if len > 0x7FFF_FFFF {
            return Err(PngError::Chunk);
        }
        let type_slice = data.get(pos + 4..pos + 8).ok_or(PngError::Chunk)?;
        let ctype = [type_slice[0], type_slice[1], type_slice[2], type_slice[3]];
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(len as usize)
            .ok_or(PngError::Chunk)?;
        let payload = data.get(data_start..data_end).ok_or(PngError::Chunk)?;
        let crc_bytes = data.get(data_end..data_end + 4).ok_or(PngError::Chunk)?;
        let expected_crc =
            u32::from_be_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        let crc_span = data.get(pos + 4..data_end).ok_or(PngError::Chunk)?;
        if crc32(crc_span) != expected_crc {
            return Err(PngError::Crc);
        }
        pos = data_end + 4;

        if ihdr.is_none() && &ctype != b"IHDR" {
            return Err(PngError::Header);
        }
        if !idat_slices.is_empty() && &ctype != b"IDAT" {
            idat_done = true;
        }

        match &ctype {
            b"IHDR" => {
                if ihdr.is_some() {
                    return Err(PngError::Header);
                }
                ihdr = Some(parse_ihdr(payload, max_pixels)?);
            }
            b"PLTE" => {
                let hdr = ihdr.ok_or(PngError::Header)?;
                if plte.is_some()
                    || !idat_slices.is_empty()
                    || trns.is_some()
                    || hdr.color_type == 0
                    || hdr.color_type == 4
                    || payload.is_empty()
                    || payload.len() > 768
                    || payload.len() % 3 != 0
                {
                    return Err(PngError::Chunk);
                }
                let mut entries = Vec::with_capacity(payload.len() / 3);
                for rgb in payload.as_chunks::<3>().0 {
                    entries.push((rgb[0], rgb[1], rgb[2]));
                }
                plte = Some(entries);
            }
            b"tRNS" => {
                let hdr = ihdr.ok_or(PngError::Header)?;
                if trns.is_some() || !idat_slices.is_empty() {
                    return Err(PngError::Chunk);
                }
                trns = Some(match hdr.color_type {
                    0 => {
                        if payload.len() != 2 {
                            return Err(PngError::Chunk);
                        }
                        Trns::Grey(u16::from_be_bytes([payload[0], payload[1]]))
                    }
                    2 => {
                        if payload.len() != 6 {
                            return Err(PngError::Chunk);
                        }
                        Trns::Rgb(
                            u16::from_be_bytes([payload[0], payload[1]]),
                            u16::from_be_bytes([payload[2], payload[3]]),
                            u16::from_be_bytes([payload[4], payload[5]]),
                        )
                    }
                    3 => {
                        let Some(ref pal) = plte else {
                            return Err(PngError::Chunk);
                        };
                        if payload.len() > pal.len() {
                            return Err(PngError::Chunk);
                        }
                        Trns::Palette(payload.to_vec())
                    }
                    _ => return Err(PngError::Chunk),
                });
            }
            b"IDAT" => {
                let hdr = ihdr.ok_or(PngError::Header)?;
                if idat_done {
                    return Err(PngError::Chunk);
                }
                if hdr.color_type == 3 && plte.is_none() {
                    return Err(PngError::Data);
                }
                idat_slices.push(payload);
            }
            b"IEND" => {
                if !payload.is_empty() {
                    return Err(PngError::Chunk);
                }
                iend_seen = true;
                break;
            }
            _ => {
                if ctype[0] & 0x20 == 0 {
                    return Err(PngError::Unsupported);
                }
            }
        }
    }

    if !iend_seen {
        return Err(PngError::Chunk);
    }
    let hdr = ihdr.ok_or(PngError::Header)?;
    if idat_slices.is_empty() {
        return Err(PngError::Data);
    }

    let raw_len = expected_raw_len(hdr)?;
    let raw = if idat_slices.len() == 1 {
        zlib_decompress(idat_slices[0], raw_len).map_err(|_| PngError::Data)?
    } else {
        let total_idat: usize = idat_slices
            .iter()
            .try_fold(0usize, |acc, s| acc.checked_add(s.len()))
            .ok_or(PngError::TooLarge)?;
        let mut joined = Vec::new();
        joined
            .try_reserve_exact(total_idat)
            .map_err(|_| PngError::TooLarge)?;
        for s in idat_slices {
            joined.extend_from_slice(s);
        }
        zlib_decompress(&joined, raw_len).map_err(|_| PngError::Data)?
    };
    if raw.len() != raw_len {
        return Err(PngError::Data);
    }

    let total_px = (hdr.width as usize)
        .checked_mul(hdr.height as usize)
        .ok_or(PngError::TooLarge)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(total_px)
        .map_err(|_| PngError::TooLarge)?;
    pixels.resize(total_px, Rgba(0, 0, 0, 0));

    if hdr.interlace == 0 {
        decode_pass(
            &raw,
            hdr,
            plte.as_deref(),
            trns.as_ref(),
            (hdr.width, hdr.height, 0, 0, 1, 1),
            &mut pixels,
        )?;
    } else {
        let mut off = 0usize;
        for &(x0, y0, dx, dy) in &ADAM7 {
            let (pw, ph) = pass_dims(hdr.width, hdr.height, x0, y0, dx, dy);
            if pw == 0 || ph == 0 {
                continue;
            }
            let used = decode_pass(
                raw.get(off..).ok_or(PngError::Data)?,
                hdr,
                plte.as_deref(),
                trns.as_ref(),
                (pw, ph, x0, y0, dx, dy),
                &mut pixels,
            )?;
            off += used;
        }
    }

    Ok(Canvas {
        width: hdr.width,
        height: hdr.height,
        pixels,
    })
}

fn decode_pass(
    raw: &[u8],
    hdr: Ihdr,
    plte: Option<&[(u8, u8, u8)]>,
    trns: Option<&Trns>,
    pass: (u32, u32, u32, u32, u32, u32),
    pixels: &mut [Rgba],
) -> Result<usize, PngError> {
    let (pw, ph, x0, y0, dx, dy) = pass;
    let rb = row_bytes_u64(pw, hdr.color_type, hdr.bit_depth) as usize;
    let filter_bpp = ((channels(hdr.color_type) * u32::from(hdr.bit_depth)) / 8).max(1) as usize;

    let mut prev = vec![0u8; rb];
    let mut recon = vec![0u8; rb];
    let mut off = 0usize;

    for py in 0..ph {
        let filter_byte = *raw.get(off).ok_or(PngError::Data)?;
        let filt = raw.get(off + 1..off + 1 + rb).ok_or(PngError::Data)?;
        unfilter_row(filter_byte, filt, &prev, &mut recon, filter_bpp)?;
        let img_y = (y0 + py * dy) as usize;
        let row_base = img_y * (hdr.width as usize);
        unpack_row(&recon, hdr, plte, trns, pw, x0, dx, row_base, pixels)?;
        core::mem::swap(&mut prev, &mut recon);
        off += 1 + rb;
    }
    Ok(off)
}

#[allow(clippy::too_many_arguments)]
fn unpack_row(
    recon: &[u8],
    hdr: Ihdr,
    plte: Option<&[(u8, u8, u8)]>,
    trns: Option<&Trns>,
    pw: u32,
    x0: u32,
    dx: u32,
    row_base: usize,
    pixels: &mut [Rgba],
) -> Result<(), PngError> {
    let pw_usize = pw as usize;
    let put = |px: usize, rgba: Rgba, pixels: &mut [Rgba]| -> Result<(), PngError> {
        let img_x = (x0 as usize) + px * (dx as usize);
        let slot = pixels.get_mut(row_base + img_x).ok_or(PngError::Data)?;
        *slot = rgba;
        Ok(())
    };

    match (hdr.color_type, hdr.bit_depth) {
        (0, 1 | 2 | 4) => {
            let depth = hdr.bit_depth;
            let mask = ((1u16 << depth) - 1) as u8;
            let scale = match depth {
                1 => 255u8,
                2 => 85,
                _ => 17,
            };
            let per_byte = 8 / usize::from(depth);
            for px in 0..pw_usize {
                let byte = recon[px / per_byte];
                let shift = (per_byte - 1 - (px % per_byte)) * usize::from(depth);
                let s = (byte >> shift) & mask;
                let g = s * scale;
                let a = if matches!(trns, Some(&Trns::Grey(t)) if t == u16::from(s)) {
                    0
                } else {
                    255
                };
                put(px, Rgba(g, g, g, a), pixels)?;
            }
        }
        (0, 8) => {
            for (px, &s) in recon.iter().enumerate().take(pw_usize) {
                let a = if matches!(trns, Some(&Trns::Grey(t)) if t == u16::from(s)) {
                    0
                } else {
                    255
                };
                put(px, Rgba(s, s, s, a), pixels)?;
            }
        }
        (0, 16) => {
            for px in 0..pw_usize {
                let s = u16::from_be_bytes([recon[2 * px], recon[2 * px + 1]]);
                let g = (s >> 8) as u8;
                let a = if matches!(trns, Some(&Trns::Grey(t)) if t == s) {
                    0
                } else {
                    255
                };
                put(px, Rgba(g, g, g, a), pixels)?;
            }
        }
        (2, 8) => {
            for px in 0..pw_usize {
                let (r, g, b) = (recon[3 * px], recon[3 * px + 1], recon[3 * px + 2]);
                let a = if matches!(
                    trns,
                    Some(&Trns::Rgb(tr, tg, tb))
                        if tr == u16::from(r) && tg == u16::from(g) && tb == u16::from(b)
                ) {
                    0
                } else {
                    255
                };
                put(px, Rgba(r, g, b, a), pixels)?;
            }
        }
        (2, 16) => {
            for px in 0..pw_usize {
                let r16 = u16::from_be_bytes([recon[6 * px], recon[6 * px + 1]]);
                let g16 = u16::from_be_bytes([recon[6 * px + 2], recon[6 * px + 3]]);
                let b16 = u16::from_be_bytes([recon[6 * px + 4], recon[6 * px + 5]]);
                let a = if matches!(
                    trns,
                    Some(&Trns::Rgb(tr, tg, tb)) if tr == r16 && tg == g16 && tb == b16
                ) {
                    0
                } else {
                    255
                };
                put(
                    px,
                    Rgba((r16 >> 8) as u8, (g16 >> 8) as u8, (b16 >> 8) as u8, a),
                    pixels,
                )?;
            }
        }
        (3, 1 | 2 | 4 | 8) => {
            let pal = plte.ok_or(PngError::Data)?;
            let alphas = match trns {
                Some(Trns::Palette(v)) => v.as_slice(),
                _ => &[],
            };
            let depth = hdr.bit_depth;
            let mask = ((1u16 << depth) - 1) as u8;
            let per_byte = 8 / usize::from(depth);
            for px in 0..pw_usize {
                let byte = recon[px / per_byte];
                let shift = (per_byte - 1 - (px % per_byte)) * usize::from(depth);
                let idx = usize::from((byte >> shift) & mask);
                let &(r, g, b) = pal.get(idx).ok_or(PngError::Data)?;
                let a = alphas.get(idx).copied().unwrap_or(255);
                put(px, Rgba(r, g, b, a), pixels)?;
            }
        }
        (4, 8) => {
            for px in 0..pw_usize {
                let g = recon[2 * px];
                let a = recon[2 * px + 1];
                put(px, Rgba(g, g, g, a), pixels)?;
            }
        }
        (4, 16) => {
            for px in 0..pw_usize {
                let g = recon[4 * px];
                let a = recon[4 * px + 2];
                put(px, Rgba(g, g, g, a), pixels)?;
            }
        }
        (6, 8) => {
            for px in 0..pw_usize {
                put(
                    px,
                    Rgba(
                        recon[4 * px],
                        recon[4 * px + 1],
                        recon[4 * px + 2],
                        recon[4 * px + 3],
                    ),
                    pixels,
                )?;
            }
        }
        (6, 16) => {
            for px in 0..pw_usize {
                put(
                    px,
                    Rgba(
                        recon[8 * px],
                        recon[8 * px + 2],
                        recon[8 * px + 4],
                        recon[8 * px + 6],
                    ),
                    pixels,
                )?;
            }
        }
        _ => return Err(PngError::Header),
    }
    Ok(())
}
