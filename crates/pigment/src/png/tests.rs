#![allow(clippy::unwrap_used)]

use super::*;
use crate::canvas::Canvas;
use crate::color::Rgba;
use crate::flate::{crc32, zlib_compress};
use alloc::vec;
use alloc::vec::Vec;
use assay::forall;

fn build_png(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    for &(ctype, data) in chunks {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(ctype);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

fn ihdr_bytes(w: u32, h: u32, depth: u8, color_type: u8, interlace: u8) -> [u8; 13] {
    let mut b = [0u8; 13];
    b[0..4].copy_from_slice(&w.to_be_bytes());
    b[4..8].copy_from_slice(&h.to_be_bytes());
    b[8] = depth;
    b[9] = color_type;
    b[12] = interlace;
    b
}

#[test]
fn handbuilt_palette_1bit_with_trns() {
    // 5x2 1-bit indexed image:
    // Palette: 0 = (255, 0, 0), 1 = (0, 255, 0); tRNS: [128] (index 0 has alpha 128, index 1 has 255)
    // Row 0: indices [0, 1, 0, 1, 1] -> MSB-first byte: 0b01011_000 = 0x58
    // Row 1: indices [1, 1, 0, 0, 1] -> MSB-first byte: 0b11001_000 = 0xC8
    let ihdr = ihdr_bytes(5, 2, 1, 3, 0);
    let plte = [255, 0, 0, 0, 255, 0];
    let trns = [128];
    let raw = [0u8, 0x58, 0u8, 0xC8];
    let idat = zlib_compress(&raw, 6);
    let png = build_png(&[
        (b"IHDR", &ihdr),
        (b"gAMA", &45455u32.to_be_bytes()),
        (b"PLTE", &plte),
        (b"tRNS", &trns),
        (b"IDAT", &idat),
        (b"IEND", &[]),
    ]);

    let c = decode_png(&png, 100).unwrap();
    assert_eq!(c.width, 5);
    assert_eq!(c.height, 2);
    let r = Rgba(255, 0, 0, 128);
    let g = Rgba(0, 255, 0, 255);
    assert_eq!(c.pixels, vec![r, g, r, g, g, g, g, r, r, g]);
}

#[test]
fn handbuilt_grey_16bit_with_trns() {
    // 3x1 16-bit greyscale with tRNS = 0x1234:
    // Pixel 0: 0x1234 -> matches tRNS -> Rgba(0x12, 0x12, 0x12, 0)
    // Pixel 1: 0x1200 -> does NOT match tRNS (even though high byte is 0x12) -> Rgba(0x12, 0x12, 0x12, 255)
    // Pixel 2: 0xABCD -> Rgba(0xAB, 0xAB, 0xAB, 255)
    let ihdr = ihdr_bytes(3, 1, 16, 0, 0);
    let trns = [0x12, 0x34];
    let raw = [0u8, 0x12, 0x34, 0x12, 0x00, 0xAB, 0xCD];
    let idat = zlib_compress(&raw, 6);
    // Split IDAT across two chunks to test multi-IDAT reassembly
    let split = idat.len() / 2;
    let png = build_png(&[
        (b"IHDR", &ihdr),
        (b"tRNS", &trns),
        (b"IDAT", &idat[..split]),
        (b"IDAT", &idat[split..]),
        (b"IEND", &[]),
    ]);

    let c = decode_png(&png, 100).unwrap();
    assert_eq!(
        c.pixels,
        vec![
            Rgba(0x12, 0x12, 0x12, 0),
            Rgba(0x12, 0x12, 0x12, 255),
            Rgba(0xAB, 0xAB, 0xAB, 255),
        ]
    );
}

#[test]
fn handbuilt_adam7_rgb_and_filters() {
    // 3x3 Adam7 RGB image (passes with 0 width/height exist: pass 1 (x0=4) and pass 2 (y0=4) are empty!)
    // Pass 0 (0,0,8,8): 1x1 -> (0,0) = (10, 20, 30)
    // Pass 1 (4,0,8,8): 0x1 -> empty
    // Pass 2 (0,4,4,8): 1x0 -> empty
    // Pass 3 (2,0,4,4): 1x1 -> (2,0) = (40, 50, 60)
    // Pass 4 (0,2,2,4): 2x1 -> (0,2) = (70, 80, 90), (2,2) = (100, 110, 120)
    // Pass 5 (1,0,2,2): 1x2 -> (1,0) = (11, 22, 33), (1,2) = (44, 55, 66)
    // Pass 6 (0,1,1,2): 3x1 -> (0,1) = (1, 2, 3), (1,1) = (4, 5, 6), (2,1) = (7, 8, 9)
    let ihdr = ihdr_bytes(3, 3, 8, 2, 1);
    let mut raw = Vec::new();
    // Pass 0 (None filter)
    raw.extend_from_slice(&[0, 10, 20, 30]);
    // Pass 3 (None filter)
    raw.extend_from_slice(&[0, 40, 50, 60]);
    // Pass 4: 2 pixels, use Sub filter (1): px0=(70,80,90), px1 raw diff=(30,30,30) -> (100,110,120)
    raw.extend_from_slice(&[1, 70, 80, 90, 30, 30, 30]);
    // Pass 5: 1x2: row0 None=(11,22,33), row1 Up filter (2) diff=(33,33,33) -> (44,55,66)
    raw.extend_from_slice(&[0, 11, 22, 33, 2, 33, 33, 33]);
    // Pass 6: 3x1: Average filter (3) with prev=0 -> avg=(left/2):
    // px0=(1,2,3); px1=(4,5,6) - (0,1,1) = (4,4,5); px2=(7,8,9) - (2,2,3) = (5,6,6)
    raw.extend_from_slice(&[3, 1, 2, 3, 4, 4, 5, 5, 6, 6]);

    let idat = zlib_compress(&raw, 6);
    let png = build_png(&[(b"IHDR", &ihdr), (b"IDAT", &idat), (b"IEND", &[])]);

    let c = decode_png(&png, 100).unwrap();
    assert_eq!(c.width, 3);
    assert_eq!(c.height, 3);
    assert_eq!(
        c.pixels,
        vec![
            Rgba(10, 20, 30, 255),
            Rgba(11, 22, 33, 255),
            Rgba(40, 50, 60, 255),
            Rgba(1, 2, 3, 255),
            Rgba(4, 5, 6, 255),
            Rgba(7, 8, 9, 255),
            Rgba(70, 80, 90, 255),
            Rgba(44, 55, 66, 255),
            Rgba(100, 110, 120, 255),
        ]
    );
}

#[test]
fn grey_subbyte_and_grey_alpha_and_rgb_trns() {
    // Colour type 0, 2-bit: 4 pixels [0, 1, 2, 3] -> byte 0b00_01_10_11 = 0x1B
    let png0 = build_png(&[
        (b"IHDR", &ihdr_bytes(4, 1, 2, 0, 0)),
        (b"IDAT", &zlib_compress(&[0, 0x1B], 6)),
        (b"IEND", &[]),
    ]);
    let c0 = decode_png(&png0, 10).unwrap();
    assert_eq!(
        c0.pixels,
        vec![
            Rgba(0, 0, 0, 255),
            Rgba(85, 85, 85, 255),
            Rgba(170, 170, 170, 255),
            Rgba(255, 255, 255, 255),
        ]
    );

    // Colour type 4, 8-bit grey+alpha
    let png4 = build_png(&[
        (b"IHDR", &ihdr_bytes(2, 1, 8, 4, 0)),
        (b"IDAT", &zlib_compress(&[0, 100, 50, 200, 250], 6)),
        (b"IEND", &[]),
    ]);
    let c4 = decode_png(&png4, 10).unwrap();
    assert_eq!(
        c4.pixels,
        vec![Rgba(100, 100, 100, 50), Rgba(200, 200, 200, 250)]
    );

    // Colour type 2, 8-bit RGB with tRNS
    let png2 = build_png(&[
        (b"IHDR", &ihdr_bytes(2, 1, 8, 2, 0)),
        (b"tRNS", &[0, 1, 0, 2, 0, 3]),
        (b"IDAT", &zlib_compress(&[0, 1, 2, 3, 4, 5, 6], 6)),
        (b"IEND", &[]),
    ]);
    let c2 = decode_png(&png2, 10).unwrap();
    assert_eq!(c2.pixels, vec![Rgba(1, 2, 3, 0), Rgba(4, 5, 6, 255)]);

    // Colour type 6, 16-bit RGBA + Paeth filter (4) on row 1
    let raw6 = [
        0, 0x10, 0x11, 0x20, 0x22, 0x30, 0x33, 0x40, 0x44, 0x50, 0x55, 0x60, 0x66, 0x70, 0x77,
        0x80, 0x88, 4, 0x01, 0xEF, 0x01, 0xDE, 0x01, 0xCD, 0x01, 0xBC, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let png6 = build_png(&[
        (b"IHDR", &ihdr_bytes(2, 2, 16, 6, 0)),
        (b"IDAT", &zlib_compress(&raw6, 6)),
        (b"IEND", &[]),
    ]);
    let c6 = decode_png(&png6, 10).unwrap();
    assert_eq!(
        c6.pixels[..3],
        [
            Rgba(0x10, 0x20, 0x30, 0x40),
            Rgba(0x50, 0x60, 0x70, 0x80),
            Rgba(0x11, 0x21, 0x31, 0x41),
        ]
    );
}

#[test]
fn encode_png_emits_ihdr_idat_iend() {
    let c = Canvas::new(4, 3, Rgba(12, 34, 56, 200));
    let png = encode_png(&c);
    assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut pos = 8;
    let mut types = Vec::new();
    while pos < png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let ctype: [u8; 4] = png[pos + 4..pos + 8].try_into().unwrap();
        types.push(ctype);
        pos += 12 + len;
    }
    assert_eq!(pos, png.len());
    assert_eq!(types, vec![*b"IHDR", *b"IDAT", *b"IEND"]);
}

#[test]
fn png_error_variants() {
    assert_eq!(decode_png(b"not a png", 100), Err(PngError::Signature));

    let valid = encode_png(&Canvas::new(2, 2, Rgba(10, 20, 30, 40)));
    assert_eq!(decode_png(&valid, 3), Err(PngError::TooLarge));
    assert_eq!(decode_png(&valid, 4).unwrap().width, 2);

    // Corrupt CRC in IHDR
    let mut bad_crc = valid.clone();
    bad_crc[29] ^= 1;
    assert_eq!(decode_png(&bad_crc, 100), Err(PngError::Crc));

    // Unknown critical chunk (uppercase first byte -> bit 5 == 0) -> Unsupported
    let ihdr = ihdr_bytes(1, 1, 8, 6, 0);
    let idat = zlib_compress(&[0, 1, 2, 3, 4], 6);
    let crit_png = build_png(&[
        (b"IHDR", &ihdr),
        (b"CRIT", b"hello"),
        (b"IDAT", &idat),
        (b"IEND", &[]),
    ]);
    assert_eq!(decode_png(&crit_png, 100), Err(PngError::Unsupported));

    // Invalid bit depth for color type 6 -> Header
    let bad_hdr = build_png(&[
        (b"IHDR", &ihdr_bytes(1, 1, 4, 6, 0)),
        (b"IDAT", &idat),
        (b"IEND", &[]),
    ]);
    assert_eq!(decode_png(&bad_hdr, 100), Err(PngError::Header));

    // Truncated before IEND -> Chunk
    assert_eq!(
        decode_png(&valid[..valid.len() - 4], 100),
        Err(PngError::Chunk)
    );

    // Bad filter byte 5 -> Data
    let bad_filt = build_png(&[
        (b"IHDR", &ihdr),
        (b"IDAT", &zlib_compress(&[5, 1, 2, 3, 4], 6)),
        (b"IEND", &[]),
    ]);
    assert_eq!(decode_png(&bad_filt, 100), Err(PngError::Data));
}

#[test]
fn forall_encode_decode_round_trip() {
    forall(100, 0x59_9600_0001, |r| {
        let w = r.range(1, 71) as u32;
        let h = r.range(1, 51) as u32;
        let mut c = Canvas::new(w, h, Rgba(0, 0, 0, 0));
        if r.chance(1, 2) {
            // Structured gradients + repeats to exercise all 5 filters
            let base = r.u64().to_le_bytes();
            for y in 0..h {
                for x in 0..w {
                    let idx = (y * w + x) as usize;
                    c.pixels[idx] = Rgba(
                        base[0].wrapping_add((x * 3 + y * 5) as u8),
                        base[1].wrapping_add((x ^ y) as u8),
                        base[2].wrapping_add(y as u8),
                        base[3].wrapping_add(x as u8),
                    );
                }
            }
        } else {
            for p in &mut c.pixels {
                let v = r.u64().to_le_bytes();
                *p = Rgba(v[0], v[1], v[2], v[3]);
            }
        }
        let encoded = encode_png(&c);
        let decoded = decode_png(&encoded, u64::from(w) * u64::from(h)).unwrap();
        assert_eq!(decoded, c);
    });
}

#[test]
fn forall_fuzz_decode_png_never_panics() {
    let sample = encode_png(&Canvas::new(4, 4, Rgba(12, 34, 56, 78)));
    forall(1000, 0x59_9600_0002, |r| {
        if r.chance(1, 2) {
            let garbage = r.bytes(256);
            let _ = decode_png(&garbage, 4096);
        } else {
            let mut mutated = sample.clone();
            if r.chance(1, 2) && !mutated.is_empty() {
                let trunc = r.below(mutated.len());
                mutated.truncate(trunc);
            } else if !mutated.is_empty() {
                let flips = r.range(1, 6);
                for _ in 0..flips {
                    let idx = r.below(mutated.len());
                    mutated[idx] ^= 1u8 << (r.below(8) as u8);
                }
            }
            let _ = decode_png(&mutated, 4096);
        }
    });
}
