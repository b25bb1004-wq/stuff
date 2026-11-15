//! 2-D rigid-body world with substepped impulse resolution and positional correction.

use alloc::vec::Vec;

use super::{collide, Body, BodyId, Contact, Shape};
use crate::fx::{Fx, Vec2};

/// Maximum number of substeps per `World::step` call.
const MAX_SUBSTEPS: usize = 64;

/// 2-D impulse-based physics world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    /// Constant gravitational acceleration applied to dynamic bodies.
    pub gravity: Vec2,
    bodies: Vec<Option<Body>>,
    contacts: Vec<Contact>,
}

impl World {
    /// Create an empty world with the given `gravity`.
    pub fn new(gravity: Vec2) -> World {
        World {
            gravity,
            bodies: Vec::new(),
            contacts: Vec::new(),
        }
    }

    /// Add `b` to the world and return its unique `BodyId` (ids are never reused).
    pub fn add(&mut self, b: Body) -> BodyId {
        let id = self.bodies.len();
        self.bodies.push(Some(b));
        id
    }

    /// Remove the body with `id`, returning `true` if it was present.
    pub fn remove(&mut self, id: BodyId) -> bool {
        if let Some(slot) = self.bodies.get_mut(id) {
            if slot.is_some() {
                *slot = None;
                return true;
            }
        }
        false
    }

    /// Borrow a live body by `id`.
    pub fn body(&self, id: BodyId) -> Option<&Body> {
        self.bodies.get(id).and_then(|s| s.as_ref())
    }

    /// Mutably borrow a live body by `id`.
    pub fn body_mut(&mut self, id: BodyId) -> Option<&mut Body> {
        self.bodies.get_mut(id).and_then(|s| s.as_mut())
    }

    /// Return all live body ids in ascending order.
    pub fn ids(&self) -> Vec<BodyId> {
        self.bodies
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|_| i))
            .collect()
    }

    /// Advance the simulation by `dt` using adaptive substepping.
    pub fn step(&mut self, dt: Fx) {
        if dt <= Fx::ZERO {
            self.contacts.clear();
            return;
        }

        // Apply gravity to dynamic bodies for the full step.
        let dv = self.gravity.scale(dt);
        for b in self.bodies.iter_mut().flatten() {
            if b.inv_mass > Fx::ZERO {
                b.vel = b.vel + dv;
            }
        }

        let n = self.substep_count(dt);
        let sdt = dt / Fx::from_int(n as i32);
        let live = self.ids();

        let slop = Fx::from_ratio(1, 100);
        let percent = Fx::from_ratio(4, 5);

        for _ in 0..n {
            for &id in &live {
                if let Some(Some(b)) = self.bodies.get_mut(id) {
                    if b.inv_mass > Fx::ZERO {
                        b.pos = b.pos + b.vel.scale(sdt);
                    }
                }
            }

            self.contacts.clear();
            for (idx_a, &a_id) in live.iter().enumerate() {
                let Some(a) = self.body(a_id).copied() else {
                    continue;
                };
                for &b_id in live.iter().skip(idx_a + 1) {
                    let Some(b) = self.body(b_id).copied() else {
                        continue;
                    };
                    if a.inv_mass <= Fx::ZERO && b.inv_mass <= Fx::ZERO {
                        continue;
                    }
                    if let Some((normal, depth)) = collide(&a, &b) {
                        self.contacts.push(Contact {
                            a: a_id,
                            b: b_id,
                            normal,
                            depth,
                        });
                    }
                }
            }

            for _iter in 0..64 {
                let mut any = false;
                for i in 0..self.contacts.len() {
                    let c = self.contacts[i];
                    let (Some(a), Some(b)) = (self.body(c.a).copied(), self.body(c.b).copied())
                    else {
                        continue;
                    };
                    let inv_a = a.inv_mass.max(Fx::ZERO);
                    let inv_b = b.inv_mass.max(Fx::ZERO);
                    let inv_sum = inv_a + inv_b;
                    if inv_sum <= Fx::ZERO {
                        continue;
                    }

                    let vrel = b.vel - a.vel;
                    let vn = vrel.dot(c.normal);
                    if vn < Fx::ZERO {
                        any = true;
                        let e = a.restitution.min(b.restitution).clamp(Fx::ZERO, Fx::ONE);
                        let j = -((Fx::ONE + e) * vn) / inv_sum;
                        let impulse = c.normal.scale(j);
                        if inv_a > Fx::ZERO {
                            if let Some(body_a) = self.body_mut(c.a) {
                                body_a.vel = body_a.vel - impulse.scale(inv_a);
                            }
                        }
                        if inv_b > Fx::ZERO {
                            if let Some(body_b) = self.body_mut(c.b) {
                                body_b.vel = body_b.vel + impulse.scale(inv_b);
                            }
                        }
                    }
                }
                if !any {
                    break;
                }
            }

            for iter in 0..32 {
                let mut any = false;
                let len = self.contacts.len();
                for step_i in 0..len {
                    let i = if iter % 2 == 0 {
                        step_i
                    } else {
                        len - 1 - step_i
                    };
                    let c = self.contacts[i];
                    let (Some(a), Some(b)) = (self.body(c.a).copied(), self.body(c.b).copied())
                    else {
                        continue;
                    };
                    let inv_a = a.inv_mass.max(Fx::ZERO);
                    let inv_b = b.inv_mass.max(Fx::ZERO);
                    let inv_sum = inv_a + inv_b;
                    if inv_sum <= Fx::ZERO {
                        continue;
                    }

                    let Some((normal, depth)) = collide(&a, &b) else {
                        continue;
                    };
                    let corr_depth = (depth - slop).max(Fx::ZERO);
                    if corr_depth > Fx::ZERO {
                        any = true;
                        let corr_mag = (corr_depth * percent) / inv_sum;
                        let corr = normal.scale(corr_mag);
                        if inv_a > Fx::ZERO {
                            if let Some(body_a) = self.body_mut(c.a) {
                                body_a.pos = body_a.pos - corr.scale(inv_a);
                            }
                        }
                        if inv_b > Fx::ZERO {
                            if let Some(body_b) = self.body_mut(c.b) {
                                body_b.pos = body_b.pos + corr.scale(inv_b);
                            }
                        }
                    }
                }
                if !any {
                    break;
                }
            }
        }
    }

    /// Contacts found in the last substep of the most recent `step`.
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// Total kinetic energy `sum(v.v / (2 * inv_mass))` over all dynamic bodies.
    pub fn kinetic_energy(&self) -> Fx {
        let mut total = Fx::ZERO;
        for b in self.bodies.iter().flatten() {
            if b.inv_mass > Fx::ZERO {
                let two_inv_m = b.inv_mass + b.inv_mass;
                total = total + b.vel.dot(b.vel) / two_inv_m;
            }
        }
        total
    }

    /// Smallest substep count in `1..=64` so no dynamic body moves more than half its smallest extent.
    fn substep_count(&self, dt: Fx) -> usize {
        let mut n = 1usize;
        for slot in &self.bodies {
            let Some(b) = slot else { continue };
            if b.inv_mass <= Fx::ZERO {
                continue;
            }
            let move_dist = b.vel.len() * dt;
            if move_dist <= Fx::ZERO {
                continue;
            }
            let half_ext = Fx(smallest_extent(b.shape).0 >> 1);
            if half_ext <= Fx::ZERO {
                return MAX_SUBSTEPS;
            }
            let m = move_dist.0 as u128;
            let h = half_ext.0 as u128;
            let need = m.div_ceil(h);
            if need >= MAX_SUBSTEPS as u128 {
                return MAX_SUBSTEPS;
            }
            n = n.max(need as usize);
        }
        n
    }
}

/// Smallest half-extent of a shape (radius for circles, min half-size for boxes).
fn smallest_extent(shape: Shape) -> Fx {
    match shape {
        Shape::Circle { r } => r.max(Fx::ZERO),
        Shape::Aabb { half } => half.x.min(half.y).max(Fx::ZERO),
    }
}
