#![allow(clippy::unwrap_used)]

use super::*;
use alloc::vec;
use assay::{forall, Rng};

const CLEAR: Rgba = Rgba(0, 0, 0, 0);
const WHITE: Rgba = Rgba(255, 255, 255, 255);

fn count(c: &Canvas, col: Rgba) -> usize {
    c.pixels.iter().filter(|p| **p == col).count()
}

fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

#[test]
fn new_fills_and_reports_size() {
    let c = Canvas::new(3, 2, WHITE);
    assert_eq!((c.width, c.height, c.pixels.len()), (3, 2, 6));
    assert_eq!(c.get(2, 1), Some(WHITE));
    assert_eq!(c.get(3, 0), None);
    assert_eq!(c.get(0, -1), None);
}

#[test]
fn new_unallocatable_size_gives_empty_canvas() {
    let c = Canvas::new(u32::MAX, u32::MAX, WHITE);
    assert_eq!((c.width, c.height, c.pixels.len()), (0, 0, 0));
}

#[test]
fn blend_is_source_over_and_ignores_off_canvas() {
    let mut c = Canvas::new(2, 2, WHITE);
    c.blend(0, 0, Rgba(0, 0, 0, 255));
    assert_eq!(c.get(0, 0), Some(Rgba(0, 0, 0, 255)));
    c.blend(-1, 0, Rgba(9, 9, 9, 255));
    c.blend(i32::MIN, i32::MAX, Rgba(9, 9, 9, 255));
    assert_eq!(count(&c, Rgba(9, 9, 9, 255)), 0);
}

#[test]
fn fill_rect_clips_at_each_edge() {
    let mut c = Canvas::new(5, 5, CLEAR);
    c.fill_rect(rect(-2, 1, 4, 2), WHITE); // left edge: x 0..2, y 1..3
    assert_eq!(count(&c, WHITE), 4);
    assert_eq!(c.get(1, 2), Some(WHITE));
    assert_eq!(c.get(2, 2), Some(CLEAR));

    let mut c = Canvas::new(5, 5, CLEAR);
    c.fill_rect(rect(3, 0, 5, 1), WHITE); // right edge
    assert_eq!(count(&c, WHITE), 2);

    let mut c = Canvas::new(5, 5, CLEAR);
    c.fill_rect(rect(0, -3, 2, 4), WHITE); // top edge: only y 0
    assert_eq!(count(&c, WHITE), 2);
    assert_eq!(c.get(1, 0), Some(WHITE));

    let mut c = Canvas::new(5, 5, CLEAR);
    c.fill_rect(rect(4, 3, 1, 9), WHITE); // bottom edge: y 3 and 4
    assert_eq!(count(&c, WHITE), 2);
}

#[test]
fn fill_rect_outside_empty_and_huge() {
    let mut c = Canvas::new(5, 5, CLEAR);
    c.fill_rect(rect(5, 0, 3, 3), WHITE);
    c.fill_rect(rect(-10, -10, 5, 5), WHITE);
    c.fill_rect(rect(-1, -1, 1, 1), WHITE);
    c.fill_rect(rect(0, 0, 0, 5), WHITE);
    c.fill_rect(rect(0, 0, 5, 0), WHITE);
    assert_eq!(count(&c, WHITE), 0);
    c.fill_rect(rect(i32::MIN, i32::MIN, u32::MAX, u32::MAX), WHITE);
    assert_eq!(count(&c, WHITE), 25);
}

#[test]
fn stroke_rect_border_only() {
    let mut c = Canvas::new(5, 5, CLEAR);
    c.stroke_rect(rect(0, 0, 3, 3), WHITE);
    assert_eq!(count(&c, WHITE), 8);
    assert_eq!(c.get(1, 1), Some(CLEAR));
}

#[test]
fn stroke_rect_thin_sides() {
    let cases: &[(u32, u32, usize)] = &[
        (1, 1, 1),
        (2, 1, 2),
        (1, 2, 2),
        (2, 2, 4),
        (1, 3, 3),
        (3, 1, 3),
    ];
    for &(w, h, want) in cases {
        let mut c = Canvas::new(5, 5, CLEAR);
        c.stroke_rect(rect(0, 0, w, h), WHITE);
        assert_eq!(count(&c, WHITE), want, "w={w} h={h}");
    }
    let mut c = Canvas::new(5, 5, CLEAR);
    c.stroke_rect(rect(1, 0, 1, 3), WHITE);
    assert_eq!(c.get(1, 0), Some(WHITE));
    assert_eq!(c.get(1, 2), Some(WHITE));
    assert_eq!(c.get(1, 3), Some(CLEAR));
}

#[test]
fn stroke_rect_clipped_partly_off_canvas() {
    let mut c = Canvas::new(5, 5, CLEAR);
    c.stroke_rect(rect(-1, -1, 3, 3), WHITE);
    assert_eq!(count(&c, WHITE), 3);
    assert_eq!(c.get(0, 1), Some(WHITE));
    assert_eq!(c.get(1, 1), Some(WHITE));
    assert_eq!(c.get(1, 0), Some(WHITE));
}

#[test]
fn stroke_rect_blends_each_pixel_once() {
    let half = Rgba(255, 255, 255, 128);
    let mut c = Canvas::new(3, 3, CLEAR);
    c.stroke_rect(rect(0, 0, 3, 3), half);
    assert_eq!(c.get(0, 0), Some(half));
    assert_eq!(c.get(1, 0), Some(half));
}

#[test]
fn draw_copies_clipped_source() {
    let mut src = Canvas::new(2, 2, CLEAR);
    src.blend(1, 1, Rgba(10, 20, 30, 255));
    let mut dst = Canvas::new(2, 2, CLEAR);
    dst.draw(&src, -1, -1);
    assert_eq!(dst.get(0, 0), Some(Rgba(10, 20, 30, 255)));
    assert_eq!(count(&dst, CLEAR), 3);

    let mut dst = Canvas::new(2, 2, CLEAR);
    dst.draw(&src, 2, 0);
    dst.draw(&src, i32::MAX, i32::MIN);
    assert_eq!(count(&dst, CLEAR), 4);
}

#[test]
fn to_ppm_header_and_bytes() {
    let mut c = Canvas::new(2, 1, Rgba(1, 2, 3, 0));
    c.blend(1, 0, Rgba(9, 8, 7, 255));
    let bytes = c.to_ppm();
    assert_eq!(&bytes[..11], b"P6\n2 1\n255\n");
    assert_eq!(&bytes[11..], &[1, 2, 3, 9, 8, 7]);
}

#[test]
fn draw_and_fill_never_panic_on_random_input() {
    forall(300, 21, |r: &mut Rng| {
        let mut c = Canvas::new(r.range(0, 9) as u32, r.range(0, 9) as u32, CLEAR);
        let coord = |r: &mut Rng| r.range(0, 4_000_000) as i32 - 2_000_000;
        let rr = rect(
            coord(r),
            coord(r),
            r.range(0, 5_000_000) as u32,
            r.range(0, 5_000_000) as u32,
        );
        c.fill_rect(rr, WHITE);
        c.stroke_rect(rr, WHITE);
        let src = Canvas {
            width: 2,
            height: 2,
            pixels: vec![WHITE; 4],
        };
        c.draw(&src, coord(r), coord(r));
        let _ = c.get(coord(r), coord(r));
    });
}
