//! Integration tests for `odyssey::paint`.
#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use odyssey::{html, layout, paint, style};
use pigment::{glyph, Canvas, Rgba};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv_canvas(c: &Canvas) -> u64 {
    let mut h = FNV_OFFSET;
    for b in c.width.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    for b in c.height.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    for p in &c.pixels {
        for b in [p.0, p.1, p.2, p.3] {
            h ^= u64::from(b);
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

fn render_page(src: &str, w: u32, h: u32) -> (Canvas, layout::Layout) {
    let doc = html::parse(src);
    let styles = style::compute(&doc);
    let lay = layout::layout(&doc, &styles, w as i32);
    let mut canvas = Canvas::new(w, h, Rgba(255, 255, 255, 255));
    paint::paint(&doc, &styles, &lay, &mut canvas);
    (canvas, lay)
}

fn first_glyph_offset(ch: char, scale: i32) -> (i32, i32) {
    let g = glyph(ch);
    for (row, &bits) in g.iter().enumerate() {
        for col in 0..8 {
            if (bits >> (7 - col)) & 1 != 0 {
                return (col * scale, (row as i32) * scale);
            }
        }
    }
    (0, 0)
}

#[test]
fn page_1_box_borders_and_text() {
    let src = r#"
    <html><body style="margin: 0">
      <div style="width: 100px; height: 40px; background-color: #ffcc00;
                  border-width: 4px 8px 6px 2px; border-top-width: 4px;
                  padding: 5px; color: #112233">Hi</div>
    </body></html>"#;
    let (c, lay) = render_page(src, 160, 80);
    // Border box is at (0, 0), w = 2 + 5 + 100 + 5 + 8 = 120, h = 4 + 5 + 40 + 5 + 6 = 60.
    let border_col = Rgba(0x11, 0x22, 0x33, 255);
    let bg_col = Rgba(0xff, 0xcc, 0x00, 255);
    // Border sides
    assert_eq!(c.get(60, 2), Some(border_col)); // top
    assert_eq!(c.get(115, 30), Some(border_col)); // right
    assert_eq!(c.get(60, 56), Some(border_col)); // bottom
    assert_eq!(c.get(1, 30), Some(border_col)); // left
                                                // Background inside padding/content
    assert_eq!(c.get(50, 30), Some(bg_col));
    // Outside box is untouched canvas white
    assert_eq!(c.get(130, 30), Some(Rgba(255, 255, 255, 255)));
    // First text pixel: content starts at x = 2 + 5 = 7, y = 4 + 5 = 9
    let run = &lay.runs[0];
    assert_eq!((run.rect.x, run.rect.y, run.scale), (7, 9, 1));
    let (dx, dy) = first_glyph_offset('H', 1);
    assert_eq!(c.get(7 + dx, 9 + dy), Some(border_col));
    assert_eq!(fnv_canvas(&c), 0x990d_8959_7cc1_ac72);
}

#[test]
fn page_2_nested_boxes_and_inherited_color() {
    let src = r#"
    <html><body style="margin: 0; background-color: #eeeeee; color: #004488">
      <div style="margin: 10px; padding: 6px; border-width: 3px; background-color: #aabbcc">
        <p style="margin: 0; padding: 4px; border-width: 2px; background-color: #ffffff; color: #cc0000">Red</p>
      </div>
    </body></html>"#;
    let (c, lay) = render_page(src, 200, 100);
    // Outer div: x=10, y=10, border=3 (#004488), bg=#aabbcc
    assert_eq!(c.get(5, 5), Some(Rgba(0xee, 0xee, 0xee, 255))); // body bg
    assert_eq!(c.get(10, 11), Some(Rgba(0x00, 0x44, 0x88, 255))); // div top/left border
    assert_eq!(c.get(15, 15), Some(Rgba(0xaa, 0xbb, 0xcc, 255))); // div padding bg
                                                                  // Inner p: x = 10+3+6 = 19, y = 10+3+6 = 19, border=2 (#cc0000), bg=#ffffff
    assert_eq!(c.get(19, 20), Some(Rgba(0xcc, 0x00, 0x00, 255))); // p top border
    assert_eq!(c.get(22, 22), Some(Rgba(0xff, 0xff, 0xff, 255))); // p bg
    let run = &lay.runs[0];
    let (dx, dy) = first_glyph_offset('R', 1);
    assert_eq!(
        c.get(run.rect.x + dx, run.rect.y + dy),
        Some(Rgba(0xcc, 0x00, 0x00, 255))
    );
    assert_eq!(fnv_canvas(&c), 0x9b3a_83c9_d708_3d94);
}

#[test]
fn page_3_per_side_borders_and_scale2_text() {
    let src = r#"
    <html><body style="margin: 0">
      <h1 style="margin: 0; font-size: 32px; color: #225511; background-color: #ddeeff;
                 border-top-width: 5px; border-right-width: 0px;
                 border-bottom-width: 3px; border-left-width: 7px; padding: 4px">Title</h1>
    </body></html>"#;
    let (c, lay) = render_page(src, 240, 80);
    let fg = Rgba(0x22, 0x55, 0x11, 255);
    let bg = Rgba(0xdd, 0xee, 0xff, 255);
    assert_eq!(c.get(100, 2), Some(fg)); // top border (5px)
    assert_eq!(c.get(239, 10), Some(bg)); // right border is 0px -> bg
    assert_eq!(c.get(100, 46), Some(fg)); // bottom border: h = 5+4+32+4+3 = 48, y=45..48
    assert_eq!(c.get(3, 20), Some(fg)); // left border (7px)
    assert_eq!(c.get(9, 7), Some(bg)); // padding inside
    let run = &lay.runs[0];
    assert_eq!(run.scale, 2);
    let (dx, dy) = first_glyph_offset('T', 2);
    assert_eq!(c.get(run.rect.x + dx, run.rect.y + dy), Some(fg));
    assert_eq!(fnv_canvas(&c), 0xb0cb_e7b0_c43a_9965);
}

#[test]
fn page_4_flex_row_boxes() {
    let src = r#"
    <html><body style="margin: 0">
      <div style="display: flex; gap: 10px; background-color: #101010; padding: 8px">
        <div style="width: 60px; height: 30px; background-color: red; border-width: 2px; color: white">One</div>
        <div style="width: 60px; height: 30px; background-color: blue; border-width: 2px; color: white">Two</div>
      </div>
    </body></html>"#;
    let (c, lay) = render_page(src, 200, 80);
    assert_eq!(c.get(4, 4), Some(Rgba(0x10, 0x10, 0x10, 255))); // flex container padding
    assert_eq!(c.get(8, 8), Some(Rgba(255, 255, 255, 255))); // first box border
    assert_eq!(c.get(12, 35), Some(Rgba(255, 0, 0, 255))); // first box bg
    assert_eq!(c.get(75, 20), Some(Rgba(0x10, 0x10, 0x10, 255))); // 10px gap
                                                                  // Second box starts at x = 8 + (2+60+2) + 10 = 82
    assert_eq!(c.get(82, 8), Some(Rgba(255, 255, 255, 255))); // second box border
    assert_eq!(c.get(86, 35), Some(Rgba(0, 0, 255, 255))); // second box bg
    let run = &lay.runs[0];
    let (dx, dy) = first_glyph_offset('O', 1);
    assert_eq!(
        c.get(run.rect.x + dx, run.rect.y + dy),
        Some(Rgba(255, 255, 255, 255))
    );
    assert_eq!(fnv_canvas(&c), 0x01f3_89d2_d57a_bffb);
}

#[test]
fn page_5_overlap_and_display_none() {
    let src = r#"
    <html><body style="margin: 0">
      <div style="width: 80px; height: 40px; margin-bottom: -20px; background-color: red; color: black">First</div>
      <div style="display: none; width: 80px; height: 40px; background-color: green">Hidden</div>
      <div style="margin-top: -20px; width: 60px; height: 30px; background-color: blue; border-width: 2px; color: white">Top</div>
    </body></html>"#;
    let (c, lay) = render_page(src, 140, 80);
    assert_eq!(c.get(70, 10), Some(Rgba(255, 0, 0, 255))); // red where not overlapped
    assert_eq!(c.get(10, 20), Some(Rgba(255, 255, 255, 255))); // second box top border on top of red
    assert_eq!(c.get(62, 35), Some(Rgba(255, 255, 255, 255))); // second box right border (w=2+60+2=64)
    assert_eq!(c.get(30, 52), Some(Rgba(255, 255, 255, 255))); // second box bottom border (y=20, h=34 -> 52..54)
    assert_eq!(c.get(0, 35), Some(Rgba(255, 255, 255, 255))); // second box left border
    assert_eq!(c.get(30, 30), Some(Rgba(0, 0, 255, 255))); // second box bg on top of red
    assert_eq!(lay.runs.len(), 2); // Hidden text not laid out or painted
    let run = &lay.runs[1];
    let (dx, dy) = first_glyph_offset('T', 1);
    assert_eq!(
        c.get(run.rect.x + dx, run.rect.y + dy),
        Some(Rgba(255, 255, 255, 255))
    );
    assert_eq!(fnv_canvas(&c), 0x8f5d_24be_0c84_6f8f);
}

#[test]
fn page_6_inline_span_color_and_wrap() {
    let src = r#"
    <html><body style="margin: 0">
      <p style="margin: 0; width: 80px; padding: 6px; border-width: 2px; background-color: #fafafa; color: black">
        Hello <span style="color: green">world</span>
      </p>
    </body></html>"#;
    let (c, lay) = render_page(src, 120, 80);
    assert_eq!(c.get(0, 0), Some(Rgba(0, 0, 0, 255))); // p border
    assert_eq!(c.get(3, 3), Some(Rgba(0xfa, 0xfa, 0xfa, 255))); // p bg
    assert_eq!(lay.runs.len(), 2);
    let r0 = &lay.runs[0];
    let (dx0, dy0) = first_glyph_offset('H', 1);
    assert_eq!(
        c.get(r0.rect.x + dx0, r0.rect.y + dy0),
        Some(Rgba(0, 0, 0, 255))
    );
    let r1 = &lay.runs[1];
    let (dx1, dy1) = first_glyph_offset('w', 1);
    assert_eq!(
        c.get(r1.rect.x + dx1, r1.rect.y + dy1),
        Some(Rgba(0, 128, 0, 255))
    );
    assert_eq!(fnv_canvas(&c), 0xa35e_00d8_ad08_9d1d);
}

#[test]
fn paint_is_deterministic_and_total() {
    fn gen_html(rng: &mut Rng) -> String {
        let colors = ["red", "green", "blue", "#123", "#abcdef", "transparent"];
        let mut s = String::from("<html><body style=\"margin:4px;background-color:white\">");
        for i in 0..4 {
            let bg = colors[rng.below(colors.len())];
            let fg = colors[rng.below(colors.len())];
            let bw = rng.below(6);
            s.push_str(&format!(
                "<div style=\"background-color:{bg};color:{fg};border-width:{bw}px;padding:2px\">word{i}</div>"
            ));
        }
        s.push_str("</body></html>");
        s
    }

    forall(40, 0x5eed_cafe_1234_u64, |rng| {
        let html_src = gen_html(rng);
        let doc = html::parse(&html_src);
        let styles = style::compute(&doc);
        let lay = layout::layout(&doc, &styles, 200);
        let mut c1 = Canvas::new(200, 160, Rgba(255, 255, 255, 255));
        let mut c2 = Canvas::new(200, 160, Rgba(255, 255, 255, 255));
        paint::paint(&doc, &styles, &lay, &mut c1);
        paint::paint(&doc, &styles, &lay, &mut c2);
        assert_eq!(c1.pixels, c2.pixels);
    });
}
