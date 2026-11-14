#![allow(clippy::unwrap_used)]

use super::*;
use alloc::string::String;
use alloc::vec::Vec;
use assay::{forall, Rng};

const SET: Rgba = Rgba(255, 255, 255, 255);
const CLEAR: Rgba = Rgba(0, 0, 0, 0);
const E: &str = ".........";

/// Render a 9x9 canvas: `#` for the set colour, `.` otherwise.
fn render(c: &Canvas) -> Vec<String> {
    (0..9)
        .map(|y| {
            (0..9)
                .map(|x| if c.get(x, y) == Some(SET) { '#' } else { '.' })
                .collect()
        })
        .collect()
}

// Lines from the centre (4,4) to the eight octant targets and the four axis targets.
// Rounding is anchored at the endpoint with the smaller major coordinate.
const OCT_1_X: [&str; 9] = [E, E, E, E, "....##...", "......###", E, E, E]; // to (8,5)
const OCT_2_Y: [&str; 9] = [
    E,
    E,
    E,
    E,
    "....#....",
    "....#....",
    ".....#...",
    ".....#...",
    ".....#...",
]; // to (5,8)
const OCT_3_X: [&str; 9] = [
    E,
    E,
    E,
    E,
    "....#....",
    "..##.....",
    ".#.......",
    "#........",
    E,
]; // to (0,7)
const OCT_4_Y: [&str; 9] = [
    E,
    E,
    E,
    E,
    "....#....",
    "...#.....",
    "..#......",
    "..#......",
    ".#.......",
]; // to (1,8)
const OCT_5_X: [&str; 9] = [
    E,
    "#........",
    ".#.......",
    "..##.....",
    "....#....",
    E,
    E,
    E,
    E,
]; // to (0,1)
const OCT_6_Y: [&str; 9] = [
    "...#.....",
    "...#.....",
    "....#....",
    "....#....",
    "....#....",
    E,
    E,
    E,
    E,
]; // to (3,0)
const OCT_7_X: [&str; 9] = [
    E,
    "........#",
    "......##.",
    ".....#...",
    "....#....",
    E,
    E,
    E,
    E,
]; // to (8,1)
const OCT_8_Y: [&str; 9] = [
    ".......#.",
    "......#..",
    ".....#...",
    ".....#...",
    "....#....",
    E,
    E,
    E,
    E,
]; // to (7,0)
const AXIS_RIGHT: [&str; 9] = [E, E, E, E, "....#####", E, E, E, E]; // to (8,4)
const AXIS_LEFT: [&str; 9] = [E, E, E, E, "#####....", E, E, E, E]; // to (0,4)
const AXIS_DOWN: [&str; 9] = [
    E,
    E,
    E,
    E,
    "....#....",
    "....#....",
    "....#....",
    "....#....",
    "....#....",
]; // to (4,8)
const AXIS_UP: [&str; 9] = [
    "....#....",
    "....#....",
    "....#....",
    "....#....",
    "....#....",
    E,
    E,
    E,
    E,
]; // to (4,0)

fn golden(to: (i32, i32), want: &[&str; 9]) {
    let mut c = Canvas::new(9, 9, CLEAR);
    c.line(4, 4, to.0, to.1, SET);
    assert_eq!(render(&c), *want, "line (4,4) -> {to:?}");
    // the same set comes back when the ends are swapped
    let mut r = Canvas::new(9, 9, CLEAR);
    r.line(to.0, to.1, 4, 4, SET);
    assert_eq!(r, c, "reverse of (4,4) -> {to:?}");
}

#[test]
fn octant_goldens() {
    golden((8, 5), &OCT_1_X);
    golden((5, 8), &OCT_2_Y);
    golden((0, 7), &OCT_3_X);
    golden((1, 8), &OCT_4_Y);
    golden((0, 1), &OCT_5_X);
    golden((3, 0), &OCT_6_Y);
    golden((8, 1), &OCT_7_X);
    golden((7, 0), &OCT_8_Y);
}

#[test]
fn axis_goldens() {
    golden((8, 4), &AXIS_RIGHT);
    golden((0, 4), &AXIS_LEFT);
    golden((4, 8), &AXIS_DOWN);
    golden((4, 0), &AXIS_UP);
}

#[test]
fn single_point_and_endpoints_drawn() {
    let mut c = Canvas::new(9, 9, CLEAR);
    c.line(2, 2, 2, 2, SET);
    assert_eq!(c.pixels.iter().filter(|p| **p == SET).count(), 1);
    assert_eq!(c.get(2, 2), Some(SET));
}

#[test]
fn line_is_symmetric_under_swapping_ends() {
    forall(500, 31, |r: &mut Rng| {
        let mut p = || r.range(0, 24) as i32 - 6;
        let (x0, y0, x1, y1) = (p(), p(), p(), p());
        let mut a = Canvas::new(12, 12, CLEAR);
        let mut b = Canvas::new(12, 12, CLEAR);
        a.line(x0, y0, x1, y1, SET);
        b.line(x1, y1, x0, y0, SET);
        assert_eq!(a, b, "({x0},{y0}) ({x1},{y1})");
    });
}

#[test]
fn each_pixel_blended_once() {
    let half = Rgba(255, 255, 255, 128);
    forall(200, 32, |r: &mut Rng| {
        let mut p = || r.range(0, 12) as i32;
        let mut c = Canvas::new(12, 12, CLEAR);
        c.line(p(), p(), p(), p(), half);
        for px in &c.pixels {
            assert!(px.3 == 0 || px.3 == 128, "pixel alpha {}", px.3);
        }
    });
}

#[test]
fn huge_and_far_lines_do_not_panic_and_clip() {
    let mut c = Canvas::new(9, 9, CLEAR);
    c.line(i32::MIN, i32::MIN, i32::MAX, i32::MAX, SET);
    for d in 0..9 {
        assert_eq!(c.get(d, d), Some(SET), "diagonal {d}");
    }
    let mut c = Canvas::new(9, 9, CLEAR);
    c.line(-1_000_000, 3, 1_000_000, 3, SET);
    assert_eq!(c.pixels.iter().filter(|p| **p == SET).count(), 9);
    forall(300, 33, |r: &mut Rng| {
        let mut p = || r.u64().to_le_bytes();
        let [a, b, c_, d] = [p(), p(), p(), p()];
        let mut c = Canvas::new(5, 5, CLEAR);
        let i = |v: [u8; 8]| i32::from_le_bytes([v[0], v[1], v[2], v[3]]);
        c.line(i(a), i(b), i(c_), i(d), SET);
    });
}
