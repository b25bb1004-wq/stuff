//! pigment: framebuffer, lines, rectangles, anti-aliased polygons and source-over blending.
//!
//! Everything is integer and total: no input panics, and no allocation grows with coordinates.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod canvas;
mod color;
mod font;
mod line;
mod polygon;
mod text;

pub use canvas::{Canvas, Rect};
pub use color::{over, Rgba};
pub use font::{glyph, GLYPH_H, GLYPH_W};
pub use text::{layout, measure, Line};
