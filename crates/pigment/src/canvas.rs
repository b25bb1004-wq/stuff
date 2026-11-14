//! Row-major RGBA canvas with clipped rectangles, copies and PPM output.

use alloc::format;
use alloc::vec::Vec;

use crate::color::{over, Rgba};

#[cfg(test)]
mod tests;

/// Row-major pixel buffer. `pixels.len()` equals `width * height` for canvases this crate builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels, row major, straight alpha.
    pub pixels: Vec<Rgba>,
}

/// Axis-aligned rectangle with its top-left corner at (`x`, `y`), `w` wide and `h` high.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width; zero draws nothing.
    pub w: u32,
    /// Height; zero draws nothing.
    pub h: u32,
}

impl Rect {
    /// Edges as (left, top, right exclusive, bottom exclusive) in 64-bit arithmetic.
    pub(crate) fn bounds(self) -> (i64, i64, i64, i64) {
        let x0 = i64::from(self.x);
        let y0 = i64::from(self.y);
        (x0, y0, x0 + i64::from(self.w), y0 + i64::from(self.h))
    }
}

impl Canvas {
    /// New canvas filled with `fill`. If the pixel buffer cannot be allocated, returns a 0x0 canvas.
    pub fn new(width: u32, height: u32, fill: Rgba) -> Canvas {
        let mut pixels = Vec::new();
        if let Some(n) = (width as usize).checked_mul(height as usize) {
            if pixels.try_reserve_exact(n).is_ok() {
                pixels.resize(n, fill);
                return Canvas {
                    width,
                    height,
                    pixels,
                };
            }
        }
        Canvas {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        }
    }

    /// Pixel at (`x`, `y`), or `None` when off the canvas.
    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        let i = self.index(i64::from(x), i64::from(y))?;
        self.pixels.get(i).copied()
    }

    /// Source-over `c` onto the pixel at (`x`, `y`); off-canvas pixels are ignored.
    pub fn blend(&mut self, x: i32, y: i32, c: Rgba) {
        self.blend_at(i64::from(x), i64::from(y), c);
    }

    /// Fill `r` with `c`, source-over, clipped to the canvas.
    pub fn fill_rect(&mut self, r: Rect, c: Rgba) {
        let (x0, y0, x1, y1) = r.bounds();
        self.fill_box(x0, y0, x1, y1, c);
    }

    /// Draw a 1-pixel border just inside `r`. Each pixel is blended once, even when w or h is 1 or 2.
    pub fn stroke_rect(&mut self, r: Rect, c: Rgba) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        let (x0, y0, x1, y1) = r.bounds();
        self.fill_box(x0, y0, x1, y0 + 1, c);
        if r.h > 1 {
            self.fill_box(x0, y1 - 1, x1, y1, c);
        }
        if r.h > 2 {
            self.fill_box(x0, y0 + 1, x0 + 1, y1 - 1, c);
            if r.w > 1 {
                self.fill_box(x1 - 1, y0 + 1, x1, y1 - 1, c);
            }
        }
    }

    /// Source-over copy of `src` with its top-left corner at (`x`, `y`), clipped to both canvases.
    pub fn draw(&mut self, src: &Canvas, x: i32, y: i32) {
        let (x, y) = (i64::from(x), i64::from(y));
        let sx0 = (-x).max(0);
        let sy0 = (-y).max(0);
        let sx1 = (i64::from(self.width) - x).min(i64::from(src.width));
        let sy1 = (i64::from(self.height) - y).min(i64::from(src.height));
        for sy in sy0..sy1 {
            for sx in sx0..sx1 {
                let Some(p) = src.index(sx, sy).and_then(|i| src.pixels.get(i)).copied() else {
                    continue;
                };
                self.blend_at(x + sx, y + sy, p);
            }
        }
    }

    /// Binary PPM (P6) bytes of the colour channels; alpha is dropped.
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        out.reserve(self.pixels.len() * 3);
        for p in &self.pixels {
            out.extend_from_slice(&[p.0, p.1, p.2]);
        }
        out
    }

    /// Flat index of (`x`, `y`) if it lies on the canvas.
    fn index(&self, x: i64, y: i64) -> Option<usize> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        (y as usize)
            .checked_mul(self.width as usize)?
            .checked_add(x as usize)
    }

    /// Source-over one pixel given 64-bit coordinates; off-canvas is ignored.
    pub(crate) fn blend_at(&mut self, x: i64, y: i64, c: Rgba) {
        if let Some(p) = self.index(x, y).and_then(|i| self.pixels.get_mut(i)) {
            *p = over(*p, c);
        }
    }

    /// Source-over every pixel of the box [x0, x1) x [y0, y1), clipped to the canvas.
    pub(crate) fn fill_box(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: Rgba) {
        let x1 = x1.min(i64::from(self.width));
        let y1 = y1.min(i64::from(self.height));
        for y in y0.max(0)..y1 {
            for x in x0.max(0)..x1 {
                self.blend_at(x, y, c);
            }
        }
    }
}
