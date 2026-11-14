//! Anti-aliased polygon fill (non-zero winding), without floating-point libraries.
//!
//! Method: each pixel row is sampled on 16 horizontal sub-scanlines at their centres. On each
//! sub-scanline the filled intervals are found from the edge crossings, and every pixel takes the
//! exact horizontal length of the interval inside it. Coverage is the mean over the 16 lines.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::canvas::Canvas;
use crate::color::Rgba;

#[cfg(test)]
mod tests;

/// Sub-scanlines per pixel row.
const SUB: usize = 16;
/// Crossing positions are clamped to this magnitude so that every span stays finite.
const LIMIT: f32 = 2_147_483_648.0;

impl Canvas {
    /// Fill the closed polygon through `pts` with `c`, anti-aliased.
    ///
    /// Coverage scales the alpha of `c` (`a = round(c.3 * coverage)`), and each pixel is blended
    /// once. Fully covered pixels get exactly `c`'s alpha. Points with a NaN or infinite
    /// coordinate are dropped. Fewer than three remaining points draw nothing.
    pub fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Rgba) {
        let mut pts: Vec<(f32, f32)> = pts
            .iter()
            .copied()
            .filter(|p| p.0.is_finite() && p.1.is_finite())
            .collect();
        if pts.len() < 3 {
            return;
        }
        // One orientation before rasterising, so float sums (and pixels) do not depend on winding.
        let n = pts.len();
        let twice_area: f64 = (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                f64::from(a.0) * f64::from(b.1) - f64::from(b.0) * f64::from(a.1)
            })
            .sum();
        if twice_area < 0.0 {
            pts.reverse();
        }
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        for &(x, y) in &pts {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        let x0 = floor_i64(min_x).max(0);
        let y0 = floor_i64(min_y).max(0);
        let x1 = ceil_i64(max_x).min(i64::from(self.width));
        let y1 = ceil_i64(max_y).min(i64::from(self.height));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        // Edges as (ax, ay, bx, by), closing the ring; horizontal edges never cross a sub-scanline.
        let edges: Vec<(f32, f32, f32, f32)> = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .filter(|(p, q)| p.1 != q.1)
            .map(|(p, q)| (p.0, p.1, q.0, q.1))
            .collect();

        let mut cov = vec![0.0f32; (x1 - x0) as usize];
        let mut xs: Vec<(f32, i32)> = Vec::new();
        let weight = 1.0 / SUB as f32;
        for y in y0..y1 {
            cov.fill(0.0);
            for s in 0..SUB {
                let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
                xs.clear();
                for &(ax, ay, bx, by) in &edges {
                    let (lo, hi) = if ay < by { (ay, by) } else { (by, ay) };
                    if sy >= lo && sy < hi {
                        let x = ax + (sy - ay) * (bx - ax) / (by - ay);
                        // overflow can make x NaN; such a crossing is dropped
                        let x = x.clamp(-LIMIT, LIMIT);
                        if !x.is_nan() {
                            xs.push((x, if by > ay { 1 } else { -1 }));
                        }
                    }
                }
                xs.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
                let mut wind = 0i32;
                let mut start = 0.0f32;
                for &(x, d) in &xs {
                    let was = wind;
                    wind += d;
                    if was == 0 {
                        start = x;
                    } else if wind == 0 {
                        add_span(&mut cov, x0, start, x, weight);
                    }
                }
            }
            for (i, &v) in cov.iter().enumerate() {
                let a = floor_i64(f32::from(c.3) * v.min(1.0) + 0.5).clamp(0, 255) as u8;
                if a > 0 {
                    self.blend_at(x0 + i as i64, y, Rgba(c.0, c.1, c.2, a));
                }
            }
        }
    }
}

/// Add `weight` times the exact length of [xa, xb) inside each pixel of `cov` (pixel `base` first).
fn add_span(cov: &mut [f32], base: i64, xa: f32, xb: f32, weight: f32) {
    let lo = floor_i64(xa).max(base);
    let hi = ceil_i64(xb).min(base + cov.len() as i64);
    for px in lo..hi {
        let left = (px as f32).max(xa);
        let right = ((px + 1) as f32).min(xb);
        if right > left {
            if let Some(v) = cov.get_mut((px - base) as usize) {
                *v += (right - left) * weight;
            }
        }
    }
}

/// Floor of a finite value in the clamped range, as an integer (no `std` needed).
fn floor_i64(v: f32) -> i64 {
    let t = v as i64;
    if (t as f32) > v {
        t.saturating_sub(1)
    } else {
        t
    }
}

/// Ceiling of a finite value in the clamped range, as an integer (no `std` needed).
fn ceil_i64(v: f32) -> i64 {
    let t = v as i64;
    if (t as f32) < v {
        t.saturating_add(1)
    } else {
        t
    }
}
