//! PNG app icon generator for the Athena PWA bundle.

use pigment::{encode_png, Canvas, Rect, Rgba};

const BG: Rgba = Rgba(0x0e, 0x10, 0x13, 255);
const ACCENT: Rgba = Rgba(0xe0, 0xa4, 0x58, 255);

/// Render a square `size`x`size` PNG icon with `--bg` background and an `--accent` capital-A mark.
pub(crate) fn render_icon_png(size: u32, maskable: bool) -> Vec<u8> {
    let mut canvas = Canvas::new(size, size, BG);
    let s = size as f32;
    let pad_y = if maskable { s * 0.20 } else { s * 0.14 };
    let pad_x = if maskable { s * 0.24 } else { s * 0.17 };

    let left = pad_x;
    let right = s - pad_x;
    let top = pad_y;
    let bottom = s - pad_y;
    let w = right - left;
    let h = bottom - top;
    let cx = s * 0.5;
    let stroke = (w * 0.19).max(3.0);

    // Outer A triangle in accent.
    canvas.fill_polygon(&[(cx, top), (right, bottom), (left, bottom)], ACCENT);

    // Inner cutout triangle in background colour.
    let inner_top = top + stroke * 1.45;
    let inner_left = left + stroke * 1.15;
    let inner_right = right - stroke * 1.15;
    if inner_right > inner_left && bottom > inner_top {
        canvas.fill_polygon(
            &[
                (cx, inner_top),
                (inner_right, bottom + 1.0),
                (inner_left, bottom + 1.0),
            ],
            BG,
        );
    }

    // Horizontal crossbar rectangle.
    let bar_y = (top + h * 0.61) as i32;
    let bar_h = (stroke * 0.78).round().max(2.0) as u32;
    let bar_x = (left + w * 0.20) as i32;
    let bar_w = (w * 0.60).round().max(2.0) as u32;
    canvas.fill_rect(
        Rect {
            x: bar_x,
            y: bar_y,
            w: bar_w,
            h: bar_h,
        },
        ACCENT,
    );

    // Subtle horizontal base feet rectangles for crisp alignment at 192 px.
    let foot_h = (stroke * 0.30).round().max(1.0) as u32;
    let foot_y = (bottom as i32).saturating_sub(foot_h as i32);
    let foot_w = (stroke * 1.15).round().max(2.0) as u32;
    canvas.fill_rect(
        Rect {
            x: left.round() as i32,
            y: foot_y,
            w: foot_w,
            h: foot_h,
        },
        ACCENT,
    );
    canvas.fill_rect(
        Rect {
            x: (right - foot_w as f32).round() as i32,
            y: foot_y,
            w: foot_w,
            h: foot_h,
        },
        ACCENT,
    );

    encode_png(&canvas)
}
