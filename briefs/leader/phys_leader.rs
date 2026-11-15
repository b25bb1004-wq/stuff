//! Leader check for T69: properties of helios::phys and the renderer.
#![allow(
    clippy::unwrap_used,
    clippy::type_complexity,
    clippy::cast_possible_truncation
)]
use assay::Rng;
use pigment::{Canvas, Rgba};
use helios::draw::{draw_world, to_screen, Camera};
use helios::fx::{Fx, Vec2};
use helios::phys::*;

fn fx(x: f64) -> Fx {
    Fx((x * 65536.0).round() as i64)
}
fn fl(x: Fx) -> f64 {
    x.0 as f64 / 65536.0
}
fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(fx(x), fx(y))
}
fn circle(p: Vec2, vel: Vec2, r: f64, inv_mass: f64, e: f64) -> Body {
    Body {
        pos: p,
        vel,
        inv_mass: fx(inv_mass),
        restitution: fx(e),
        shape: Shape::Circle { r: fx(r) },
    }
}
fn aabb(p: Vec2, hx: f64, hy: f64, inv_mass: f64, e: f64) -> Body {
    Body {
        pos: p,
        vel: v(0.0, 0.0),
        inv_mass: fx(inv_mass),
        restitution: fx(e),
        shape: Shape::Aabb { half: v(hx, hy) },
    }
}
fn dt() -> Fx {
    Fx::from_ratio(1, 60)
}

#[test]
fn collide_cases() {
    let a = circle(v(0.0, 0.0), v(0.0, 0.0), 1.0, 1.0, 1.0);
    let b = circle(v(1.5, 0.0), v(0.0, 0.0), 1.0, 1.0, 1.0);
    let (n, d) = collide(&a, &b).unwrap();
    assert_eq!((n.x, n.y), (Fx::ONE, Fx::ZERO));
    assert!((fl(d) - 0.5).abs() < 1e-3);
    let far = circle(v(2.0, 0.0), v(0.0, 0.0), 1.0, 1.0, 1.0);
    assert!(collide(&a, &far).is_none(), "touching is no contact");
    let same = circle(v(0.0, 0.0), v(0.0, 0.0), 0.5, 1.0, 1.0);
    assert_eq!(collide(&a, &same).unwrap().0, Vec2::new(Fx::ONE, Fx::ZERO));
    let bx = aabb(v(0.0, 0.0), 2.0, 1.0, 0.0, 1.0);
    let c = circle(v(0.0, 1.5), v(0.0, 0.0), 1.0, 1.0, 1.0);
    let (n, d) = collide(&bx, &c).unwrap();
    assert_eq!((n.x, n.y), (Fx::ZERO, Fx::ONE));
    assert!((fl(d) - 0.5).abs() < 1e-3);
    let b2 = aabb(v(1.0, 1.0), 1.0, 1.0, 1.0, 1.0);
    let sq = aabb(v(0.0, 0.0), 1.0, 1.0, 1.0, 1.0);
    let (n, _) = collide(&sq, &b2).unwrap();
    assert_eq!((n.x, n.y), (Fx::ONE, Fx::ZERO), "ties pick x");
}

fn momentum(w: &World) -> (f64, f64) {
    w.ids().iter().fold((0.0, 0.0), |acc, &id| {
        let b = w.body(id).unwrap();
        let m = 1.0 / fl(b.inv_mass);
        (acc.0 + fl(b.vel.x) * m, acc.1 + fl(b.vel.y) * m)
    })
}

fn random_scene(r: &mut Rng, n: usize) -> World {
    let mut w = World::new(v(0.0, 0.0));
    for i in 0..n {
        let p = v(
            (i % 5) as f64 * 3.0 + r.below(100) as f64 / 100.0,
            (i / 5) as f64 * 3.0,
        );
        let vel = v(
            r.below(800) as f64 / 100.0 - 4.0,
            r.below(800) as f64 / 100.0 - 4.0,
        );
        let inv = [1.0, 0.5, 0.25][r.below(3)];
        let e = r.below(101) as f64 / 100.0;
        if r.below(2) == 0 {
            w.add(circle(p, vel, 0.4 + r.below(60) as f64 / 100.0, inv, e));
        } else {
            let mut b = aabb(p, 0.5, 0.4, inv, e);
            b.vel = vel;
            w.add(b);
        }
    }
    w
}

#[test]
fn momentum_energy_determinism() {
    let mut r = Rng::new(0x69_0001);
    for case in 0..8 {
        let mut w = random_scene(&mut r, 20);
        let mut twin = w.clone();
        let (px, py) = momentum(&w);
        let e0 = fl(w.kinetic_energy());
        for _ in 0..240 {
            w.step(dt());
            twin.step(dt());
            let e = fl(w.kinetic_energy());
            assert!(
                e <= e0 * 1.01 + 1e-3,
                "case {case}: energy {e} > start {e0}"
            );
        }
        let (qx, qy) = momentum(&w);
        let tol = 0.01 * (1.0 + px.abs() + py.abs()) + 0.05;
        assert!(
            (qx - px).abs() < tol && (qy - py).abs() < tol,
            "case {case}: momentum ({px},{py}) -> ({qx},{qy})"
        );
        for id in w.ids() {
            assert_eq!(w.body(id), twin.body(id), "case {case}: not deterministic");
        }
    }
}

#[test]
fn bullets_never_tunnel() {
    let mut r = Rng::new(0x69_0002);
    for case in 0..40 {
        let mut w = World::new(v(0.0, 0.0));
        w.add(aabb(v(0.0, 0.0), 0.05, 20.0, 0.0, 1.0));
        let rad = 0.05 + r.below(20) as f64 / 100.0;
        let speed = 1.0 + (32.0 * rad * 60.0 - 1.0) * r.below(1001) as f64 / 1000.0;
        let e = r.below(101) as f64 / 100.0;
        let id = w.add(circle(
            v(-6.0, r.below(2000) as f64 / 100.0 - 10.0),
            v(speed, 0.0),
            rad,
            1.0,
            e,
        ));
        for _ in 0..240 {
            w.step(dt());
            assert!(
                fl(w.body(id).unwrap().pos.x) < 0.0,
                "case {case}: r {rad} speed {speed} passed the wall"
            );
        }
    }
}

#[test]
fn stack_settles() {
    let mut w = World::new(v(0.0, -10.0));
    w.add(aabb(v(0.0, -1.0), 20.0, 1.0, 0.0, 0.0));
    let ids: Vec<_> = (0..5)
        .map(|i| w.add(aabb(v(0.0, 0.55 + 1.05 * i as f64), 0.5, 0.5, 1.0, 0.0)))
        .collect();
    for _ in 0..600 {
        w.step(dt());
    }
    let mut below_top = 0.0;
    for (i, &id) in ids.iter().enumerate() {
        let b = w.body(id).unwrap();
        let bottom = fl(b.pos.y) - 0.5;
        assert!(
            below_top - bottom < 0.02,
            "box {i} sunk {}",
            below_top - bottom
        );
        assert!(
            fl(b.vel.y).abs() < 0.1 && fl(b.vel.x).abs() < 0.1,
            "box {i} still moving"
        );
        below_top = fl(b.pos.y) + 0.5;
    }
}

#[test]
fn stress_box_keeps_everything_inside() {
    let mut r = Rng::new(0x69_0003);
    let mut w = World::new(v(0.0, -9.8));
    for (x, y, hx, hy) in [
        (0.0, -11.0, 12.0, 1.0),
        (0.0, 11.0, 12.0, 1.0),
        (-11.0, 0.0, 1.0, 12.0),
        (11.0, 0.0, 1.0, 12.0),
    ] {
        w.add(aabb(v(x, y), hx, hy, 0.0, 0.5));
    }
    let mut ids = Vec::new();
    for i in 0..200 {
        let p = v(-9.0 + (i % 20) as f64 * 0.9, -9.0 + (i / 20) as f64 * 0.9);
        let vel = v(
            r.below(1000) as f64 / 100.0 - 5.0,
            r.below(1000) as f64 / 100.0 - 5.0,
        );
        ids.push(w.add(circle(p, vel, 0.3, 1.0, 0.5)));
    }
    for _ in 0..300 {
        w.step(dt());
    }
    for id in ids {
        let p = w.body(id).unwrap().pos;
        assert!(
            fl(p.x).abs() < 10.0 && fl(p.y).abs() < 10.0,
            "body {id} escaped to ({}, {})",
            fl(p.x),
            fl(p.y)
        );
    }
}

#[test]
fn render_pixels() {
    let cam = Camera {
        center: v(0.0, 0.0),
        scale: fx(10.0),
    };
    assert_eq!(to_screen(&cam, 100, 80, v(0.0, 0.0)), (50, 40));
    assert_eq!(to_screen(&cam, 100, 80, v(1.0, 1.0)), (60, 30));
    assert_eq!(to_screen(&cam, 101, 81, v(-2.0, -1.5)), (30, 55));
    let mut w = World::new(v(0.0, 0.0));
    let c = w.add(circle(v(-2.0, 0.0), v(0.0, 0.0), 1.0, 1.0, 1.0));
    let b = w.add(aabb(v(2.0, 1.0), 1.0, 0.5, 0.0, 1.0));
    let bg = Rgba(1, 2, 3, 255);
    let mut cv = Canvas::new(100, 80, bg);
    let col = |id: BodyId| {
        if id == c {
            Rgba(200, 0, 0, 255)
        } else {
            Rgba(0, 200, 0, 255)
        }
    };
    draw_world(&mut cv, &cam, &w, &col);
    let (cx, cy) = to_screen(&cam, 100, 80, w.body(c).unwrap().pos);
    assert_eq!(cv.get(cx, cy), Some(col(c)));
    let (bx, by) = to_screen(&cam, 100, 80, w.body(b).unwrap().pos);
    assert_eq!(cv.get(bx, by), Some(col(b)));
    assert_eq!(cv.get(2, 2), Some(bg));
    assert_eq!(cv.get(50, 70), Some(bg));
}
