//! Bresenham lines in exact integer arithmetic.

use crate::canvas::Canvas;
use crate::color::Rgba;

#[cfg(test)]
mod tests;

impl Canvas {
    /// Draw a 1-pixel line from (`x0`, `y0`) to (`x1`, `y1`), both ends included.
    ///
    /// The major axis is the one with the larger span. Each major step gets one minor step, with
    /// exact half-pixel ties rounding up. The endpoint with the smaller major coordinate is the
    /// origin of that rounding, so the pixel set does not depend on which end is given first.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        let (x0, y0) = (i64::from(x0), i64::from(y0));
        let (x1, y1) = (i64::from(x1), i64::from(y1));
        let y_major = (y1 - y0).abs() > (x1 - x0).abs();
        let (a0, b0, a1, b1) = if y_major {
            (y0, x0, y1, x1)
        } else {
            (x0, y0, x1, y1)
        };
        let (a0, b0, a1, b1) = if a0 <= a1 {
            (a0, b0, a1, b1)
        } else {
            (a1, b1, a0, b0)
        };
        let da = a1 - a0;
        if da == 0 {
            self.blend_at(x0, y0, c);
            return;
        }
        let (sb, mb) = ((b1 - b0).signum(), (b1 - b0).abs());
        let dim = i64::from(if y_major { self.height } else { self.width });
        for a in a0.max(0)..=a1.min(dim - 1) {
            // step i along the major axis: minor offset is round-half-up of mb * i / da
            let i = i128::from(a - a0);
            let m = (2 * i128::from(mb) * i + i128::from(da)) / (2 * i128::from(da));
            let b = b0 + sb * (m as i64);
            if y_major {
                self.blend_at(b, a, c);
            } else {
                self.blend_at(a, b, c);
            }
        }
    }
}
