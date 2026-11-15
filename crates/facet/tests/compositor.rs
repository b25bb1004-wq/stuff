//! Integration tests for `facet::Compositor`.
#![allow(clippy::unwrap_used)]

use facet::{Compositor, Layout, Routed};
use glint::anim::{tween, Ease};
use glint::{Event, Key};
use pigment::{Rect, Rgba};

#[test]
fn floating_open_focus_raise_and_click_routing() {
    let mut c = Compositor::new(400, 300);
    assert_eq!(c.focused(), None);
    assert_eq!(c.order(), Vec::<u32>::new());

    let w1 = c.open("one", 100, 80, 0);
    let w2 = c.open("two", 100, 80, 0);
    assert_eq!(c.focused(), Some(w2));
    assert_eq!(c.order(), vec![w1, w2]);

    c.move_to(w1, 10, 10, 0);
    c.move_to(w2, 50, 40, 0);
    let dmg = c.frame(200);
    assert!(!dmg.is_empty());
    assert_eq!(
        c.rect(w1),
        Some(Rect {
            x: 10,
            y: 10,
            w: 102,
            h: 100
        })
    );
    assert_eq!(
        c.rect(w2),
        Some(Rect {
            x: 50,
            y: 40,
            w: 102,
            h: 100
        })
    );

    // Click in overlap region hits topmost w2 in content coords (x: 51..151, y: 59..139).
    let r_overlap = c.input(Event::MouseDown { x: 70, y: 70 }, false, 210);
    assert_eq!(
        r_overlap,
        Routed::ToWindow(w2, Event::MouseDown { x: 19, y: 11 })
    );

    // Click exposed part of w1 (e.g. x=20, y=35 -> content (20 - 11, 35 - 29) = (9, 6)) focuses and raises w1.
    let r_w1 = c.input(Event::MouseDown { x: 20, y: 35 }, false, 220);
    assert_eq!(r_w1, Routed::ToWindow(w1, Event::MouseDown { x: 9, y: 6 }));
    assert_eq!(c.focused(), Some(w1));
    assert_eq!(c.order(), vec![w2, w1]);

    // Click desktop background drops the event and keeps focus.
    let r_bg = c.input(Event::MouseDown { x: 350, y: 250 }, false, 230);
    assert_eq!(r_bg, Routed::Dropped);
    assert_eq!(c.focused(), Some(w1));

    //Explicit raise brings w2 back on top without changing focus.
    c.raise(w2);
    assert_eq!(c.order(), vec![w1, w2]);
    assert_eq!(c.focused(), Some(w1));
}

#[test]
fn floating_title_bar_drag_and_animation_curves() {
    let mut c = Compositor::new(300, 200);
    let id = c.open("drag", 80, 60, 0);
    c.move_to(id, 20, 20, 0);
    let _ = c.frame(180);

    // Verify 180 ms Ease::OutCubic animation intermediate and final rects.
    c.move_to(id, 120, 80, 1000);
    let _ = c.frame(1090);
    let mid = c.rect(id).unwrap();
    assert_eq!(mid.x, tween(20, 120, 1000, 180, 1090, Ease::OutCubic));
    assert_eq!(mid.y, tween(20, 80, 1000, 180, 1090, Ease::OutCubic));
    assert!(mid.x > 70, "OutCubic should be past midpoint at t=50%");

    let _ = c.frame(1180);
    assert_eq!(
        c.rect(id),
        Some(Rect {
            x: 120,
            y: 80,
            w: 82,
            h: 80
        })
    );

    // Click title bar (y = 80 + 5 = 85) and drag partially off screen to (-30, -10).
    assert_eq!(
        c.input(Event::MouseDown { x: 140, y: 85 }, false, 1200),
        Routed::Handled
    );
    assert_eq!(
        c.input(Event::MouseMove { x: -10, y: -5 }, false, 1205),
        Routed::Handled
    );
    assert_eq!(
        c.rect(id),
        Some(Rect {
            x: -30,
            y: -10,
            w: 82,
            h: 80
        })
    );
    let dmg = c.frame(1205);
    assert!(!dmg.is_empty());
    for r in dmg {
        assert!(r.x >= 0 && r.y >= 0 && r.x + r.w as i32 <= 300 && r.y + r.h as i32 <= 200);
    }

    assert_eq!(
        c.input(Event::MouseUp { x: -10, y: -5 }, false, 1210),
        Routed::Handled
    );
    // Subsequent move no longer drags.
    assert_eq!(
        c.input(Event::MouseMove { x: 200, y: 150 }, false, 1220),
        Routed::Dropped
    );
    assert_eq!(
        c.rect(id),
        Some(Rect {
            x: -30,
            y: -10,
            w: 82,
            h: 80
        })
    );
}

#[test]
fn tiling_master_stack_and_super_shortcuts() {
    let mut c = Compositor::new(401, 301);
    let w1 = c.open("a", 100, 60, 0);
    let w2 = c.open("b", 100, 60, 0);
    let w3 = c.open("c", 100, 60, 0);
    c.move_to(w1, 10, 10, 0);
    c.move_to(w2, 40, 40, 0);
    c.move_to(w3, 80, 80, 0);
    let _ = c.frame(180);

    c.set_layout(Layout::Tiling, 200);
    let _ = c.frame(290);
    assert_ne!(
        c.rect(w1),
        Some(Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 301
        })
    );

    let _ = c.frame(380);
    assert_eq!(
        c.rect(w1),
        Some(Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 301
        })
    );
    assert_eq!(
        c.rect(w2),
        Some(Rect {
            x: 200,
            y: 0,
            w: 201,
            h: 150
        })
    );
    assert_eq!(
        c.rect(w3),
        Some(Rect {
            x: 200,
            y: 150,
            w: 201,
            h: 151
        })
    );

    // Focus is on w3; Super+Tab cycles w3 -> w1 -> w2 without raising in Tiling.
    assert_eq!(c.focused(), Some(w3));
    let initial_order = c.order();
    assert_eq!(c.input(Event::Key(Key::Tab), true, 400), Routed::Handled);
    assert_eq!(c.focused(), Some(w1));
    assert_eq!(c.order(), initial_order);

    assert_eq!(
        c.input(Event::Key(Key::ShiftTab), true, 410),
        Routed::Handled
    );
    assert_eq!(c.focused(), Some(w3));

    // Super+'[' moves w3 earlier in tiling order: [w1, w3, w2].
    assert_eq!(c.input(Event::Text('['), true, 420), Routed::Handled);
    let _ = c.frame(600);
    assert_eq!(
        c.rect(w3),
        Some(Rect {
            x: 200,
            y: 0,
            w: 201,
            h: 150
        })
    );
    assert_eq!(
        c.rect(w2),
        Some(Rect {
            x: 200,
            y: 150,
            w: 201,
            h: 151
        })
    );

    // Super+']' moves w3 back later: [w1, w2, w3].
    assert_eq!(c.input(Event::Text(']'), true, 610), Routed::Handled);
    let _ = c.frame(790);
    assert_eq!(
        c.rect(w2),
        Some(Rect {
            x: 200,
            y: 0,
            w: 201,
            h: 150
        })
    );

    // Switch back to Floating animates windows back to their floating rects.
    c.set_layout(Layout::Floating, 800);
    let _ = c.frame(980);
    assert_eq!(
        c.rect(w1),
        Some(Rect {
            x: 10,
            y: 10,
            w: 102,
            h: 80
        })
    );
    assert_eq!(
        c.rect(w2),
        Some(Rect {
            x: 40,
            y: 40,
            w: 102,
            h: 80
        })
    );
    assert_eq!(
        c.rect(w3),
        Some(Rect {
            x: 80,
            y: 80,
            w: 102,
            h: 80
        })
    );

    // In Floating, Super+Tab raises the newly focused window.
    assert_eq!(c.focused(), Some(w3));
    assert_eq!(c.input(Event::Key(Key::Tab), true, 990), Routed::Handled);
    assert_eq!(c.focused(), Some(w1));
    assert_eq!(c.order().last().copied(), Some(w1));
}

#[test]
fn surface_damage_resize_close_and_key_routing() {
    let mut c = Compositor::new(300, 200);
    let id = c.open("app", 100, 80, 0);
    c.move_to(id, 20, 20, 0);
    let _ = c.frame(180);

    // Idle frame returns empty damage.
    assert!(c.frame(200).is_empty());

    // Paint a 10x10 red square inside client surface; damage covers just that 10x10 region.
    {
        let s = c.surface_mut(id).unwrap();
        s.fill_rect(
            Rect {
                x: 15,
                y: 10,
                w: 10,
                h: 10,
            },
            Rgba(240, 40, 40, 255),
        );
    }
    let dmg = c.frame(210);
    assert_eq!(
        dmg,
        vec![Rect {
            x: 36,
            y: 49,
            w: 10,
            h: 10
        }]
    );
    assert_eq!(c.screen().get(36, 49), Some(Rgba(240, 40, 40, 255)));

    // Resize larger preserves top-left pixels; resize smaller clears vacated area.
    c.resize(id, 120, 90, 220);
    let _ = c.frame(220);
    assert_eq!(c.screen().get(36, 49), Some(Rgba(240, 40, 40, 255)));

    c.resize(id, 50, 40, 230);
    let shrink_dmg = c.frame(230);
    assert!(!shrink_dmg.is_empty());
    assert_eq!(
        c.rect(id),
        Some(Rect {
            x: 20,
            y: 20,
            w: 52,
            h: 60
        })
    );

    // Out-of-bounds partial damage does not panic.
    c.damage(
        id,
        Rect {
            x: -50,
            y: -50,
            w: 20,
            h: 20,
        },
    );
    assert!(c.frame(240).is_empty());

    // Regular keys & text route to focused window.
    assert_eq!(
        c.input(Event::Key(Key::Enter), false, 250),
        Routed::ToWindow(id, Event::Key(Key::Enter))
    );
    assert_eq!(
        c.input(Event::Text('['), false, 255),
        Routed::ToWindow(id, Event::Text('['))
    );

    // Closing clears screen back to desktop colour.
    assert!(c.close(id, 260));
    assert!(!c.close(id, 260));
    assert_eq!(c.focused(), None);
    let close_dmg = c.frame(260);
    assert!(!close_dmg.is_empty());
    assert_eq!(c.screen().get(20, 20), c.screen().get(0, 0));
}
