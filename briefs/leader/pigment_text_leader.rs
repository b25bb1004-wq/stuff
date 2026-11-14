//! Leader check for T60: glyph rules, layout invariants with a fresh seed, draw box bounds, and a
//! rendered sample PPM for eyeballing (target/t60-sample.ppm).
#![allow(clippy::unwrap_used)]
use assay::Rng;
use pigment::{glyph, layout, measure, Canvas, Rgba, GLYPH_H, GLYPH_W};

#[test]
fn leader_glyph_rules() {
    let mut all: Vec<[u8; 16]> = (0x20u8..=0x7e).map(|b| glyph(b as char)).collect();
    let rep = glyph('\u{FFFD}');
    all.push(rep);
    for (i, g) in all.iter().enumerate() {
        assert!(g.iter().all(|row| row & 1 == 0), "glyph {i} uses column 7");
        if i != 0 {
            assert!(g.iter().any(|&r| r != 0), "glyph {i} is empty");
        }
        for (j, h) in all.iter().enumerate().skip(i + 1) {
            assert_ne!(g, h, "glyphs {i} and {j} are identical");
        }
    }
    assert_eq!(glyph(' '), [0; 16]);
    for c in ['é', '€', '\0', '\u{7f}', '\t', '😀'] {
        assert_eq!(glyph(c), rep, "{c:?}");
    }
}

fn chars(s: &str) -> u32 {
    s.chars().count() as u32
}

fn check_layout(text: &str, maxw: u32, scale: u32) {
    let s = scale.max(1);
    let cell = GLYPH_W * s;
    let lines = layout(text, maxw, scale);
    assert!(!lines.is_empty(), "{text:?}");
    let mut pos = 0usize;
    for l in &lines {
        assert!(
            l.start >= pos && l.start <= l.end && l.end <= text.len(),
            "{text:?} {lines:?}"
        );
        assert!(text.is_char_boundary(l.start) && text.is_char_boundary(l.end));
        // Skipped bytes between lines are only break characters.
        assert!(
            text[pos..l.start]
                .chars()
                .all(|c| c == ' ' || c == '\n' || c == '\r'),
            "{text:?}: dropped {:?}",
            &text[pos..l.start]
        );
        let body = &text[l.start..l.end];
        assert!(!body.contains('\n'), "{text:?}: newline inside a line");
        if !body.contains('\t') {
            assert_eq!(l.width, chars(body) * cell, "{text:?}: width of {body:?}");
        }
        if l.width > maxw {
            assert_eq!(
                chars(body),
                1,
                "{text:?} max {maxw}: over-wide line {body:?}"
            );
        }
        pos = l.end;
    }
    assert!(text[pos..]
        .chars()
        .all(|c| c == ' ' || c == '\n' || c == '\r'));
    let breaks = text.matches('\n').count();
    assert!(
        lines.len() > breaks,
        "{text:?}: {} lines for {breaks} newlines",
        lines.len()
    );
}

#[test]
fn leader_layout_invariants() {
    for (t, w) in [
        ("", 100),
        ("a\n", 100),
        ("a\r\nb", 100),
        ("hello world", 40),
        ("supercalifragilistic", 24),
        ("x", 0),
        ("é€ ab", 16),
        ("\n\n", 8),
    ] {
        check_layout(t, w, 1);
    }
    assert_eq!(layout("", 50, 1).len(), 1);
    assert_eq!(layout("a\n", 50, 1).len(), 2);
    assert_eq!(layout("a\r\nb", 50, 1).len(), 2);
    let l = layout("ab cd", 2 * GLYPH_W, 1);
    assert_eq!((l[0].start, l[0].end, l[1].start, l[1].end), (0, 2, 3, 5));
    assert_eq!(measure("abc\nde", 2), (3 * GLYPH_W * 2, 2 * GLYPH_H * 2));
    let mut r = Rng::new(0x60_2026_1115);
    let alphabet: Vec<char> = "ab cdefg  hij\n\r\té€😀xyz".chars().collect();
    for _ in 0..4000 {
        let n = r.below(60);
        let t: String = (0..n).map(|_| *r.pick(&alphabet)).collect();
        check_layout(&t, r.below(300) as u32, 1 + r.below(3) as u32);
    }
}

#[test]
fn leader_draw_box_and_sample() {
    let mut r = Rng::new(0x60);
    for _ in 0..300 {
        let mut c = Canvas::new(120, 90, Rgba(0, 0, 0, 0));
        let t: String = (0..r.below(40))
            .map(|_| *r.pick(&['w', 'M', ' ', '\n', 'g', '@', 'é']))
            .collect();
        let (x, y) = (r.below(60) as i32 - 20, r.below(60) as i32 - 20);
        let (w, h) = c.draw_text(
            x,
            y,
            &t,
            Rgba(255, 255, 255, 255),
            1 + r.below(2) as u32,
            r.below(130) as u32,
        );
        for py in 0..90 {
            for px in 0..120 {
                if c.get(px, py).unwrap().3 != 0 {
                    assert!(
                        px >= x && py >= y && px < x + w as i32 && py < y + h as i32,
                        "pixel {px},{py} outside box at {x},{y} size {w}x{h} for {t:?}"
                    );
                }
            }
        }
    }
    let _ = Canvas::new(10, 10, Rgba(0, 0, 0, 255)).draw_text(
        i32::MIN,
        i32::MAX,
        "huge",
        Rgba(1, 1, 1, 255),
        64,
        0,
    );
    let mut c = Canvas::new(520, 140, Rgba(16, 18, 24, 255));
    let sample = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~é";
    c.draw_text(4, 4, sample, Rgba(230, 230, 220, 255), 1, 512);
    c.draw_text(
        4,
        60,
        "Strata: the quick brown fox",
        Rgba(120, 200, 255, 255),
        2,
        512,
    );
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/t60-sample.ppm");
    std::fs::write(out, c.to_ppm()).unwrap();
}
