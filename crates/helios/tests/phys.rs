//! Integration tests for `helios::phys`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use helios::fx::{Fx, Vec2};
use helios::phys::{collide, Body, Shape, World};

fn fx(n: i32) -> Fx {
    Fx::from_int(n)
}

fn f_rat(n: i64, d: i64) -> Fx {
    Fx::from_ratio(n, d)
}

fn v(x: Fx, y: Fx) -> Vec2 {
    Vec2::new(x, y)
}

fn circle(pos: Vec2, vel: Vec2, inv_mass: Fx, e: Fx, r: Fx) -> Body {
    Body {
        pos,
        vel,
        inv_mass,
        restitution: e,
        shape: Shape::Circle { r },
    }
}

fn aabb(pos: Vec2, vel: Vec2, inv_mass: Fx, e: Fx, half: Vec2) -> Body {
    Body {
        pos,
        vel,
        inv_mass,
        restitution: e,
        shape: Shape::Aabb { half },
    }
}

#[test]
fn collide_circle_circle_cases() {
    let a = circle(v(fx(0), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(2));
    // Separated
    let b_far = circle(v(fx(5), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(2));
    assert_eq!(collide(&a, &b_far), None);

    // Touching without overlap (depth 0)
    let b_touch = circle(v(fx(4), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(2));
    assert_eq!(collide(&a, &b_touch), None);

    // Overlapping along +X
    let b_ov = circle(v(fx(3), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(2));
    assert_eq!(collide(&a, &b_ov), Some((v(fx(1), fx(0)), fx(1))));

    // Coincident centres use normal (1, 0)
    let b_same = circle(v(fx(0), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(1));
    assert_eq!(collide(&a, &b_same), Some((v(fx(1), fx(0)), fx(3))));
}

#[test]
fn collide_aabb_aabb_cases() {
    let a = aabb(
        v(fx(0), fx(0)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        v(fx(2), fx(2)),
    );
    // Touching
    let b_touch = aabb(
        v(fx(4), fx(0)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        v(fx(2), fx(2)),
    );
    assert_eq!(collide(&a, &b_touch), None);

    // Least penetration along X
    let b_x = aabb(
        v(fx(3), fx(1)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        v(fx(2), fx(2)),
    );
    assert_eq!(collide(&a, &b_x), Some((v(fx(1), fx(0)), fx(1))));

    // Least penetration along Y (negative direction)
    let b_y = aabb(
        v(fx(1), fx(-3)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        v(fx(2), fx(2)),
    );
    assert_eq!(collide(&a, &b_y), Some((v(fx(0), fx(-1)), fx(1))));

    // Tie picks X
    let b_tie = aabb(
        v(fx(3), fx(3)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        v(fx(2), fx(2)),
    );
    assert_eq!(collide(&a, &b_tie), Some((v(fx(1), fx(0)), fx(1))));
}

#[test]
fn collide_circle_aabb_cases() {
    let box_b = aabb(
        v(fx(0), fx(0)),
        v(fx(0), fx(0)),
        fx(0),
        fx(1),
        v(fx(2), fx(2)),
    );

    // Touching
    let c_touch = circle(v(fx(3), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(1));
    assert_eq!(collide(&c_touch, &box_b), None);

    // Outside overlapping right face: circle at (2.5, 0), r=1 -> depth 0.5, normal (-1, 0) from c to box
    let c_out = circle(v(f_rat(5, 2), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(1));
    assert_eq!(
        collide(&c_out, &box_b),
        Some((v(fx(-1), fx(0)), f_rat(1, 2)))
    );
    // Reversed order negates normal
    assert_eq!(
        collide(&box_b, &c_out),
        Some((v(fx(1), fx(0)), f_rat(1, 2)))
    );

    // Circle centre inside box: at (1, 0), r=1 -> px = 1, py = 2 -> pushes out +X, so n from c to box is (-1, 0), depth = 2
    let c_in = circle(v(fx(1), fx(0)), v(fx(0), fx(0)), fx(1), fx(1), fx(1));
    assert_eq!(collide(&c_in, &box_b), Some((v(fx(-1), fx(0)), fx(2))));

    // Circle centre inside box closer to bottom face: at (0, -1), r=1 -> px = 2, py = 1 -> pushes out -Y, so n is (0, 1), depth = 2
    let c_in_y = circle(v(fx(0), fx(-1)), v(fx(0), fx(0)), fx(1), fx(1), fx(1));
    assert_eq!(collide(&c_in_y, &box_b), Some((v(fx(0), fx(1)), fx(2))));
}

#[test]
fn ball_bounces_lower_each_time() {
    let mut w = World::new(v(fx(0), fx(-10)));
    // Static floor at y = -1, top at y = 0
    w.add(aabb(
        v(fx(0), fx(-1)),
        v(fx(0), fx(0)),
        fx(0),
        f_rat(1, 2),
        v(fx(10), fx(1)),
    ));
    let ball = w.add(circle(
        v(fx(0), fx(5)),
        v(fx(0), fx(0)),
        fx(1),
        f_rat(1, 2),
        f_rat(1, 2),
    ));

    let dt = f_rat(1, 60);
    let mut peaks = Vec::new();
    let mut prev_vy = fx(0);
    for _ in 0..400 {
        w.step(dt);
        let b = w.body(ball).unwrap();
        if prev_vy > fx(0) && b.vel.y <= fx(0) {
            peaks.push(b.pos.y);
        }
        prev_vy = b.vel.y;
    }
    assert!(peaks.len() >= 3, "expected at least 3 bounces: {peaks:?}");
    assert!(peaks[0] < fx(5));
    assert!(peaks[1] < peaks[0]);
    assert!(peaks[2] < peaks[1]);
}

#[test]
fn head_on_equal_mass_elastic_swap() {
    let mut w = World::new(v(fx(0), fx(0)));
    let a = w.add(circle(
        v(fx(-2), fx(0)),
        v(fx(4), fx(0)),
        fx(1),
        fx(1),
        fx(1),
    ));
    let b = w.add(circle(
        v(fx(2), fx(0)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        fx(1),
    ));
    let dt = f_rat(1, 60);
    for _ in 0..60 {
        w.step(dt);
    }
    assert_eq!(w.body(a).unwrap().vel, v(fx(0), fx(0)));
    assert_eq!(w.body(b).unwrap().vel, v(fx(4), fx(0)));
}

#[test]
fn newtons_cradle_of_three() {
    let mut w = World::new(v(fx(0), fx(0)));
    let b0 = w.add(circle(
        v(fx(-3), fx(0)),
        v(fx(4), fx(0)),
        fx(1),
        fx(1),
        fx(1),
    ));
    let b1 = w.add(circle(
        v(fx(0), fx(0)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        fx(1),
    ));
    let b2 = w.add(circle(
        v(fx(2), fx(0)),
        v(fx(0), fx(0)),
        fx(1),
        fx(1),
        fx(1),
    ));
    let dt = f_rat(1, 60);
    for _ in 0..60 {
        w.step(dt);
    }
    assert_eq!(w.body(b0).unwrap().vel, v(fx(0), fx(0)));
    assert_eq!(w.body(b1).unwrap().vel, v(fx(0), fx(0)));
    assert_eq!(w.body(b2).unwrap().vel, v(fx(4), fx(0)));
}

#[test]
fn fast_bullet_does_not_tunnel_thin_wall() {
    let mut w = World::new(v(fx(0), fx(0)));
    // Thin static wall at x = 10, half-width = 0.5 (extent = 0.5)
    w.add(aabb(
        v(fx(10), fx(0)),
        v(fx(0), fx(0)),
        fx(0),
        fx(1),
        v(f_rat(1, 2), fx(10)),
    ));
    // Bullet with r = 0.5, |v| * dt = 15 <= 32 * 0.5 = 16
    let bullet = w.add(circle(
        v(fx(0), fx(0)),
        v(fx(900), fx(0)),
        fx(1),
        fx(1),
        f_rat(1, 2),
    ));
    let dt = f_rat(1, 60);
    for _ in 0..10 {
        w.step(dt);
        let b = w.body(bullet).unwrap();
        assert!(b.pos.x < fx(10), "bullet tunnelled to x={:?}", b.pos.x);
    }
}

#[test]
fn settling_stack_of_five_boxes() {
    let mut w = World::new(v(fx(0), fx(-10)));
    // Static floor top at y = 0
    w.add(aabb(
        v(fx(0), fx(-1)),
        v(fx(0), fx(0)),
        fx(0),
        fx(0),
        v(fx(10), fx(1)),
    ));
    let mut boxes = Vec::new();
    for i in 0..5 {
        let id = w.add(aabb(
            v(fx(0), f_rat(1 + 2 * i, 2)),
            v(fx(0), fx(0)),
            fx(1),
            fx(0),
            v(f_rat(1, 2), f_rat(1, 2)),
        ));
        boxes.push(id);
    }
    let dt = f_rat(1, 120);
    for _ in 0..480 {
        w.step(dt);
    }
    let two_slop = f_rat(2, 100);
    let b0 = w.body(boxes[0]).unwrap();
    let sunk0 = f_rat(1, 2) - b0.pos.y;
    assert!(
        sunk0 < two_slop,
        "box 0 sunk by {sunk0:?} >= 2*slop ({two_slop:?})"
    );
    for (i, &id) in boxes.iter().enumerate() {
        let b = w.body(id).unwrap();
        assert!(
            b.vel.len() < f_rat(1, 2),
            "box {i} still moving: {:?}",
            b.vel
        );
    }
}

#[test]
fn settling_stack_at_60hz() {
    let mut w = World::new(v(fx(0), fx(-10)));
    w.add(aabb(
        v(fx(0), fx(-1)),
        v(fx(0), fx(0)),
        fx(0),
        fx(0),
        v(fx(20), fx(1)),
    ));
    let mut boxes = Vec::new();
    for i in 0..5 {
        let y = f_rat(55 + 105 * i, 100);
        let id = w.add(aabb(
            v(fx(0), y),
            v(fx(0), fx(0)),
            fx(1),
            fx(0),
            v(f_rat(1, 2), f_rat(1, 2)),
        ));
        boxes.push(id);
    }
    let dt = f_rat(1, 60);
    for _ in 0..600 {
        w.step(dt);
    }
    let two_slop = f_rat(2, 100);
    let mut below_top = fx(0);
    for (i, &id) in boxes.iter().enumerate() {
        let b = w.body(id).unwrap();
        let bottom = b.pos.y - f_rat(1, 2);
        let sunk = below_top - bottom;
        assert!(sunk < two_slop, "box {i} sunk {sunk:?} >= {two_slop:?}");
        assert!(
            b.vel.len() < f_rat(1, 10),
            "box {i} still moving: {:?}",
            b.vel
        );
        below_top = b.pos.y + f_rat(1, 2);
    }
}

#[test]
fn determinism_and_id_lifecycle() {
    let mut w1 = World::new(v(fx(0), fx(-10)));
    let mut w2 = World::new(v(fx(0), fx(-10)));
    let b_a = circle(v(fx(0), fx(3)), v(fx(1), fx(0)), fx(1), f_rat(4, 5), fx(1));
    let b_b = aabb(
        v(fx(0), fx(-1)),
        v(fx(0), fx(0)),
        fx(0),
        f_rat(4, 5),
        v(fx(5), fx(1)),
    );

    let id0 = w1.add(b_a);
    let id1 = w1.add(b_b);
    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert!(w1.remove(id0));
    assert!(!w1.remove(id0));
    let id2 = w1.add(b_a);
    assert_eq!(id2, 2);
    assert_eq!(w1.ids(), vec![1, 2]);

    let w2_0 = w2.add(b_a);
    w2.add(b_b);
    w2.remove(w2_0);
    w2.add(b_a);

    let dt = f_rat(1, 60);
    for _ in 0..60 {
        w1.step(dt);
        w2.step(dt);
    }
    assert_eq!(w1, w2);
    assert_eq!(w1.contacts(), w2.contacts());
}

#[test]
fn random_scenes_momentum_and_energy() {
    forall(25, 1, |rng| {
        let mut w = World::new(v(fx(0), fx(0)));
        let count = rng.range(2, 8);
        for i in 0..count {
            let px = f_rat(rng.range(0, 100) as i64 - 50, 10);
            let py = f_rat(rng.range(0, 100) as i64 - 50, 10);
            let vx = f_rat(rng.range(0, 60) as i64 - 30, 10);
            let vy = f_rat(rng.range(0, 60) as i64 - 30, 10);
            let inv_m = f_rat(rng.range(1, 4) as i64, 2);
            let e = f_rat(rng.range(0, 10) as i64, 10);
            let shape = if i % 2 == 0 {
                Shape::Circle {
                    r: f_rat(rng.range(5, 15) as i64, 10),
                }
            } else {
                Shape::Aabb {
                    half: v(
                        f_rat(rng.range(5, 15) as i64, 10),
                        f_rat(rng.range(5, 15) as i64, 10),
                    ),
                }
            };
            w.add(Body {
                pos: v(px, py),
                vel: v(vx, vy),
                inv_mass: inv_m,
                restitution: e,
                shape,
            });
        }

        let momentum = |world: &World| -> (i128, i128) {
            let mut mx = 0i128;
            let mut my = 0i128;
            for id in world.ids() {
                let b = world.body(id).unwrap();
                if b.inv_mass > Fx::ZERO {
                    let px = (i128::from(b.vel.x.0) << 16) / i128::from(b.inv_mass.0);
                    let py = (i128::from(b.vel.y.0) << 16) / i128::from(b.inv_mass.0);
                    mx += px;
                    my += py;
                }
            }
            (mx, my)
        };

        let m0 = momentum(&w);
        let ke0 = w.kinetic_energy();
        let dt = f_rat(1, 60);
        for _ in 0..30 {
            w.step(dt);
            let ke = w.kinetic_energy();
            // Energy never rises above start by more than 1% (plus tiny tolerance for near-zero KE)
            let limit = ke0 + ke0 / fx(100) + f_rat(1, 100);
            assert!(
                ke <= limit,
                "ke={ke:?} exceeded limit={limit:?} (ke0={ke0:?})"
            );
        }
        let m1 = momentum(&w);
        let tol = 1_000_000i128; // small fixed-point rounding across 30 steps
        assert!((m1.0 - m0.0).abs() <= tol, "mx drifted: {m0:?} -> {m1:?}");
        assert!((m1.1 - m0.1).abs() <= tol, "my drifted: {m0:?} -> {m1:?}");
    });
}
