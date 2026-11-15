//! Fuzz soak tests for pigment parsers, decoders, and text/font layout.

#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use pigment::{
    decode_png, deflate, encode_png, glyph, inflate, layout, measure, zlib_compress,
    zlib_decompress, Canvas, Rgba,
};

const DEFAULT_SEED: u64 = 0x05EE_D79B;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let s = s.trim().replace('_', "");
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).unwrap_or(DEFAULT_SEED)
            } else {
                s.parse::<u64>().unwrap_or(DEFAULT_SEED)
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_millis(ms)
}

fn mutate_bytes(rng: &mut Rng, a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() {
        return rng.bytes(64);
    }
    let mut out = a.to_vec();
    match rng.below(4) {
        0 => {
            // Bit flips
            let flips = 1 + rng.below(8);
            for _ in 0..flips {
                let idx = rng.below(out.len());
                let bit = 1u8 << rng.below(8);
                out[idx] ^= bit;
            }
        }
        1 => {
            // Truncation
            let new_len = rng.below(out.len());
            out.truncate(new_len);
        }
        2 => {
            // Splice from b
            let split_a = rng.below(out.len() + 1);
            out.truncate(split_a);
            if !b.is_empty() {
                let start_b = rng.below(b.len());
                let end_b = start_b + rng.below(b.len() - start_b + 1);
                out.extend_from_slice(&b[start_b..end_b]);
            }
        }
        _ => {
            // Repeated chunk
            let start = rng.below(out.len());
            let max_chunk = (out.len() - start).min(256);
            let end = start + 1 + rng.below(max_chunk);
            let chunk = out[start..end].to_vec();
            let reps = 2 + rng.below(6);
            let mut spliced = Vec::with_capacity(out.len() + chunk.len() * reps);
            spliced.extend_from_slice(&out[..end]);
            for _ in 0..reps {
                if spliced.len() + chunk.len() > 64 * 1024 {
                    break;
                }
                spliced.extend_from_slice(&chunk);
            }
            spliced.extend_from_slice(&out[end..]);
            out = spliced;
        }
    }
    out
}

fn gen_raw_data(rng: &mut Rng) -> Vec<u8> {
    match rng.below(4) {
        0 => Vec::new(),
        1 => {
            // Repetitive / compressible data
            let len = rng.below(4096);
            let pat = rng.bytes(16);
            if pat.is_empty() {
                vec![0u8; len]
            } else {
                pat.iter().copied().cycle().take(len).collect()
            }
        }
        2 => rng.bytes(512),
        _ => rng.bytes(4096),
    }
}

fn gen_canvas(rng: &mut Rng) -> Canvas {
    let w = 1 + rng.below(48) as u32;
    let h = 1 + rng.below(48) as u32;
    let fill = Rgba(
        rng.u64() as u8,
        rng.u64() as u8,
        rng.u64() as u8,
        rng.u64() as u8,
    );
    let mut c = Canvas::new(w, h, fill);
    if rng.chance(1, 2) {
        for p in &mut c.pixels {
            *p = Rgba(
                rng.u64() as u8,
                rng.u64() as u8,
                rng.u64() as u8,
                rng.u64() as u8,
            );
        }
    } else {
        let count = rng.below(16);
        for _ in 0..count {
            let x = rng.below(w as usize) as i32;
            let y = rng.below(h as usize) as i32;
            let col = Rgba(
                rng.u64() as u8,
                rng.u64() as u8,
                rng.u64() as u8,
                rng.u64() as u8,
            );
            c.blend(x, y, col);
        }
    }
    c
}

fn gen_text(rng: &mut Rng) -> String {
    let len = rng.below(256);
    let mut s = String::new();
    for _ in 0..len {
        match rng.below(10) {
            0 => s.push(' '),
            1 => s.push('\n'),
            2 => s.push_str("\r\n"),
            3 => s.push('\t'),
            4..=7 => {
                let ch = (0x20 + rng.below(0x5F)) as u8 as char;
                s.push(ch);
            }
            _ => {
                // Arbitrary valid Unicode scalar value
                loop {
                    let cp = (rng.u64() % 0x11_0000) as u32;
                    if let Some(ch) = char::from_u32(cp) {
                        s.push(ch);
                        break;
                    }
                }
            }
        }
    }
    s
}

#[test]
fn soak_inflate() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x1111_2222_3333_4444);
    let start = Instant::now();
    let mut cases = 0u64;
    let max_out = 1024 * 1024;

    let mut valid_pool: Vec<Vec<u8>> = (0..16)
        .map(|i| {
            let raw = gen_raw_data(&mut rng);
            deflate(&raw, (i % 10) as u8)
        })
        .collect();

    while start.elapsed() < budget || cases < 10 {
        let t0 = Instant::now();
        if cases.is_multiple_of(4) {
            let raw = gen_raw_data(&mut rng);
            let level = rng.below(10) as u8;
            let comp = deflate(&raw, level);
            let dec = inflate(&comp, max_out).unwrap();
            assert_eq!(dec, raw, "inflate round-trip failed at case {cases}");
            if valid_pool.len() < 64 {
                valid_pool.push(comp);
            }
        } else if cases.is_multiple_of(2) {
            let buf = rng.bytes(4096);
            let _ = inflate(&buf, max_out);
        } else {
            let a = rng.pick(&valid_pool).clone();
            let b = rng.pick(&valid_pool).clone();
            let mut_buf = mutate_bytes(&mut rng, &a, &b);
            let _ = inflate(&mut_buf, max_out);
        }
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "inflate case {cases} exceeded 1s"
        );
        cases += 1;
    }
    println!("soak target=inflate seed={seed:#x} cases={cases}");
}

#[test]
fn soak_zlib_decompress() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x5555_6666_7777_8888);
    let start = Instant::now();
    let mut cases = 0u64;
    let max_out = 1024 * 1024;

    let mut valid_pool: Vec<Vec<u8>> = (0..16)
        .map(|i| {
            let raw = gen_raw_data(&mut rng);
            zlib_compress(&raw, (i % 10) as u8)
        })
        .collect();

    while start.elapsed() < budget || cases < 10 {
        let t0 = Instant::now();
        if cases.is_multiple_of(4) {
            let raw = gen_raw_data(&mut rng);
            let level = rng.below(10) as u8;
            let comp = zlib_compress(&raw, level);
            let dec = zlib_decompress(&comp, max_out).unwrap();
            assert_eq!(dec, raw, "zlib round-trip failed at case {cases}");
            if valid_pool.len() < 64 {
                valid_pool.push(comp);
            }
        } else if cases.is_multiple_of(2) {
            let buf = rng.bytes(4096);
            let _ = zlib_decompress(&buf, max_out);
        } else {
            let a = rng.pick(&valid_pool).clone();
            let b = rng.pick(&valid_pool).clone();
            let mut_buf = mutate_bytes(&mut rng, &a, &b);
            let _ = zlib_decompress(&mut_buf, max_out);
        }
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "zlib_decompress case {cases} exceeded 1s"
        );
        cases += 1;
    }
    println!("soak target=zlib_decompress seed={seed:#x} cases={cases}");
}

#[test]
fn soak_decode_png() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0x9999_AAAA_BBBB_CCCC);
    let start = Instant::now();
    let mut cases = 0u64;
    let max_pixels = 1_048_576u64;

    let mut valid_pool: Vec<Vec<u8>> = (0..8)
        .map(|_| {
            let c = gen_canvas(&mut rng);
            encode_png(&c)
        })
        .collect();

    while start.elapsed() < budget || cases < 10 {
        let t0 = Instant::now();
        if cases.is_multiple_of(4) {
            let c = gen_canvas(&mut rng);
            let png = encode_png(&c);
            let dec = decode_png(&png, max_pixels).unwrap();
            assert_eq!(dec, c, "png round-trip failed at case {cases}");
            if valid_pool.len() < 32 {
                valid_pool.push(png);
            }
        } else if cases.is_multiple_of(2) {
            let buf = rng.bytes(4096);
            let _ = decode_png(&buf, max_pixels);
        } else {
            let a = rng.pick(&valid_pool).clone();
            let b = rng.pick(&valid_pool).clone();
            let mut_buf = mutate_bytes(&mut rng, &a, &b);
            let _ = decode_png(&mut_buf, max_pixels);
        }
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "decode_png case {cases} exceeded 1s"
        );
        cases += 1;
    }
    println!("soak target=decode_png seed={seed:#x} cases={cases}");
}

#[test]
fn soak_text_and_font() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0xDDDD_EEEE_FFFF_0000);
    let start = Instant::now();
    let mut cases = 0u64;

    let valid_texts: Vec<String> = (0..16).map(|_| gen_text(&mut rng)).collect();
    let mut canvas = Canvas::new(64, 64, Rgba(0, 0, 0, 255));

    while start.elapsed() < budget || cases < 10 {
        let t0 = Instant::now();
        let text = if cases.is_multiple_of(2) {
            // Purely random bytes interpreted lossily as UTF-8
            let bytes = rng.bytes(256);
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            // Mutated valid text
            let a = rng.pick(&valid_texts).as_bytes();
            let b = rng.pick(&valid_texts).as_bytes();
            let mut_bytes = mutate_bytes(&mut rng, a, b);
            String::from_utf8_lossy(&mut_bytes).into_owned()
        };

        let scale = match rng.below(5) {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 16,
            _ => u32::MAX,
        };
        let max_width = match rng.below(5) {
            0 => 0,
            1 => 1,
            2 => 8,
            3 => 100,
            _ => u32::MAX,
        };

        let _ = measure(&text, scale);
        let lines = layout(&text, max_width, scale);
        for line in &lines {
            assert!(line.start <= line.end && line.end <= text.len());
            assert!(text.is_char_boundary(line.start));
            assert!(text.is_char_boundary(line.end));
        }

        if let Some(ch) = text.chars().next() {
            let _ = glyph(ch);
        }
        let x = rng.range(0, 100) as i32 - 50;
        let y = rng.range(0, 100) as i32 - 50;
        let _ = canvas.draw_text(x, y, &text, Rgba(255, 255, 255, 255), scale, max_width);

        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "text_and_font case {cases} exceeded 1s"
        );
        cases += 1;
    }
    println!("soak target=text_and_font seed={seed:#x} cases={cases}");
}
