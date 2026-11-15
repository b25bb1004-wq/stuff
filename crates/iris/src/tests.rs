#![allow(clippy::unwrap_used)]

use assay::{forall, Rng};
use indium::{Event, Key};
use pigment::{Canvas, Rect, Rgba};

use crate::{Compositor, Layout, Routed, WinId};

fn count_diff_pixels(a: &Canvas, b: &Canvas) -> (u64, Vec<(i32, i32)>) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut diffs = Vec::new();
    let w = a.width as usize;
    for (idx, (&pa, &pb)) in a.pixels.iter().zip(b.pixels.iter()).enumerate() {
        if pa != pb {
            diffs.push(((idx % w) as i32, (idx / w) as i32));
        }
    }
    (diffs.len() as u64, diffs)
}

fn rect_contains(r: Rect, x: i32, y: i32) -> bool {
    let x0 = i64::from(r.x);
    let y0 = i64::from(r.y);
    let x1 = x0 + i64::from(r.w);
    let y1 = y0 + i64::from(r.h);
    let px = i64::from(x);
    let py = i64::from(y);
    px >= x0 && px < x1 && py >= y0 && py < y1
}

#[test]
fn forall_random_sessions_pixel_identical_and_damage_bounded() {
    forall(150, 0x57A7_FACE_0063, |rng: &mut Rng| {
        let w = rng.range(120, 320) as u32;
        let h = rng.range(100, 240) as u32;
        let mut comp = Compositor::new(w, h);
        let mut ids: Vec<WinId> = Vec::new();
        let mut now_ms = 0u64;

        // Seed with 1 or 2 initial windows.
        for _ in 0..rng.range(1, 3) {
            let ww = rng.range(20, 120) as u32;
            let wh = rng.range(20, 100) as u32;
            let id = comp.open("init", ww, wh, now_ms);
            ids.push(id);
        }

        let steps = rng.range(20, 50);
        for _ in 0..steps {
            let op_count = rng.range(0, 5);
            for _ in 0..op_count {
                match rng.range(0, 10) {
                    0 => {
                        if ids.len() < 7 {
                            let ww = rng.range(0, 150) as u32;
                            let wh = rng.range(0, 120) as u32;
                            let id = comp.open("win", ww, wh, now_ms);
                            ids.push(id);
                        }
                    }
                    1 => {
                        if !ids.is_empty() {
                            let idx = rng.below(ids.len());
                            let id = ids.swap_remove(idx);
                            assert!(comp.close(id, now_ms));
                        }
                    }
                    2 => {
                        if !ids.is_empty() {
                            let id = *rng.pick(&ids);
                            if let Some(s) = comp.surface_mut(id) {
                                let col = Rgba(
                                    rng.range(0, 256) as u8,
                                    rng.range(0, 256) as u8,
                                    rng.range(0, 256) as u8,
                                    if rng.chance(1, 5) { 180 } else { 255 },
                                );
                                let rx = rng.range(0, 100) as i32 - 10;
                                let ry = rng.range(0, 100) as i32 - 10;
                                let rw = rng.range(1, 60) as u32;
                                let rh = rng.range(1, 60) as u32;
                                s.fill_rect(
                                    Rect {
                                        x: rx,
                                        y: ry,
                                        w: rw,
                                        h: rh,
                                    },
                                    col,
                                );
                            }
                        }
                    }
                    3 => {
                        if !ids.is_empty() {
                            let id = *rng.pick(&ids);
                            let r = Rect {
                                x: rng.range(0, 120) as i32 - 20,
                                y: rng.range(0, 120) as i32 - 20,
                                w: rng.range(0, 80) as u32,
                                h: rng.range(0, 80) as u32,
                            };
                            comp.damage(id, r);
                        }
                    }
                    4 => {
                        let layout = if rng.chance(1, 2) {
                            Layout::Tiling
                        } else {
                            Layout::Floating
                        };
                        comp.set_layout(layout, now_ms);
                    }
                    5 => {
                        if !ids.is_empty() {
                            let id = *rng.pick(&ids);
                            let x = rng.range(0, 400) as i32 - 80;
                            let y = rng.range(0, 300) as i32 - 60;
                            comp.move_to(id, x, y, now_ms);
                        }
                    }
                    6 => {
                        if !ids.is_empty() {
                            let id = *rng.pick(&ids);
                            let nw = rng.range(0, 180) as u32;
                            let nh = rng.range(0, 140) as u32;
                            comp.resize(id, nw, nh, now_ms);
                        }
                    }
                    7 => {
                        if !ids.is_empty() {
                            let id = *rng.pick(&ids);
                            comp.raise(id);
                        }
                    }
                    8 => {
                        let super_held = rng.chance(1, 3);
                        let ev = match rng.range(0, 7) {
                            0 => Event::MouseDown {
                                x: rng.range(0, w as u64 + 40) as i32 - 20,
                                y: rng.range(0, h as u64 + 40) as i32 - 20,
                            },
                            1 => Event::MouseMove {
                                x: rng.range(0, w as u64 + 40) as i32 - 20,
                                y: rng.range(0, h as u64 + 40) as i32 - 20,
                            },
                            2 => Event::MouseUp {
                                x: rng.range(0, w as u64 + 40) as i32 - 20,
                                y: rng.range(0, h as u64 + 40) as i32 - 20,
                            },
                            3 => Event::Key(Key::Tab),
                            4 => Event::Key(Key::ShiftTab),
                            5 => Event::Text('['),
                            _ => Event::Text(']'),
                        };
                        let _ = comp.input(ev, super_held, now_ms);
                    }
                    _ => {
                        // Do nothing (tests idle frames returning empty damage).
                    }
                }
            }

            now_ms += rng.range(0, 120);
            let prev_screen = comp.screen().clone();
            let damage = comp.frame(now_ms);
            let expected_screen = comp.full_render(now_ms);

            // 1. Pixel identity with full re-composite from scratch.
            assert_eq!(comp.screen(), &expected_screen);

            // 2. All returned rects are non-empty and clipped to screen.
            let mut total_returned_area = 0u64;
            for &r in &damage {
                assert!(r.w > 0 && r.h > 0, "zero-size rect returned: {r:?}");
                assert!(r.x >= 0 && r.y >= 0, "negative rect origin: {r:?}");
                assert!(
                    i64::from(r.x) + i64::from(r.w) <= i64::from(w),
                    "rect exceeds width: {r:?}"
                );
                assert!(
                    i64::from(r.y) + i64::from(r.h) <= i64::from(h),
                    "rect exceeds height: {r:?}"
                );
                total_returned_area += u64::from(r.w) * u64::from(r.h);
            }

            // 3. Every changed pixel is covered and returned area satisfies bound.
            let (true_changed, diffs) = count_diff_pixels(&prev_screen, comp.screen());
            if true_changed == 0 {
                assert!(
                    damage.is_empty(),
                    "expected empty damage when 0 pixels changed, got {damage:?}"
                );
            } else {
                for &(px, py) in &diffs {
                    assert!(
                        damage.iter().any(|&r| rect_contains(r, px, py)),
                        "changed pixel ({px}, {py}) not covered by damage {damage:?}"
                    );
                }
                let limit = true_changed
                    .saturating_mul(2)
                    .saturating_add(4096 * damage.len() as u64);
                assert!(
                    total_returned_area <= limit,
                    "damage area {total_returned_area} > limit {limit} (true_changed={true_changed}, rects={})",
                    damage.len()
                );
            }
        }
    });
}

#[test]
fn idle_frame_and_super_shortcuts_empty_compositor() {
    let mut c = Compositor::new(100, 80);
    assert_eq!(c.focused(), None);
    assert!(c.order().is_empty());
    assert!(c.frame(0).is_empty());
    assert_eq!(c.input(Event::Key(Key::Tab), true, 10), Routed::Handled);
    assert_eq!(c.input(Event::Key(Key::Enter), false, 10), Routed::Dropped);
    assert!(c.frame(10).is_empty());
}
