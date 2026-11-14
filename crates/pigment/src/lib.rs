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
mod flate;
mod font;
mod line;
mod png;
mod polygon;
mod text;

pub use canvas::{Canvas, Rect};
pub use color::{over, Rgba};
pub use flate::{adler32, crc32, deflate, inflate, zlib_compress, zlib_decompress, FlateError};
pub use font::{glyph, GLYPH_H, GLYPH_W};
pub use png::{decode_png, encode_png, PngError};
pub use text::{layout, measure, Line};
