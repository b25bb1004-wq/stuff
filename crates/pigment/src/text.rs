//! Text measuring, word-wrapping layout, and canvas rendering.

use alloc::vec::Vec;

use crate::canvas::Canvas;
use crate::color::Rgba;
use crate::font::{glyph, GLYPH_H, GLYPH_W};

#[cfg(test)]
mod tests;

/// A single laid-out line of text as a byte range into the input string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    /// Start byte offset in the source text.
    pub start: usize,
    /// End byte offset (exclusive) in the source text.
    pub end: usize,
    /// Rendered width of the line in pixels.
    pub width: u32,
}

/// Measure the `(width, height)` in pixels of `text` laid out without wrapping at `scale`.
pub fn measure(text: &str, scale: u32) -> (u32, u32) {
    let scale = scale.max(1);
    let lines = layout(text, u32::MAX, scale);
    bounding_box(&lines, scale)
}

/// Lay out `text` into wrapped lines fitting within `max_width` pixels at `scale`.
pub fn layout(text: &str, max_width: u32, scale: u32) -> Vec<Line> {
    let scale = scale.max(1);
    let cell_w = u64::from(GLYPH_W) * u64::from(scale);
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut seg_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            layout_hard_line(text, seg_start, i, max_width, cell_w, &mut lines);
            i += 2;
            seg_start = i;
        } else if bytes[i] == b'\n' {
            layout_hard_line(text, seg_start, i, max_width, cell_w, &mut lines);
            i += 1;
            seg_start = i;
        } else {
            i += 1;
        }
    }
    layout_hard_line(text, seg_start, bytes.len(), max_width, cell_w, &mut lines);
    lines
}

impl Canvas {
    /// Draw `text` wrapped to `max_width` at `(x, y)` with colour `c` and `scale`; returns `(width, height)` drawn.
    pub fn draw_text(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        c: Rgba,
        scale: u32,
        max_width: u32,
    ) -> (u32, u32) {
        let scale = scale.max(1);
        let lines = layout(text, max_width, scale);
        let size = bounding_box(&lines, scale);
        if c.3 == 0 {
            return size;
        }
        let scale_i = i64::from(scale);
        let cell_w = i64::from(GLYPH_W) * scale_i;
        let cell_h = i64::from(GLYPH_H) * scale_i;
        let x0 = i64::from(x);
        let y0 = i64::from(y);
        let cw = i64::from(self.width);
        let ch_limit = i64::from(self.height);

        for (line_idx, line) in lines.iter().enumerate() {
            let py = y0.saturating_add((line_idx as i64).saturating_mul(cell_h));
            if py >= ch_limit {
                break;
            }
            if py.saturating_add(cell_h) <= 0 {
                continue;
            }
            let mut col: i64 = 0;
            for ch in text[line.start..line.end].chars() {
                if ch == '\t' {
                    col = (col / 4).saturating_add(1).saturating_mul(4);
                    continue;
                }
                let px = x0.saturating_add(col.saturating_mul(cell_w));
                if px >= cw {
                    break;
                }
                if px.saturating_add(cell_w) > 0 {
                    let g = glyph(ch);
                    for (row, &bits) in g.iter().enumerate() {
                        if bits == 0 {
                            continue;
                        }
                        let ry = py.saturating_add((row as i64).saturating_mul(scale_i));
                        for bit_col in 0..8i64 {
                            if (bits >> (7 - bit_col)) & 1 != 0 {
                                let rx = px.saturating_add(bit_col.saturating_mul(scale_i));
                                self.fill_box(rx, ry, rx + scale_i, ry + scale_i, c);
                            }
                        }
                    }
                }
                col = col.saturating_add(1);
            }
        }
        size
    }
}

/// Compute `(width, height)` bounding box for laid-out `lines` at `scale` (already >= 1).
fn bounding_box(lines: &[Line], scale: u32) -> (u32, u32) {
    let width = lines.iter().map(|l| l.width).max().unwrap_or(0);
    let cell_h = GLYPH_H.saturating_mul(scale);
    let height = (lines.len() as u32).saturating_mul(cell_h);
    (width, height)
}

/// Advance past any contiguous ASCII space bytes starting at `pos` up to `end`.
fn skip_spaces(text: &str, mut pos: usize, end: usize) -> usize {
    let bytes = text.as_bytes();
    while pos < end && bytes[pos] == b' ' {
        pos += 1;
    }
    pos
}

/// Convert a cell count to a clamped `u32` pixel width.
fn width_px(col: u64, cell_w: u64) -> u32 {
    col.saturating_mul(cell_w).min(u64::from(u32::MAX)) as u32
}

/// Lay out a single hard-break-delimited segment `text[seg_start..seg_end]`.
fn layout_hard_line(
    text: &str,
    seg_start: usize,
    seg_end: usize,
    max_width: u32,
    cell_w: u64,
    out: &mut Vec<Line>,
) {
    if seg_start == seg_end {
        out.push(Line {
            start: seg_start,
            end: seg_end,
            width: 0,
        });
        return;
    }

    let max_w = u64::from(max_width);
    let mut line_start = seg_start;

    while line_start < seg_end {
        let mut col: u64 = 0;
        let mut last_space: Option<(usize, usize, u64)> = None;
        let mut wrapped = false;

        for (rel_i, ch) in text[line_start..seg_end].char_indices() {
            let i = line_start + rel_i;
            let next_i = i + ch.len_utf8();
            let new_col = if ch == '\t' {
                (col / 4).saturating_add(1).saturating_mul(4)
            } else {
                col.saturating_add(1)
            };
            let new_width = new_col.saturating_mul(cell_w);

            if new_width <= max_w {
                if ch == ' ' {
                    last_space = Some((i, next_i, col));
                }
                col = new_col;
            } else if ch == ' ' && i > line_start {
                out.push(Line {
                    start: line_start,
                    end: i,
                    width: width_px(col, cell_w),
                });
                line_start = skip_spaces(text, next_i, seg_end);
                wrapped = true;
                break;
            } else if let Some((sp_start, sp_end, col_before)) =
                last_space.filter(|(s, _, _)| *s > line_start)
            {
                out.push(Line {
                    start: line_start,
                    end: sp_start,
                    width: width_px(col_before, cell_w),
                });
                line_start = skip_spaces(text, sp_end, seg_end);
                wrapped = true;
                break;
            } else if i == line_start {
                out.push(Line {
                    start: i,
                    end: next_i,
                    width: width_px(new_col, cell_w),
                });
                line_start = next_i;
                wrapped = true;
                break;
            } else {
                out.push(Line {
                    start: line_start,
                    end: i,
                    width: width_px(col, cell_w),
                });
                line_start = i;
                wrapped = true;
                break;
            }
        }

        if !wrapped {
            out.push(Line {
                start: line_start,
                end: seg_end,
                width: width_px(col, cell_w),
            });
            break;
        }
    }
}
