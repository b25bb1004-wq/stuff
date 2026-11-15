//! Integration tests for `helios::draw`.
#![allow(clippy::unwrap_used)]

use helios::draw::{draw_world, to_screen, Camera};
use helios::fx::{Fx, Vec2};
use helios::phys::{Body, Shape, World};
use pigment::{Canvas, Rgba};

fn fx(n: i32) -> Fx {
    Fx::from_int(n)
}

#[test]
fn to_screen_projects_world_y_up_to_screen_y_down() {
    let cam = Camera {
        center: Vec2::new(fx(2), fx(-1)),
        scale: fx(10),
    };
    assert_eq!(to_screen(&cam, 100, 80, cam.center), (50, 40));
    assert_eq!(to_screen(&cam, 100, 80, Vec2::new(fx(3), fx(1))), (60, 20));
    assert_eq!(to_screen(&cam, 101, 81, Vec2::new(fx(0), fx(0))), (30, 30));
}

#[test]
fn draw_world_paints_shapes_and_respects_id_order() {
    let zero = Vec2::default();
    let mut w = World::new(zero);
    let box_id = w.add(Body {
        pos: zero,
        vel: zero,
        inv_mass: fx(1),
        restitution: fx(1),
        shape: Shape::Aabb {
            half: Vec2::new(fx(2), fx(1)),
        },
    });
    let circle_id = w.add(Body {
        pos: zero,
        vel: zero,
        inv_mass: fx(1),
        restitution: fx(1),
        shape: Shape::Circle { r: fx(1) },
    });

    let cam = Camera {
        center: zero,
        scale: fx(10),
    };
    let bg = Rgba(0, 0, 0, 255);
    let red = Rgba(255, 0, 0, 255);
    let blue = Rgba(0, 0, 255, 255);
    let mut canvas = Canvas::new(100, 100, bg);

    draw_world(&mut canvas, &cam, &w, &|id| {
        if id == box_id {
            red
        } else {
            blue
        }
    });

    // Circle has higher id, so centre (50, 50) is blue
    assert_eq!(canvas.get(50, 50), Some(blue));
    // Inside box (x = 1.5 -> px 65, y = 0 -> py 50) but outside circle (r = 10px) is red
    assert_eq!(canvas.get(65, 50), Some(red));
    // More than 1 pixel outside all bodies is untouched bg
    assert_eq!(canvas.get(72, 50), Some(bg));
    assert_eq!(canvas.get(50, 62), Some(bg));
    assert_eq!(canvas.get(0, 0), Some(bg));
    let _ = circle_id;
}
