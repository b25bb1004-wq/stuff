#![allow(clippy::unwrap_used)]

use alloc::string::String;

use assay::forall;

use super::{layout, measure, Line};
use crate::{Canvas, Rgba, GLYPH_H};

fn ln(start: usize, end: usize, width: u32) -> Line {
    Line { start, end, width }
}

#[test]
fn layout_table_32_cases() {
    let cases: &[(&str, u32, u32, &[Line])] = &[
        // 1. Empty string
        ("", 100, 1, &[ln(0, 0, 0)]),
        // 2. Empty string with width 0
        ("", 0, 1, &[ln(0, 0, 0)]),
        // 3. Single char fitting
        ("a", 8, 1, &[ln(0, 1, 8)]),
        // 4. Single char wider than max_width
        ("a", 7, 1, &[ln(0, 1, 8)]),
        // 5. Width 0 splits per char
        ("abc", 0, 1, &[ln(0, 1, 8), ln(1, 2, 8), ln(2, 3, 8)]),
        // 6. Exact fit
        ("hello", 40, 1, &[ln(0, 5, 40)]),
        // 7. Wrap at space
        ("hello world", 40, 1, &[ln(0, 5, 40), ln(6, 11, 40)]),
        // 8. Wrap at space before overflowing word
        ("hi there", 40, 1, &[ln(0, 2, 16), ln(3, 8, 40)]),
        // 9. Long word broken between chars
        ("abcdefgh", 40, 1, &[ln(0, 5, 40), ln(5, 8, 24)]),
        // 10. Multiple words wrapping
        ("one two three", 56, 1, &[ln(0, 7, 56), ln(8, 13, 40)]),
        // 11. Multiple spaces at wrap boundary dropped
        ("hello  world", 40, 1, &[ln(0, 5, 40), ln(7, 12, 40)]),
        // 12. Trailing space at wrap boundary dropped without empty line
        ("hello ", 40, 1, &[ln(0, 5, 40)]),
        // 13. Hard newline
        ("a\nb", 100, 1, &[ln(0, 1, 8), ln(2, 3, 8)]),
        // 14. Trailing newline adds empty last line
        ("a\n", 100, 1, &[ln(0, 1, 8), ln(2, 2, 0)]),
        // 15. Leading newline
        ("\na", 100, 1, &[ln(0, 0, 0), ln(1, 2, 8)]),
        // 16. Single newline
        ("\n", 100, 1, &[ln(0, 0, 0), ln(1, 1, 0)]),
        // 17. CRLF counts as one break
        ("a\r\nb", 100, 1, &[ln(0, 1, 8), ln(3, 4, 8)]),
        // 18. Trailing CRLF adds empty last line
        ("a\r\n", 100, 1, &[ln(0, 1, 8), ln(3, 3, 0)]),
        // 19. Double CRLF
        (
            "a\r\n\r\nb",
            100,
            1,
            &[ln(0, 1, 8), ln(3, 3, 0), ln(5, 6, 8)],
        ),
        // 20. Lone CR is a normal char, not a break
        ("a\rb", 100, 1, &[ln(0, 3, 24)]),
        // 21. Tab at col 0 advances to 4 cells
        ("\t", 100, 1, &[ln(0, 1, 32)]),
        // 22. Tab after 1 char advances to 4 cells
        ("a\tb", 100, 1, &[ln(0, 3, 40)]),
        // 23. Tab after 3 chars advances to 4 cells
        ("abc\td", 100, 1, &[ln(0, 5, 40)]),
        // 24. Tab after 4 chars advances to 8 cells
        ("abcd\te", 100, 1, &[ln(0, 6, 72)]),
        // 25. Tab wider than max_width alone on line
        ("\t", 16, 1, &[ln(0, 1, 32)]),
        // 26. Tab overflows after char
        ("a\tbc", 40, 1, &[ln(0, 3, 40), ln(3, 4, 8)]),
        // 27. Scale 0 treated as 1
        ("ab", 8, 0, &[ln(0, 1, 8), ln(1, 2, 8)]),
        // 28. Scale 2 doubles widths
        ("hi there", 80, 2, &[ln(0, 2, 32), ln(3, 8, 80)]),
        // 29. Multi-byte UTF-8 chars count as 1 cell each, not byte len
        ("é€🦀", 24, 1, &[ln(0, 9, 24)]),
        // 30. Multi-byte UTF-8 wrap preserves char boundaries
        ("é€ 🦀!", 16, 1, &[ln(0, 5, 16), ln(6, 11, 16)]),
        // 31. Multi-byte UTF-8 with width 0 splits on char boundaries
        ("é€🦀", 0, 1, &[ln(0, 2, 8), ln(2, 5, 8), ln(5, 9, 8)]),
        // 32. Word then space then long word
        (
            "hi superlong",
            40,
            1,
            &[ln(0, 2, 16), ln(3, 8, 40), ln(8, 12, 32)],
        ),
    ];

    for (idx, &(text, max_w, scale, expected)) in cases.iter().enumerate() {
        let got = layout(text, max_w, scale);
        assert_eq!(got.as_slice(), expected, "case {idx} failed for {text:?}");
    }
}

#[test]
fn measure_basics() {
    assert_eq!(measure("", 1), (0, 16));
    assert_eq!(measure("", 0), (0, 16));
    assert_eq!(measure("abc", 0), (24, 16));
    assert_eq!(measure("abc\ndefg", 2), (64, 64));
    assert_eq!(measure("a\tb\n", 1), (40, 32));
}

#[test]
fn extreme_coords_and_scales_never_panic() {
    let mut c = Canvas::new(32, 32, Rgba(0, 0, 0, 255));
    let fg = Rgba(255, 255, 255, 255);
    c.draw_text(i32::MIN, i32::MIN, "hello", fg, 64, 100);
    c.draw_text(i32::MAX, i32::MAX, "hello", fg, 64, 100);
    c.draw_text(-10, -10, "hello\nworld", fg, 0, 0);
    let _ = measure("hello", u32::MAX);
    let _ = layout("hello world", u32::MAX, 64);
}

#[test]
fn forall_layout_invariants_and_draw_bounds() {
    let alphabet: &[char] = &['a', 'b', ' ', ' ', '\n', '\r', '\t', 'é', '€', '🦀', 'X'];
    let bg = Rgba(10, 20, 30, 255);
    let fg = Rgba(250, 240, 230, 255);

    forall(1000, 0x57A7_A060, |rng| {
        let len = rng.below(40);
        let mut text = String::new();
        for _ in 0..len {
            text.push(*rng.pick(alphabet));
        }
        let scale = (rng.below(4) as u32) + 1;
        let max_width = if rng.chance(1, 5) {
            0
        } else {
            rng.range(0, 200) as u32
        };

        let lines = layout(&text, max_width, scale);
        assert!(!lines.is_empty(), "layout must return at least one line");

        // Ordering, char boundaries, non-overlapping, dropped break chars coverage
        let mut cursor = 0usize;
        for line in &lines {
            assert!(line.start <= line.end);
            assert!(line.end <= text.len());
            assert!(text.is_char_boundary(line.start));
            assert!(text.is_char_boundary(line.end));
            assert!(line.start >= cursor, "lines overlapped or out of order");

            // Gap between cursor and line.start must consist only of dropped break chars
            let gap = &text[cursor..line.start];
            assert!(
                gap.chars().all(|ch| ch == ' ' || ch == '\n' || ch == '\r'),
                "unexpected char in dropped gap {gap:?} for {text:?}"
            );

            // No line contains '\n' or "\r\n"
            let slice = &text[line.start..line.end];
            assert!(!slice.contains('\n'));

            // Width invariant: <= max_width unless single char > max_width
            if line.width > max_width {
                assert_eq!(
                    slice.chars().count(),
                    1,
                    "line {slice:?} exceeded max_width {max_width} with >1 char"
                );
            }
            cursor = line.end;
        }
        // Trailing gap must also only be dropped break chars
        let tail = &text[cursor..];
        assert!(
            tail.chars().all(|ch| ch == ' ' || ch == '\n' || ch == '\r'),
            "unexpected trailing gap {tail:?} for {text:?}"
        );

        // draw_text sets no pixel outside the returned box
        let cw = 128u32;
        let ch = 128u32;
        let mut canvas = Canvas::new(cw, ch, bg);
        let ox = rng.range(0, 16) as i32;
        let oy = rng.range(0, 16) as i32;
        let (bw, bh) = canvas.draw_text(ox, oy, &text, fg, scale, max_width);
        let expected_h = (lines.len() as u32) * GLYPH_H * scale;
        let expected_w = lines.iter().map(|l| l.width).max().unwrap_or(0);
        assert_eq!((bw, bh), (expected_w, expected_h));

        for y in 0..ch as i32 {
            for x in 0..cw as i32 {
                let inside = x >= ox
                    && y >= oy
                    && i64::from(x) < i64::from(ox) + i64::from(bw)
                    && i64::from(y) < i64::from(oy) + i64::from(bh);
                if !inside {
                    assert_eq!(
                        canvas.get(x, y),
                        Some(bg),
                        "pixel ({x}, {y}) modified outside box ({ox}, {oy}, {bw}, {bh})"
                    );
                }
            }
        }
    });
}
