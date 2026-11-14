//! Entities, typed component storage and a deterministic world hash.
//! Iteration is by ascending slot index, so results never depend on hashing, addresses or capacity.
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::any::Any;
use core::hash::{Hash, Hasher};

mod fnv;
#[cfg(test)]
mod tests;

use fnv::Fnv;

/// Handle to an entity: a slot index and the generation of that slot when the handle was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    /// Slot index; reused after despawn.
    pub index: u32,
    /// Generation of the slot when this handle was made.
    pub generation: u32,
}

/// Anything storable as a component.
pub trait Component: Any + Clone + Hash {}

impl<T: Any + Clone + Hash> Component for T {}

/// Type-erased storage for one component type.
trait Column {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn remove_at(&mut self, index: u32);
    fn hash_at(&self, index: u32, h: &mut Fnv);
}

/// Components of one type, keyed by slot index.
struct Typed<T> {
    items: BTreeMap<u32, T>,
}

impl<T: Component> Column for Typed<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn remove_at(&mut self, index: u32) {
        self.items.remove(&index);
    }

    fn hash_at(&self, index: u32, h: &mut Fnv) {
        match self.items.get(&index) {
            Some(v) => {
                h.write_u8(1);
                v.hash(h);
            }
            None => h.write_u8(0),
        }
    }
}

/// Position of the column for `T` in `columns`, if any.
fn find<T: Component>(columns: &[Box<dyn Column>]) -> Option<usize> {
    columns.iter().position(|c| c.as_any().is::<Typed<T>>())
}

/// Entities and their components. Despawn removes all components of the entity.
#[derive(Default)]
pub struct World {
    gens: Vec<u32>,
    live: Vec<bool>,
    free: BTreeSet<u32>,
    columns: Vec<Box<dyn Column>>,
}

impl World {
    /// Empty world.
    pub fn new() -> World {
        World::default()
    }

    /// New entity. Reuses the lowest free index with generation + 1, or appends a new index at generation 0.
    pub fn spawn(&mut self) -> Entity {
        if let Some(index) = self.free.iter().next().copied() {
            self.free.remove(&index);
            let i = index as usize;
            let generation = match self.gens.get_mut(i) {
                Some(g) => {
                    *g = g.wrapping_add(1);
                    *g
                }
                None => 0,
            };
            if let Some(l) = self.live.get_mut(i) {
                *l = true;
            }
            Entity { index, generation }
        } else {
            // shortcut: more than 2^32 slots would wrap the index; not reachable in practice
            let index = self.gens.len() as u32;
            self.gens.push(0);
            self.live.push(true);
            Entity {
                index,
                generation: 0,
            }
        }
    }

    /// Remove an entity and all its components. False if it is already dead or the handle is stale.
    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.alive(e) {
            return false;
        }
        if let Some(l) = self.live.get_mut(e.index as usize) {
            *l = false;
        }
        self.free.insert(e.index);
        for col in self.columns.iter_mut() {
            col.remove_at(e.index);
        }
        true
    }

    /// True if `e` names a live entity with its current generation.
    pub fn alive(&self, e: Entity) -> bool {
        let i = e.index as usize;
        self.live.get(i).copied().unwrap_or(false) && self.gens.get(i) == Some(&e.generation)
    }

    /// Give `e` a component of type `T`, replacing any existing one. False for a dead entity.
    pub fn insert<T: Component>(&mut self, e: Entity, c: T) -> bool {
        if !self.alive(e) {
            return false;
        }
        let p = match find::<T>(&self.columns) {
            Some(p) => p,
            None => {
                self.columns.push(Box::new(Typed::<T> {
                    items: BTreeMap::new(),
                }));
                self.columns.len() - 1
            }
        };
        match self
            .columns
            .get_mut(p)
            .and_then(|col| col.as_any_mut().downcast_mut::<Typed<T>>())
        {
            Some(col) => {
                col.items.insert(e.index, c);
                true
            }
            None => false,
        }
    }

    /// The `T` component of `e`, if alive and present.
    pub fn get<T: Component>(&self, e: Entity) -> Option<&T> {
        if !self.alive(e) {
            return None;
        }
        let p = find::<T>(&self.columns)?;
        self.columns
            .get(p)?
            .as_any()
            .downcast_ref::<Typed<T>>()?
            .items
            .get(&e.index)
    }

    /// Mutable access to the `T` component of `e`, if alive and present.
    pub fn get_mut<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        if !self.alive(e) {
            return None;
        }
        let p = find::<T>(&self.columns)?;
        self.columns
            .get_mut(p)?
            .as_any_mut()
            .downcast_mut::<Typed<T>>()?
            .items
            .get_mut(&e.index)
    }

    /// Take the `T` component off `e`, returning it.
    pub fn remove<T: Component>(&mut self, e: Entity) -> Option<T> {
        if !self.alive(e) {
            return None;
        }
        let p = find::<T>(&self.columns)?;
        self.columns
            .get_mut(p)?
            .as_any_mut()
            .downcast_mut::<Typed<T>>()?
            .items
            .remove(&e.index)
    }

    /// Visit every entity holding an `A`, in ascending index order.
    pub fn each<A: Component>(&mut self, mut f: impl FnMut(Entity, &mut A)) {
        let World { gens, columns, .. } = self;
        let Some(p) = find::<A>(columns.as_slice()) else {
            return;
        };
        let Some(a) = columns
            .get_mut(p)
            .and_then(|col| col.as_any_mut().downcast_mut::<Typed<A>>())
        else {
            return;
        };
        for (&index, va) in a.items.iter_mut() {
            let generation = gens.get(index as usize).copied().unwrap_or(0);
            f(Entity { index, generation }, va);
        }
    }

    /// Visit every entity holding both an `A` and a `B`, in ascending index order. `A` and `B` must differ.
    pub fn each2<A: Component, B: Component>(&mut self, mut f: impl FnMut(Entity, &mut A, &mut B)) {
        let World { gens, columns, .. } = self;
        let (Some(pa), Some(pb)) = (find::<A>(columns.as_slice()), find::<B>(columns.as_slice()))
        else {
            return;
        };
        if pa == pb {
            return;
        }
        let (mut ca, mut cb) = (None, None);
        for (i, col) in columns.iter_mut().enumerate() {
            if i == pa {
                ca = Some(col);
            } else if i == pb {
                cb = Some(col);
            }
        }
        let (Some(ca), Some(cb)) = (ca, cb) else {
            return;
        };
        let (Some(a), Some(b)) = (
            ca.as_any_mut().downcast_mut::<Typed<A>>(),
            cb.as_any_mut().downcast_mut::<Typed<B>>(),
        ) else {
            return;
        };
        for (&index, va) in a.items.iter_mut() {
            let Some(vb) = b.items.get_mut(&index) else {
                continue;
            };
            let generation = gens.get(index as usize).copied().unwrap_or(0);
            f(Entity { index, generation }, va, vb);
        }
    }

    /// Live entities in ascending index order.
    pub fn entities(&self) -> Vec<Entity> {
        let mut out = Vec::new();
        for (i, (generation, live)) in self.gens.iter().zip(self.live.iter()).enumerate() {
            if *live {
                out.push(Entity {
                    index: i as u32,
                    generation: *generation,
                });
            }
        }
        out
    }

    /// FNV-1a over every live entity: index, generation, then each component type in first-insertion order (presence byte, then value hash).
    pub fn hash(&self) -> u64 {
        let mut h = Fnv::new();
        for (i, (generation, live)) in self.gens.iter().zip(self.live.iter()).enumerate() {
            if !*live {
                continue;
            }
            let index = i as u32;
            h.write_u32(index);
            h.write_u32(*generation);
            for col in &self.columns {
                col.hash_at(index, &mut h);
            }
        }
        h.finish()
    }
}
