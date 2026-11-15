# pigment

pigment is the 2D drawing crate of Monolith. It draws into an RGBA canvas: rectangles, lines,
anti-aliased polygons, source-over blending and text from a built-in 8x16 font. It can save the
canvas as PNG or PPM and includes DEFLATE, zlib, CRC-32 and Adler-32 for the PNG path. Every
operation uses integer maths, so the same input gives the same pixels on any machine. The crate
is `no_std` with `alloc`, uses no unsafe code and has no dependencies.

## Features

- RGBA canvas with clipped rectangle fill and 1-pixel rectangle border (`Canvas::fill_rect`,
  `Canvas::stroke_rect`)
- Bresenham line in exact integer arithmetic (`Canvas::line`)
- Anti-aliased polygon fill with non-zero winding (`Canvas::fill_polygon`); NaN and infinite
  points are dropped
- Source-over blending of one pixel or of a whole canvas (`over`, `Canvas::blend`, `Canvas::draw`)
- Text: a fixed 8x16 glyph font scaled by an integer factor, word wrapping, measuring
  (`glyph`, `measure`, `layout`, `Canvas::draw_text`)
- PNG encode to RGBA8 and decode with a pixel limit (`encode_png`, `decode_png`)
- Raw DEFLATE and zlib compress and decompress; inflate stops at a size limit (`deflate`,
  `inflate`, `zlib_compress`, `zlib_decompress`)
- CRC-32 and Adler-32 checksums (`crc32`, `adler32`)
- Binary PPM output, alpha dropped (`Canvas::to_ppm`)

## Public API

| Item | What it does |
| --- | --- |
| `Canvas::new`, `Canvas::get`, `Canvas::blend` | create a canvas; read or blend one pixel |
| `Canvas::fill_rect`, `Canvas::stroke_rect`, `Canvas::draw` | rectangles and copying another canvas |
| `Canvas::line`, `Canvas::fill_polygon` | lines and anti-aliased polygons |
| `Canvas::draw_text`, `measure`, `layout`, `Line` | text drawing, measuring and wrapping |
| `glyph`, `GLYPH_W`, `GLYPH_H` | font bitmap for one character and the cell size |
| `over`, `Rgba` | source-over colour operator and the colour type |
| `Rect` | rectangle with `x`, `y`, `w`, `h` |
| `encode_png`, `decode_png`, `PngError` | PNG encode and decode |
| `deflate`, `inflate`, `zlib_compress`, `zlib_decompress`, `FlateError` | compression |
| `crc32`, `adler32` | checksums |

## Demo

    cargo run -p pigment --example demo

Real output:

```text
canvas 64x32, 2048 pixels
filled a rectangle, stroked its border, drew a diagonal line
filled a half-transparent green triangle (anti-aliased)
drew "Hi" at scale 1: 16x16 pixels
measure("Hi", 1) = (16, 16)
wrapped line bytes 0..3, width 24
wrapped line bytes 4..7, width 24
wrapped line bytes 8..13, width 40
glyph cell 8x16, 'A' bitmap has 16 rows
over(blue, half red) = Rgba(128, 0, 127, 255)
png: 673 bytes, decodes to the same canvas: true
deflate: 180 bytes -> 7 bytes, round trip ok: true
zlib round trip ok: true
crc32("123456789") = cbf43926
adler32("Wikipedia") = 11e60398
ppm: 6157 bytes (text header plus 3 bytes per pixel)
```

The demo uses only fixed inputs, so its output is the same on every run. It writes no files.

## Tests

    cargo test -p pigment

The gate for this crate is `bash tools/check.sh pigment`, which runs format, clippy, the tests
and the zero-dependency check.
