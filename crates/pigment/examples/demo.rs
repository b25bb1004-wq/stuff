//! Demo of pigment: draw shapes and text on a canvas, then round-trip it through PNG and DEFLATE.
//! Only integer maths and fixed inputs are used, so the output is the same on every run.

use pigment::{
    adler32, crc32, decode_png, deflate, encode_png, glyph, inflate, layout, measure, over,
    zlib_compress, zlib_decompress, Canvas, Rect, Rgba, GLYPH_H, GLYPH_W,
};

fn main() -> Result<(), String> {
    let white = Rgba(255, 255, 255, 255);
    let mut canvas = Canvas::new(64, 32, white);
    println!(
        "canvas {}x{}, {} pixels",
        canvas.width,
        canvas.height,
        canvas.pixels.len()
    );

    canvas.fill_rect(
        Rect {
            x: 2,
            y: 2,
            w: 20,
            h: 10,
        },
        Rgba(200, 30, 30, 255),
    );
    canvas.stroke_rect(
        Rect {
            x: 2,
            y: 2,
            w: 20,
            h: 10,
        },
        Rgba(0, 0, 0, 255),
    );
    canvas.line(0, 31, 63, 0, Rgba(0, 0, 255, 255));
    println!("filled a rectangle, stroked its border, drew a diagonal line");

    let triangle = [(40.0, 4.0), (60.0, 28.0), (30.0, 28.0)];
    canvas.fill_polygon(&triangle, Rgba(0, 160, 0, 128));
    println!("filled a half-transparent green triangle (anti-aliased)");

    let (tw, th) = canvas.draw_text(2, 14, "Hi", Rgba(0, 0, 0, 255), 1, 60);
    println!("drew \"Hi\" at scale 1: {}x{} pixels", tw, th);
    println!("measure(\"Hi\", 1) = {:?}", measure("Hi", 1));
    for line in layout("one two three", 48, 1) {
        println!(
            "wrapped line bytes {}..{}, width {}",
            line.start, line.end, line.width
        );
    }
    println!(
        "glyph cell {}x{}, 'A' bitmap has {} rows",
        GLYPH_W,
        GLYPH_H,
        glyph('A').len()
    );
    println!(
        "over(blue, half red) = {:?}",
        over(Rgba(0, 0, 255, 255), Rgba(255, 0, 0, 128))
    );

    // PNG round trip: the decoded canvas equals the original.
    let png = encode_png(&canvas);
    let back = decode_png(&png, 1 << 20).map_err(|e| format!("{e:?}"))?;
    println!(
        "png: {} bytes, decodes to the same canvas: {}",
        png.len(),
        back == canvas
    );

    // DEFLATE and zlib round trips on repetitive text.
    let text = "abc".repeat(60);
    let raw = text.as_bytes();
    let packed = deflate(raw, 9);
    let unpacked = inflate(&packed, 1 << 16).map_err(|e| format!("{e:?}"))?;
    println!(
        "deflate: {} bytes -> {} bytes, round trip ok: {}",
        raw.len(),
        packed.len(),
        unpacked == raw
    );
    let wrapped = zlib_compress(raw, 6);
    let unwrapped = zlib_decompress(&wrapped, 1 << 16).map_err(|e| format!("{e:?}"))?;
    println!("zlib round trip ok: {}", unwrapped == raw);
    println!("crc32(\"123456789\") = {:08x}", crc32(b"123456789"));
    println!("adler32(\"Wikipedia\") = {:08x}", adler32(b"Wikipedia"));

    println!(
        "ppm: {} bytes (text header plus 3 bytes per pixel)",
        canvas.to_ppm().len()
    );
    Ok(())
}
