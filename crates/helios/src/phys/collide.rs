//! Narrow-phase collision detection between circles and axis-aligned boxes.

use super::{Body, Shape};
use crate::fx::{Fx, Vec2};

/// Test whether `a` and `b` overlap; returns `(unit normal from a to b, depth > 0)` if so.
pub fn collide(a: &Body, b: &Body) -> Option<(Vec2, Fx)> {
    match (a.shape, b.shape) {
        (Shape::Circle { r: ra }, Shape::Circle { r: rb }) => {
            collide_circle_circle(a.pos, ra, b.pos, rb)
        }
        (Shape::Aabb { half: ha }, Shape::Aabb { half: hb }) => {
            collide_aabb_aabb(a.pos, ha, b.pos, hb)
        }
        (Shape::Circle { r }, Shape::Aabb { half }) => collide_circle_aabb(a.pos, r, b.pos, half),
        (Shape::Aabb { half }, Shape::Circle { r }) => {
            let (n, d) = collide_circle_aabb(b.pos, r, a.pos, half)?;
            Some((-n, d))
        }
    }
}

/// Circle-circle overlap test; coincident centres use normal `(1, 0)`.
fn collide_circle_circle(pa: Vec2, ra: Fx, pb: Vec2, rb: Fx) -> Option<(Vec2, Fx)> {
    let sum_r = ra.max(Fx::ZERO) + rb.max(Fx::ZERO);
    if sum_r <= Fx::ZERO {
        return None;
    }
    let d = pb - pa;
    if d.x == Fx::ZERO && d.y == Fx::ZERO {
        return Some((Vec2::new(Fx::ONE, Fx::ZERO), sum_r));
    }
    let dist = d.len();
    if dist >= sum_r {
        return None;
    }
    let depth = sum_r - dist;
    if depth <= Fx::ZERO {
        return None;
    }
    Some((d.normalized(), depth))
}

/// AABB-AABB overlap test along the axis of least penetration (ties pick X).
fn collide_aabb_aabb(pa: Vec2, ha: Vec2, pb: Vec2, hb: Vec2) -> Option<(Vec2, Fx)> {
    let hax = ha.x.max(Fx::ZERO);
    let hay = ha.y.max(Fx::ZERO);
    let hbx = hb.x.max(Fx::ZERO);
    let hby = hb.y.max(Fx::ZERO);

    let d = pb - pa;
    let ox = (hax + hbx) - d.x.abs();
    if ox <= Fx::ZERO {
        return None;
    }
    let oy = (hay + hby) - d.y.abs();
    if oy <= Fx::ZERO {
        return None;
    }

    if ox <= oy {
        let nx = if d.x >= Fx::ZERO { Fx::ONE } else { -Fx::ONE };
        Some((Vec2::new(nx, Fx::ZERO), ox))
    } else {
        let ny = if d.y >= Fx::ZERO { Fx::ONE } else { -Fx::ONE };
        Some((Vec2::new(Fx::ZERO, ny), oy))
    }
}

/// Circle vs AABB overlap test; normal points from the circle (`a`) to the box (`b`).
fn collide_circle_aabb(pc: Vec2, r: Fx, pb: Vec2, hb: Vec2) -> Option<(Vec2, Fx)> {
    let r = r.max(Fx::ZERO);
    let hx = hb.x.max(Fx::ZERO);
    let hy = hb.y.max(Fx::ZERO);

    let diff = pc - pb;
    let closest = Vec2::new(diff.x.clamp(-hx, hx), diff.y.clamp(-hy, hy));
    let delta = diff - closest;

    if delta.x != Fx::ZERO || delta.y != Fx::ZERO {
        let dist = delta.len();
        if dist >= r {
            return None;
        }
        let depth = r - dist;
        if depth <= Fx::ZERO {
            return None;
        }
        Some(((-delta).normalized(), depth))
    } else {
        let px = hx - diff.x.abs();
        let py = hy - diff.y.abs();
        if px <= py {
            let depth = r + px;
            if depth <= Fx::ZERO {
                return None;
            }
            let nx = if diff.x >= Fx::ZERO {
                -Fx::ONE
            } else {
                Fx::ONE
            };
            Some((Vec2::new(nx, Fx::ZERO), depth))
        } else {
            let depth = r + py;
            if depth <= Fx::ZERO {
                return None;
            }
            let ny = if diff.y >= Fx::ZERO {
                -Fx::ONE
            } else {
                Fx::ONE
            };
            Some((Vec2::new(Fx::ZERO, ny), depth))
        }
    }
}
