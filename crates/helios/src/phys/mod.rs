//! Fixed-point 2-D impulse physics: shapes, bodies, collision detection and world stepping.

mod collide;
mod world;

use crate::fx::{Fx, Vec2};

pub use collide::collide;
pub use world::World;

/// Collision shape attached to a body (axis-aligned, no rotation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Circle with radius `r`.
    Circle {
        /// Circle radius in world units.
        r: Fx,
    },
    /// Axis-aligned bounding box with half-extents `half`.
    Aabb {
        /// Half-width and half-height in world units.
        half: Vec2,
    },
}

/// Rigid body in the 2-D physics world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    /// World-space centre position.
    pub pos: Vec2,
    /// Linear velocity.
    pub vel: Vec2,
    /// Inverse mass (`0` means static / infinite mass).
    pub inv_mass: Fx,
    /// Coefficient of restitution (typically in `0..=1`).
    pub restitution: Fx,
    /// Collision shape.
    pub shape: Shape,
}

/// Opaque handle to a body in a [`World`].
pub type BodyId = usize;

/// Overlapping pair detected during a physics step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    /// Lower body id in the overlapping pair.
    pub a: BodyId,
    /// Higher body id in the overlapping pair.
    pub b: BodyId,
    /// Unit contact normal pointing from `a` to `b`.
    pub normal: Vec2,
    /// Penetration depth (`> 0`).
    pub depth: Fx,
}
