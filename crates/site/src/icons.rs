//! PNG app icon generator for Monolith apps using an embedded 5x7 bitmap font.

use std::f32::consts::FRAC_PI_2;

use pigment::{encode_png, Canvas, Rect, Rgba};

const BG: Rgba = Rgba(0x0e, 0x10, 0x13, 255);
const LINE: Rgba = Rgba(0x26, 0x2c, 0x34, 255);
const ACCENT: Rgba = Rgba(0xe0, 0xa4, 0x58, 255);
const TRANSPARENT: Rgba = Rgba(0, 0, 0, 0);

const GLYPHS_AZ: [[u8; 7]; 26] = [
    [
        0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ], // A
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
    ], // B
    [
        0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
    ], // C
    [
        0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100,
    ], // D
    [
        0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
    ], // E
    [
        0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
    ], // F
    [
        0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
    ], // G
    [
        0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ], // H
    [
        0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ], // I
    [
        0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
    ], // J
    [
        0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
    ], // K
    [
        0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
    ], // L
    [
        0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
    ], // M
    [
        0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
    ], // N
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ], // O
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
    ], // P
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
    ], // Q
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
    ], // R
    [
        0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
    ], // S
    [
        0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
    ], // T
    [
        0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ], // U
    [
        0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
    ], // V
    [
        0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
    ], // W
    [
        0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
    ], // X
    [
        0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
    ], // Y
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
    ], // Z
];

fn glyph_for(ch: char) -> [u8; 7] {
    let upper = ch.to_ascii_uppercase();
    if upper.is_ascii_uppercase() {
        let idx = usize::from(upper as u8 - b'A');
        GLYPHS_AZ[idx]
    } else {
        // Fallback solid block for unexpected non-ASCII initials.
        [0b11111; 7]
    }
}

fn rounded_rect_points(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) -> Vec<(f32, f32)> {
    let r = radius
        .min((right - left) * 0.5)
        .min((bottom - top) * 0.5)
        .max(0.0);
    let steps = 6usize;
    let mut pts = Vec::with_capacity((steps + 1) * 4);
    let corners = [
        (right - r, top + r, -FRAC_PI_2),
        (right - r, bottom - r, 0.0),
        (left + r, bottom - r, FRAC_PI_2),
        (left + r, top + r, std::f32::consts::PI),
    ];
    for (cx, cy, start_angle) in corners {
        for i in 0..=steps {
            let t = (i as f32) / (steps as f32);
            let angle = start_angle + t * FRAC_PI_2;
            pts.push((cx + r * angle.cos(), cy + r * angle.sin()));
        }
    }
    pts
}

/// Render a `size`x`size` PNG icon with a `--bg` rounded square, thin `--line` ring,
/// and the uppercase `initial` scaled up from the 5x7 bitmap font in `--accent`.
pub(crate) fn render_icon_png(initial: char, size: u32, maskable: bool) -> Vec<u8> {
    let s = size as f32;
    let mut canvas = if maskable {
        Canvas::new(size, size, BG)
    } else {
        Canvas::new(size, size, TRANSPARENT)
    };

    if !maskable {
        let outer_bg = rounded_rect_points(0.0, 0.0, s, s, s * 0.18);
        canvas.fill_polygon(&outer_bg, BG);
    }

    let ring_pad = if maskable { s * 0.14 } else { s * 0.06 };
    let ring_radius = if maskable { s * 0.15 } else { s * 0.14 };
    let ring_stroke = (s * 0.024).max(2.0);
    let ring_outer =
        rounded_rect_points(ring_pad, ring_pad, s - ring_pad, s - ring_pad, ring_radius);
    canvas.fill_polygon(&ring_outer, LINE);

    let inner_pad = ring_pad + ring_stroke;
    let inner_radius = (ring_radius - ring_stroke).max(1.0);
    let ring_inner = rounded_rect_points(
        inner_pad,
        inner_pad,
        s - inner_pad,
        s - inner_pad,
        inner_radius,
    );
    canvas.fill_polygon(&ring_inner, BG);

    let cell = if maskable {
        (size / 14).max(2)
    } else {
        (size / 10).max(2)
    };
    let glyph_w = cell * 5;
    let glyph_h = cell * 7;
    let x0 = ((size.saturating_sub(glyph_w)) / 2) as i32;
    let y0 = ((size.saturating_sub(glyph_h)) / 2) as i32;

    let rows = glyph_for(initial);
    for (r_idx, &bits) in rows.iter().enumerate() {
        for c_idx in 0..5 {
            if ((bits >> (4 - c_idx)) & 1) == 1 {
                canvas.fill_rect(
                    Rect {
                        x: x0 + c_idx * (cell as i32),
                        y: y0 + (r_idx as i32) * (cell as i32),
                        w: cell,
                        h: cell,
                    },
                    ACCENT,
                );
            }
        }
    }

    encode_png(&canvas)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn renders_valid_png_dimensions() {
        for (ch, size, maskable) in [('T', 192, false), ('I', 512, false), ('Z', 512, true)] {
            let png = render_icon_png(ch, size, maskable);
            assert!(png.len() > 24);
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert_eq!((w, h), (size, size));
        }
    }
}
