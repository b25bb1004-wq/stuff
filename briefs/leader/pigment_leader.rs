//! Leader check for T58: `over` against the brief's formula, Bresenham as "nearest pixel per
//! major-axis step", single blending, polygon coverage sums and winding, clip fuzz.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use pigment::{over, Canvas, Rect, Rgba};
use std::time::Duration;

fn reference(d: Rgba, s: Rgba) -> Rgba {
    let (sa, da) = (u32::from(s.3), u32::from(d.3));
    let oa = sa + (da * (255 - sa) + 127) / 255;
    let ch = |sc: u8, dc: u8| {
        if oa == 0 {
            0
        } else {
            ((u32::from(sc) * sa * 255 + u32::from(dc) * da * (255 - sa) + oa * 255 / 2)
                / (oa * 255)) as u8
        }
    };
    Rgba(ch(s.0, d.0), ch(s.1, d.1), ch(s.2, d.2), oa as u8)
}

fn rgba(r: &mut Rng) -> Rgba {
    let v = r.u64().to_le_bytes();
    Rgba(v[0], v[1], v[2], v[3])
}

#[test]
fn leader_over_is_the_brief_formula() {
    let mut r = Rng::new(0x58);
    for _ in 0..300_000 {
        let (d, s) = (rgba(&mut r), rgba(&mut r));
        assert_eq!(over(d, s), reference(d, s), "over({d:?}, {s:?})");
    }
    for sa in 0..=255u8 {
        for da in 0..=255u8 {
            let (d, s) = (Rgba(10, 200, 77, da), Rgba(250, 3, 128, sa));
            assert_eq!(over(d, s), reference(d, s));
        }
    }
}

fn set(c: &Canvas) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for y in 0..c.height as i32 {
        for x in 0..c.width as i32 {
            if c.get(x, y).unwrap().3 != 0 {
                v.push((x, y));
            }
        }
    }
    v
}

#[test]
fn leader_lines_are_nearest_pixel_and_blended_once() {
    let mut r = Rng::new(0x58_11);
    let ink = Rgba(0, 0, 0, 255);
    for _ in 0..3000 {
        let p: Vec<i32> = (0..4).map(|_| r.below(41) as i32).collect();
        let (x0, y0, x1, y1) = (p[0], p[1], p[2], p[3]);
        let mut c = Canvas::new(41, 41, Rgba(0, 0, 0, 0));
        c.line(x0, y0, x1, y1, ink);
        let mut back = Canvas::new(41, 41, Rgba(0, 0, 0, 0));
        back.line(x1, y1, x0, y0, ink);
        let px = set(&c);
        assert_eq!(px, set(&back), "ends swapped: {p:?}");
        let (dx, dy) = (x1 - x0, y1 - y0);
        let steep = dy.abs() > dx.abs();
        let n = dx.abs().max(dy.abs());
        assert_eq!(px.len() as i32, n + 1, "one pixel per major step: {p:?}");
        let mut majors: Vec<i32> = px.iter().map(|&(x, y)| if steep { y } else { x }).collect();
        majors.dedup();
        majors.sort_unstable();
        majors.dedup();
        assert_eq!(
            majors.len() as i32,
            n + 1,
            "two pixels on one major step: {p:?}"
        );
        for &(x, y) in &px {
            // Ideal minor coordinate at this major coordinate, as an exact fraction num/n.
            let (maj, min, m0, n0, dmaj, dmin) = if steep {
                (y, x, y0, x0, dy, dx)
            } else {
                (x, y, x0, y0, dx, dy)
            };
            if n == 0 {
                assert_eq!((x, y), (x0, y0));
                continue;
            }
            let t = (maj - m0) * dmaj.signum(); // 0..=n
            let num2 = 2 * (n0 * n + t * dmin); // twice the ideal minor coordinate, times n
            let ideal2n = num2; // reject pixels farther than half a pixel from the ideal line
            assert!(
                (min * 2 * n - ideal2n).abs() <= n,
                "{p:?}: pixel {x},{y} too far"
            );
        }
    }
    // Single blending: half-alpha ink on white gives exactly one `over` everywhere it lands.
    let mut c = Canvas::new(20, 20, Rgba(255, 255, 255, 255));
    let half = Rgba(0, 0, 0, 128);
    c.line(1, 1, 18, 7, half);
    c.line(3, 19, 3, 0, half);
    let once = over(Rgba(255, 255, 255, 255), half);
    let twice = over(once, half);
    for y in 0..20 {
        for x in 0..20 {
            let p = c.get(x, y).unwrap();
            assert!(
                p == Rgba(255, 255, 255, 255) || p == once || (x == 3 && p == twice),
                "{x},{y}: {p:?}"
            );
        }
    }
}

fn coverage_sum(c: &Canvas) -> f64 {
    c.pixels.iter().map(|p| f64::from(p.3) / 255.0).sum()
}

#[test]
fn leader_polygon_area_winding_and_interior() {
    let mut r = Rng::new(0x58_22);
    let white = Rgba(255, 255, 255, 255);
    for case in 0..300 {
        let k = 3 + r.below(8);
        let (cx, cy) = (40.0 + r.below(20) as f32, 40.0 + r.below(20) as f32);
        let rad = 5.0 + r.below(25) as f32 + (r.below(100) as f32) / 100.0;
        let mut ang: Vec<f32> = (0..k)
            .map(|_| (r.below(100_000) as f32) / 100_000.0 * 6.283_185)
            .collect();
        ang.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pts: Vec<(f32, f32)> = ang
            .iter()
            .map(|a| (cx + rad * a.cos(), cy + rad * a.sin()))
            .collect();
        let area = 0.5
            * (0..k)
                .map(|i| {
                    let (a, b) = (pts[i], pts[(i + 1) % k]);
                    f64::from(a.0) * f64::from(b.1) - f64::from(b.0) * f64::from(a.1)
                })
                .sum::<f64>()
                .abs();
        let mut c = Canvas::new(100, 100, Rgba(0, 0, 0, 0));
        c.fill_polygon(&pts, white);
        let rev: Vec<(f32, f32)> = pts.iter().rev().copied().collect();
        let mut d = Canvas::new(100, 100, Rgba(0, 0, 0, 0));
        d.fill_polygon(&rev, white);
        assert_eq!(c, d, "case {case}: winding changes pixels");
        let sum = coverage_sum(&c);
        assert!(
            (sum - area).abs() <= 0.01 * area + 1.0,
            "case {case}: coverage {sum} vs area {area}"
        );
        // Far outside the bounding box: untouched.
        let (minx, maxx) = pts
            .iter()
            .fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
        for y in 0..100 {
            for x in 0..100 {
                let a = c.get(x, y).unwrap().3;
                if (x as f32) < minx - 2.0 || (x as f32) > maxx + 2.0 {
                    assert_eq!(a, 0, "case {case}: {x},{y} outside");
                }
            }
        }
        // Centre pixel of a big shape is fully covered.
        if rad > 12.0 && k >= 5 {
            assert_eq!(
                c.get(cx as i32, cy as i32).unwrap(),
                white,
                "case {case}: centre"
            );
        }
    }
    let mut a = Canvas::new(16, 16, Rgba(9, 9, 9, 255));
    let mut b = a.clone();
    a.fill_polygon(
        &[(2.0, 3.0), (11.0, 3.0), (11.0, 9.0), (2.0, 9.0)],
        Rgba(200, 10, 10, 180),
    );
    b.fill_rect(
        Rect {
            x: 2,
            y: 3,
            w: 9,
            h: 6,
        },
        Rgba(200, 10, 10, 180),
    );
    assert_eq!(a, b, "integer square equals fill_rect");
}

#[test]
fn leader_clip_fuzz_is_total_and_fast() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        eprintln!("WATCHDOG: clip fuzz still running after 60 s (huge coordinates stepped pixel by pixel?)");
        std::process::abort();
    });
    let mut r = Rng::new(0x58_33);
    let big = [
        i32::MIN,
        i32::MAX,
        -1_000_000_000,
        1_000_000_000,
        -3,
        0,
        7,
        31,
        32,
        33,
    ];
    let mut c = Canvas::new(32, 32, Rgba(0, 0, 0, 0));
    for _ in 0..3000 {
        let v: Vec<i32> = (0..4).map(|_| *r.pick(&big)).collect();
        c.line(v[0], v[1], v[2], v[3], Rgba(1, 2, 3, 200));
        c.fill_rect(
            Rect {
                x: v[0],
                y: v[1],
                w: v[2] as u32,
                h: v[3] as u32,
            },
            Rgba(4, 5, 6, 90),
        );
        c.stroke_rect(
            Rect {
                x: v[1],
                y: v[0],
                w: v[3] as u32,
                h: v[2] as u32,
            },
            Rgba(4, 5, 6, 90),
        );
        let f = |i: usize| v[i] as f32;
        c.fill_polygon(
            &[(f(0), f(1)), (f(2), f(3)), (f(1), f(2))],
            Rgba(7, 8, 9, 255),
        );
        c.fill_polygon(
            &[
                (f32::NAN, 1.0),
                (f(0), f(3)),
                (f(2), f32::INFINITY),
                (3.0, 4.0),
            ],
            Rgba(7, 8, 9, 255),
        );
        let src = Canvas::new(5, 5, Rgba(1, 1, 1, 128));
        c.draw(&src, v[0], v[3]);
    }
    let _ = c.to_ppm();
    assert_eq!(c.get(-1, 0), None);
    assert_eq!(c.get(32, 0), None);
}
