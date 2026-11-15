//! Odyssey HTML + CSS layout and software paint demo.

use pigment::{Canvas, Rgba};

use super::{canvas_to_output, check_input, Demo, Output};

const VIEWPORT_W: i32 = 480;
const MIN_H: i32 = 160;
const MAX_H: i32 = 400;

const DEFAULT_HTML: &str = "\
<style>
  body { background: #f4f6f9; color: #1e293b; padding: 14px; font-size: 16px; }
  h1 { color: #0f172a; font-size: 24px; margin: 0 0 8px 0; }
  p { margin: 0 0 10px 0; }
  .row { display: flex; }
  .card { background: #ffffff; border: 2px solid #2563eb; color: #1d4ed8; padding: 8px; margin: 0 8px 0 0; }
</style>
<h1>Odyssey Browser</h1>
<p>HTML5 tokenizer, CSS cascade, block + flex layout, software paint.</p>
<div class=\"row\">
  <div class=\"card\">480px viewport</div>
  <div class=\"card\">Zero crates</div>
</div>";

/// Registered Odyssey HTML/CSS layout and paint demo.
pub const DEMO: Demo = Demo {
    id: "odyssey",
    title: "Odyssey Browser",
    default_input: DEFAULT_HTML,
    run,
};

/// Parse HTML + inline `<style>`, lay out at 480 px wide, and paint RGBA pixels.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    if input.trim().is_empty() {
        return Err(String::from("empty HTML document"));
    }
    if input.contains('\0') {
        return Err(String::from("HTML input contains NUL byte"));
    }

    let doc = odyssey::html::parse(input);
    let styles = odyssey::style::compute(&doc);
    let laid = odyssey::layout::layout(&doc, &styles, VIEWPORT_W);
    let h = u32::try_from(laid.height.clamp(MIN_H, MAX_H)).unwrap_or(MIN_H as u32);
    let mut canvas = Canvas::new(VIEWPORT_W as u32, h, Rgba(255, 255, 255, 255));
    odyssey::paint::paint(&doc, &styles, &laid, &mut canvas);
    Ok(canvas_to_output(canvas))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn renders_distinct_colors_at_480px() {
        let Output::Pixels { w, h, rgba } = run(DEFAULT_HTML).unwrap() else {
            panic!("expected pixels");
        };
        assert_eq!(w, 480);
        assert!((160..=400).contains(&h));
        let mut distinct = std::collections::BTreeSet::new();
        for &px in rgba.as_chunks::<4>().0 {
            distinct.insert(px);
            if distinct.len() >= 4 {
                break;
            }
        }
        assert!(distinct.len() >= 4, "expected at least 4 distinct colors");
    }
}
