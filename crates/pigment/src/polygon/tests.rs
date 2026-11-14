#![allow(clippy::unwrap_used)]

use super::*;
use alloc::vec::Vec;
use assay::{forall, Rng};

const CLEAR: Rgba = Rgba(0, 0, 0, 0);
const WHITE: Rgba = Rgba(255, 255, 255, 255);

fn alpha(c: &Canvas, x: i32, y: i32) -> f32 {
    f32::from(c.get(x, y).unwrap().3)
}

fn area(pts: &[(f32, f32)]) -> f64 {
    let n = pts.len();
    let mut s = 0.0f64;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        s += f64::from(a.0) * f64::from(b.1) - f64::from(b.0) * f64::from(a.1);
    }
    (s / 2.0).abs()
}

#[test]
fn integer_square_equals_fill_rect() {
    let c = Rgba(40, 80, 120, 200);
    let mut poly = Canvas::new(6, 6, CLEAR);
    poly.fill_polygon(&[(1.0, 1.0), (4.0, 1.0), (4.0, 4.0), (1.0, 4.0)], c);
    let mut rect = Canvas::new(6, 6, CLEAR);
    rect.fill_rect(
        crate::Rect {
            x: 1,
            y: 1,
            w: 3,
            h: 3,
        },
        c,
    );
    assert_eq!(poly, rect);
    assert_eq!(poly.get(2, 2), Some(c));
}

#[test]
fn half_offset_square_has_half_and_quarter_coverage() {
    let mut c = Canvas::new(6, 6, CLEAR);
    c.fill_polygon(&[(1.5, 1.5), (4.5, 1.5), (4.5, 4.5), (1.5, 4.5)], WHITE);
    let near = |got: f32, want: f32| assert!((got - want).abs() <= 1.0, "got {got}, want {want}");
    near(alpha(&c, 1, 1), 63.75); // corner: 0.25
    near(alpha(&c, 1, 2), 127.5); // left edge: 0.5
    near(alpha(&c, 2, 1), 127.5); // top edge: 0.5
    near(alpha(&c, 4, 4), 63.75); // far corner: 0.25
    near(alpha(&c, 2, 2), 255.0); // interior
    assert_eq!(alpha(&c, 0, 0), 0.0);
    assert_eq!(alpha(&c, 5, 5), 0.0);
}

#[test]
fn triangle_coverage_sums_to_area() {
    let pts = [(1.2f32, 0.7f32), (9.6, 2.3), (4.1, 8.9)];
    let mut c = Canvas::new(12, 12, CLEAR);
    c.fill_polygon(&pts, WHITE);
    let sum: f64 = c.pixels.iter().map(|p| f64::from(p.3) / 255.0).sum();
    let want = area(&pts);
    assert!((sum - want).abs() / want < 0.01, "sum {sum}, area {want}");
}

#[test]
fn winding_direction_does_not_matter() {
    let square = [(0.3f32, 0.2f32), (5.6, 0.9), (4.8, 5.4), (0.7, 4.1)];
    let mut rev = square;
    rev.reverse();
    let (mut a, mut b) = (Canvas::new(8, 8, CLEAR), Canvas::new(8, 8, CLEAR));
    a.fill_polygon(&square, WHITE);
    b.fill_polygon(&rev, WHITE);
    assert_eq!(a, b);
    assert!(a.pixels.iter().any(|p| p.3 > 0 && p.3 < 255));
}

#[test]
fn interior_pixels_get_exact_colour_alpha() {
    let c = Rgba(9, 9, 9, 77);
    let mut canvas = Canvas::new(12, 12, CLEAR);
    canvas.fill_polygon(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)], c);
    assert_eq!(canvas.get(1, 1), Some(c));
    assert_eq!(canvas.get(11, 11), Some(CLEAR));
}

#[test]
fn degenerate_and_nan_inputs_draw_as_documented() {
    let mut c = Canvas::new(8, 8, CLEAR);
    c.fill_polygon(&[(0.0, 0.0), (7.0, 7.0)], WHITE);
    c.fill_polygon(&[(0.0, 0.0), (f32::NAN, 1.0), (5.0, 0.0)], WHITE);
    assert_eq!(c.pixels.iter().filter(|p| p.3 > 0).count(), 0);
    c.fill_polygon(
        &[(0.0, 0.0), (f32::NAN, 1.0), (5.0, 0.0), (0.0, 5.0)],
        WHITE,
    );
    assert!(c.pixels.iter().any(|p| p.3 > 0));
}

#[test]
fn random_points_never_panic() {
    forall(1000, 41, |r: &mut Rng| {
        let n = r.range(0, 8) as usize;
        let mut pts = Vec::new();
        for _ in 0..n {
            let mut f = || f32::from_bits(r.u64() as u32);
            pts.push((f(), f()));
        }
        let mut c = Canvas::new(16, 16, CLEAR);
        c.fill_polygon(&pts, Rgba(1, 2, 3, 255));
        let mut big = Vec::new();
        for _ in 0..n {
            let mut g = || (r.range(0, 4_000_000) as f32) - 2_000_000.0;
            big.push((g(), g()));
        }
        c.fill_polygon(&big, WHITE);
    });
}
