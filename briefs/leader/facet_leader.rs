//! Leader check for T63 (iris): random sessions on a fresh seed against a naive full compositor written
//! from the brief (pixel identity), the damage rule, routing, animation end points and tiling geometry.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;

use assay::Rng;
use iris::{Compositor, Layout, Routed, WinId};
use indium::{Event, Key};
use pigment::{Canvas, Rect, Rgba};

const SEED: u64 = 0x0FAC_E763;
const W: u32 = 200;
const H: u32 = 150;
const DESKTOP: Rgba = Rgba(28, 32, 40, 255);
const BORDER: Rgba = Rgba(68, 76, 92, 255);
const TITLE_ON: Rgba = Rgba(64, 118, 210, 255);
const TITLE_OFF: Rgba = Rgba(46, 52, 64, 255);
const CONTENT: Rgba = Rgba(24, 26, 32, 255);

/// Naive compositor: paint everything, bottom to top, from the public state.
fn reference(c: &Compositor, shadow: &BTreeMap<WinId, Canvas>) -> Canvas {
    let mut s = Canvas::new(W, H, DESKTOP);
    for id in c.order() {
        let f = c.rect(id).unwrap();
        s.fill_rect(f, BORDER);
        let title = if c.focused() == Some(id) {
            TITLE_ON
        } else {
            TITLE_OFF
        };
        s.fill_rect(
            Rect {
                x: f.x + 1,
                y: f.y + 1,
                w: f.w - 2,
                h: 18,
            },
            title,
        );
        let (cx, cy, cw, ch) = (f.x + 1, f.y + 19, f.w - 2, f.h - 20);
        s.fill_rect(
            Rect {
                x: cx,
                y: cy,
                w: cw,
                h: ch,
            },
            CONTENT,
        );
        let surf = &shadow[&id];
        for y in 0..surf.height.min(ch) as i32 {
            for x in 0..surf.width.min(cw) as i32 {
                s.blend(cx + x, cy + y, surf.get(x, y).unwrap());
            }
        }
    }
    s
}

fn check_damage(prev: &Canvas, cur: &Canvas, dmg: &[Rect], ctx: &str) {
    let mut changed = 0u64;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if prev.get(x, y) != cur.get(x, y) {
                changed += 1;
                let inside = dmg
                    .iter()
                    .any(|r| x >= r.x && y >= r.y && x < r.x + r.w as i32 && y < r.y + r.h as i32);
                assert!(
                    inside,
                    "{ctx}: changed pixel ({x},{y}) outside damage {dmg:?}"
                );
            }
        }
    }
    let mut area = 0u64;
    for r in dmg {
        assert!(r.w > 0 && r.h > 0, "{ctx}: zero-size rect {r:?}");
        assert!(
            r.x >= 0 && r.y >= 0 && r.x as u32 + r.w <= W && r.y as u32 + r.h <= H,
            "{ctx}: unclipped {r:?}"
        );
        area += u64::from(r.w) * u64::from(r.h);
    }
    assert!(
        area <= 2 * changed + 4096 * dmg.len() as u64,
        "{ctx}: damage area {area} for {changed} changed px ({} rects)",
        dmg.len()
    );
    if changed == 0 {
        assert!(dmg.is_empty(), "{ctx}: nothing changed but damage {dmg:?}");
    }
}

fn top_at(c: &Compositor, px: i32, py: i32) -> Option<(WinId, Rect)> {
    c.order()
        .into_iter()
        .rev()
        .map(|id| (id, c.rect(id).unwrap()))
        .find(|(_, f)| px >= f.x && py >= f.y && px < f.x + f.w as i32 && py < f.y + f.h as i32)
}

fn colour(r: &mut Rng) -> Rgba {
    let a = if r.chance(1, 3) { 128 } else { 255 };
    Rgba(
        r.below(256) as u8,
        r.below(256) as u8,
        r.below(256) as u8,
        a,
    )
}

#[test]
fn random_sessions_pixel_identity_damage_and_routing() {
    let mut r = Rng::new(SEED);
    for case in 0..120 {
        let mut c = Compositor::new(W, H);
        let mut shadow: BTreeMap<WinId, Canvas> = BTreeMap::new();
        let mut t = 0u64;
        let mut prev = c.screen().clone();
        let mut dragging = false;
        for step in 0..60 {
            let ctx = format!("case {case} step {step}");
            let ids: Vec<WinId> = shadow.keys().copied().collect();
            let pick = |r: &mut Rng| *r.pick(&ids);
            match r.below(12) {
                0 | 1 if ids.len() < 6 => {
                    let id = c.open("w", 10 + r.below(70) as u32, 5 + r.below(50) as u32, t);
                    assert_eq!(c.focused(), Some(id), "{ctx}: new window focused");
                    assert_eq!(c.order().last(), Some(&id), "{ctx}: new window on top");
                    shadow.insert(id, c.surface_mut(id).unwrap().clone());
                }
                2 if !ids.is_empty() => {
                    let id = pick(&mut r);
                    assert!(c.close(id, t));
                    shadow.remove(&id);
                    assert!(!c.order().contains(&id) && c.rect(id).is_none());
                }
                3 | 4 if !ids.is_empty() => {
                    let id = pick(&mut r);
                    let s = c.surface_mut(id).unwrap();
                    let rect = Rect {
                        x: r.below(80) as i32 - 5,
                        y: r.below(60) as i32 - 5,
                        w: r.below(40) as u32,
                        h: r.below(30) as u32,
                    };
                    let col = colour(&mut r);
                    s.fill_rect(rect, col);
                    shadow.get_mut(&id).unwrap().fill_rect(rect, col);
                }
                5 if !ids.is_empty() => {
                    let id = pick(&mut r);
                    c.move_to(id, r.below(260) as i32 - 50, r.below(200) as i32 - 40, t);
                }
                6 if !ids.is_empty() => {
                    let id = pick(&mut r);
                    c.resize(id, 10 + r.below(70) as u32, 5 + r.below(50) as u32, t);
                    shadow.insert(id, c.surface_mut(id).unwrap().clone());
                }
                7 if !ids.is_empty() => {
                    let id = pick(&mut r);
                    c.raise(id);
                    assert_eq!(c.order().last(), Some(&id), "{ctx}: raise");
                }
                8 => c.set_layout(
                    if r.chance(1, 2) {
                        Layout::Tiling
                    } else {
                        Layout::Floating
                    },
                    t,
                ),
                9 => {
                    // pointer: routing is judged against the state of the last frame (input at the same time)
                    let (px, py) = (r.below(W as usize) as i32, r.below(H as usize) as i32);
                    let ev = match r.below(3) {
                        0 => Event::MouseDown { x: px, y: py },
                        1 => Event::MouseMove { x: px, y: py },
                        _ => Event::MouseUp { x: px, y: py },
                    };
                    let hit = top_at(&c, px, py);
                    let got = c.input(ev, false, t);
                    match (hit, ev) {
                        _ if dragging => {}
                        (None, _) => assert_eq!(got, Routed::Dropped, "{ctx}: desktop {ev:?}"),
                        (Some((id, f)), _) => {
                            let (cx, cy) = (px - f.x - 1, py - f.y - 19);
                            let in_content =
                                cx >= 0 && cy >= 0 && cx < f.w as i32 - 2 && cy < f.h as i32 - 20;
                            if in_content {
                                let local = match ev {
                                    Event::MouseDown { .. } => Event::MouseDown { x: cx, y: cy },
                                    Event::MouseMove { .. } => Event::MouseMove { x: cx, y: cy },
                                    _ => Event::MouseUp { x: cx, y: cy },
                                };
                                assert_eq!(got, Routed::ToWindow(id, local), "{ctx}");
                            }
                            if matches!(ev, Event::MouseDown { .. }) {
                                assert_eq!(c.focused(), Some(id), "{ctx}: click focuses");
                            }
                        }
                    }
                    dragging = matches!(ev, Event::MouseDown { .. })
                        && matches!(got, Routed::Handled)
                        || (dragging && !matches!(ev, Event::MouseUp { .. }));
                }
                10 => {
                    let key = *r.pick(&[
                        Event::Key(Key::Tab),
                        Event::Key(Key::ShiftTab),
                        Event::Text('['),
                        Event::Text(']'),
                        Event::Key(Key::Enter),
                        Event::Text('a'),
                    ]);
                    let sup = r.chance(1, 2);
                    let before = c.focused();
                    let got = c.input(key, sup, t);
                    let shortcut = sup
                        && matches!(
                            key,
                            Event::Key(Key::Tab | Key::ShiftTab) | Event::Text('[' | ']')
                        );
                    if shortcut {
                        assert_eq!(got, Routed::Handled, "{ctx}");
                    } else {
                        let want = before.map_or(Routed::Dropped, |id| Routed::ToWindow(id, key));
                        assert_eq!(got, want, "{ctx}: key routing");
                    }
                }
                _ => {}
            }
            t += *r.pick(&[0u64, 5, 16, 33, 90, 200]);
            let dmg = c.frame(t);
            let cur = c.screen().clone();
            check_damage(&prev, &cur, &dmg, &ctx);
            let want = reference(&c, &shadow);
            if cur != want {
                let i = cur
                    .pixels
                    .iter()
                    .zip(&want.pixels)
                    .position(|(a, b)| a != b)
                    .unwrap();
                panic!(
                    "{ctx}: screen differs from naive composite at ({},{}): {:?} vs {:?}",
                    i as u32 % W,
                    i as u32 / W,
                    cur.pixels[i],
                    want.pixels[i]
                );
            }
            prev = cur;
        }
    }
}

#[test]
fn move_animation_ends_on_target_and_is_monotone() {
    let mut c = Compositor::new(W, H);
    let id = c.open("a", 40, 30, 0);
    c.frame(0);
    let from = c.rect(id).unwrap();
    c.move_to(id, from.x + 100, from.y + 60, 10);
    let mut last = from.x;
    for t in [10u64, 40, 80, 120, 170] {
        c.frame(t);
        let x = c.rect(id).unwrap().x;
        assert!(x >= last && x <= from.x + 100, "t {t}: x {x} not monotone");
        last = x;
    }
    c.frame(190);
    let end = c.rect(id).unwrap();
    assert_eq!(
        (end.x, end.y, end.w, end.h),
        (from.x + 100, from.y + 60, from.w, from.h)
    );
    assert!(c.frame(400).is_empty(), "settled window must not damage");
}

#[test]
fn tiling_geometry_master_and_stack() {
    for n in 1..=5u32 {
        let mut c = Compositor::new(W, H);
        let ids: Vec<WinId> = (0..n).map(|i| c.open("t", 30, 20, u64::from(i))).collect();
        c.set_layout(Layout::Tiling, 100);
        c.frame(100 + 180);
        let rects: Vec<Rect> = ids.iter().map(|&id| c.rect(id).unwrap()).collect();
        let m = rects[0];
        assert_eq!((m.x, m.y), (0, 0), "n {n}: master at origin");
        assert_eq!(m.h, H, "n {n}: master full height");
        if n == 1 {
            assert!(m.w == W, "n 1: single window fills the screen, got {m:?}");
            continue;
        }
        assert!(
            m.w.abs_diff(W / 2) <= 1,
            "n {n}: master half width, got {m:?}"
        );
        let mut y = 0i32;
        let mut area = u64::from(m.w) * u64::from(m.h);
        for s in &rects[1..] {
            assert_eq!(s.x, m.w as i32, "n {n}: stack right of master {s:?}");
            assert_eq!(s.x as u32 + s.w, W, "n {n}: stack reaches the right edge");
            assert_eq!(s.y, y, "n {n}: stack in open order, top to bottom");
            y += s.h as i32;
            area += u64::from(s.w) * u64::from(s.h);
        }
        assert_eq!(y as u32, H, "n {n}: stack fills the height");
        assert_eq!(
            area,
            u64::from(W) * u64::from(H),
            "n {n}: tiles cover the screen once"
        );
    }
}
