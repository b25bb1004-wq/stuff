#![allow(clippy::unwrap_used)]

use alloc::string::String;
use alloc::vec::Vec;

use super::{glyph, GLYPH_H, GLYPH_W};
use crate::{Canvas, Rgba};

fn draw_to_ascii(text: &str, scale: u32) -> String {
    let w = GLYPH_W * scale;
    let h = GLYPH_H * scale;
    let bg = Rgba(0, 0, 0, 255);
    let fg = Rgba(255, 255, 255, 255);
    let mut c = Canvas::new(w, h, bg);
    c.draw_text(0, 0, text, fg, scale, u32::MAX);
    let mut out = String::new();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            out.push(if c.get(x, y) == Some(fg) { '#' } else { '.' });
        }
        out.push('\n');
    }
    out
}

#[test]
fn space_is_empty_and_all_printable_non_empty() {
    assert_eq!(glyph(' '), [0u8; 16]);
    for code in 0x21u8..=0x7E {
        let ch = code as char;
        assert_ne!(glyph(ch), [0u8; 16], "glyph for {ch:?} should be non-empty");
    }
}

#[test]
fn all_96_glyphs_pairwise_different_and_col7_empty() {
    let mut all: Vec<[u8; 16]> = (0x20u8..=0x7E).map(|c| glyph(c as char)).collect();
    let repl = glyph('\u{FFFD}');
    all.push(repl);
    assert_eq!(all.len(), 96);

    for (i, g) in all.iter().enumerate() {
        for (row, &b) in g.iter().enumerate() {
            assert_eq!(
                b & 0x01,
                0,
                "glyph {i} row {row} has column 7 set: {b:#04x}"
            );
        }
        for (j, other) in all.iter().enumerate().skip(i + 1) {
            assert_ne!(g, other, "glyph {i} and glyph {j} are identical");
        }
    }
}

#[test]
fn non_ascii_and_control_get_replacement() {
    let repl = glyph('\u{FFFD}');
    assert_eq!(glyph('é'), repl);
    assert_eq!(glyph('€'), repl);
    assert_eq!(glyph('\0'), repl);
    assert_eq!(glyph('\x1F'), repl);
    assert_eq!(glyph('\x7F'), repl);
    assert_eq!(glyph('🦀'), repl);
}

#[test]
fn golden_ascii_art_scale_1() {
    assert_eq!(
        draw_to_ascii("A", 1),
        "\
........\n\
........\n\
........\n\
..##....\n\
.#..#...\n\
#....#..\n\
#....#..\n\
#....#..\n\
######..\n\
#....#..\n\
#....#..\n\
#....#..\n\
#....#..\n\
........\n\
........\n\
........\n"
    );

    assert_eq!(
        draw_to_ascii("g", 1),
        "\
........\n\
........\n\
........\n\
........\n\
........\n\
........\n\
.###.#..\n\
#...##..\n\
#....#..\n\
#....#..\n\
#....#..\n\
#...##..\n\
.###.#..\n\
.....#..\n\
#....#..\n\
.####...\n"
    );

    assert_eq!(
        draw_to_ascii("@", 1),
        "\
........\n\
........\n\
........\n\
..####..\n\
.#....#.\n\
#.....#.\n\
#..##.#.\n\
#.#..##.\n\
#.#..##.\n\
#.#..##.\n\
#..###..\n\
#.......\n\
.#####..\n\
........\n\
........\n\
........\n"
    );

    assert_eq!(
        draw_to_ascii("0", 1),
        "\
........\n\
........\n\
........\n\
..###...\n\
.#...#..\n\
#.....#.\n\
#....##.\n\
#...#.#.\n\
#..#..#.\n\
#.#...#.\n\
##....#.\n\
.#...#..\n\
..###...\n\
........\n\
........\n\
........\n"
    );

    assert_eq!(
        draw_to_ascii("~", 1),
        "\
........\n\
........\n\
........\n\
........\n\
........\n\
........\n\
.##...#.\n\
#..#..#.\n\
#...##..\n\
........\n\
........\n\
........\n\
........\n\
........\n\
........\n\
........\n"
    );
}

#[test]
fn golden_ascii_art_a_scale_2() {
    assert_eq!(
        draw_to_ascii("A", 2),
        "\
................\n\
................\n\
................\n\
................\n\
................\n\
................\n\
....####........\n\
....####........\n\
..##....##......\n\
..##....##......\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
############....\n\
############....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
##........##....\n\
................\n\
................\n\
................\n\
................\n\
................\n\
................\n"
    );
}
