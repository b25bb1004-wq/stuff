#![allow(clippy::unwrap_used)]

use super::*;
use assay::{forall, Rng};

fn rgba(r: &mut Rng) -> Rgba {
    let v = r.u64().to_le_bytes();
    Rgba(v[0], v[1], v[2], v[3])
}

#[test]
fn over_hand_worked_table() {
    let cases: &[(Rgba, Rgba, Rgba)] = &[
        // opaque source wins
        (
            Rgba(10, 20, 30, 255),
            Rgba(200, 100, 50, 255),
            Rgba(200, 100, 50, 255),
        ),
        (
            Rgba(255, 255, 255, 255),
            Rgba(0, 0, 0, 255),
            Rgba(0, 0, 0, 255),
        ),
        (
            Rgba(0, 0, 0, 255),
            Rgba(255, 255, 255, 255),
            Rgba(255, 255, 255, 255),
        ),
        (
            Rgba(255, 0, 0, 255),
            Rgba(0, 0, 255, 255),
            Rgba(0, 0, 255, 255),
        ),
        // transparent source keeps dst
        (
            Rgba(10, 20, 30, 255),
            Rgba(0, 0, 0, 0),
            Rgba(10, 20, 30, 255),
        ),
        (
            Rgba(200, 0, 0, 255),
            Rgba(0, 200, 0, 0),
            Rgba(200, 0, 0, 255),
        ),
        (
            Rgba(0, 0, 0, 255),
            Rgba(255, 255, 255, 0),
            Rgba(0, 0, 0, 255),
        ),
        (Rgba(40, 50, 60, 0), Rgba(0, 0, 0, 0), Rgba(40, 50, 60, 0)),
        (Rgba(0, 0, 0, 0), Rgba(0, 0, 0, 0), Rgba(0, 0, 0, 0)),
        (
            Rgba(255, 255, 255, 0),
            Rgba(0, 0, 0, 0),
            Rgba(255, 255, 255, 0),
        ),
        // half alpha over opaque white and black
        (
            Rgba(255, 255, 255, 255),
            Rgba(0, 0, 0, 128),
            Rgba(127, 127, 127, 255),
        ),
        (
            Rgba(0, 0, 0, 255),
            Rgba(255, 255, 255, 128),
            Rgba(128, 128, 128, 255),
        ),
        // half alpha over transparent: colour is the source colour
        (Rgba(0, 0, 0, 0), Rgba(255, 0, 0, 128), Rgba(255, 0, 0, 128)),
        // half alpha over half alpha: oa = 192, channel 255*128*255 + 192*255/2 over 192*255
        (
            Rgba(0, 0, 255, 128),
            Rgba(255, 0, 0, 128),
            Rgba(170, 0, 85, 192),
        ),
        (
            Rgba(0, 0, 0, 128),
            Rgba(255, 255, 255, 128),
            Rgba(170, 170, 170, 192),
        ),
        // one unit of alpha over opaque: barely changes dst
        (
            Rgba(100, 100, 100, 255),
            Rgba(200, 0, 0, 1),
            Rgba(100, 100, 100, 255),
        ),
        // one unit of alpha over transparent keeps the colour
        (Rgba(0, 0, 0, 0), Rgba(10, 20, 30, 1), Rgba(10, 20, 30, 1)),
        // 64/255 red over opaque green
        (
            Rgba(0, 255, 0, 255),
            Rgba(255, 0, 255, 64),
            Rgba(64, 191, 64, 255),
        ),
        // half alpha over opaque black keeps alpha 255
        (
            Rgba(0, 0, 0, 255),
            Rgba(255, 0, 0, 128),
            Rgba(128, 0, 0, 255),
        ),
        // opaque over half alpha
        (
            Rgba(0, 0, 0, 128),
            Rgba(255, 0, 0, 255),
            Rgba(255, 0, 0, 255),
        ),
        // alpha 255 over transparent
        (
            Rgba(0, 0, 0, 0),
            Rgba(255, 255, 255, 255),
            Rgba(255, 255, 255, 255),
        ),
    ];
    for (i, (d, s, want)) in cases.iter().enumerate() {
        assert_eq!(over(*d, *s), *want, "case {i}");
    }
}

#[test]
fn over_opaque_source_wins() {
    forall(2000, 11, |r| {
        let (d, s) = (rgba(r), rgba(r));
        let s = Rgba(s.0, s.1, s.2, 255);
        assert_eq!(over(d, s), s);
    });
}

#[test]
fn over_transparent_source_keeps_dst() {
    forall(2000, 12, |r| {
        let (d, s) = (rgba(r), rgba(r));
        let s = Rgba(s.0, s.1, s.2, 0);
        assert_eq!(over(d, s), d);
    });
}

#[test]
fn over_alpha_is_monotone_in_source_alpha() {
    forall(2000, 13, |r| {
        let (d, c) = (rgba(r), rgba(r));
        let (a, b) = (r.u64().to_le_bytes()[0], r.u64().to_le_bytes()[0]);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let alpha_lo = over(d, Rgba(c.0, c.1, c.2, lo)).3;
        let alpha_hi = over(d, Rgba(c.0, c.1, c.2, hi)).3;
        assert!(
            alpha_lo <= alpha_hi,
            "sa {lo} -> {alpha_lo}, sa {hi} -> {alpha_hi}"
        );
    });
}
