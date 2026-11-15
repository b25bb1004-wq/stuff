//! Safe simulator: owns its arena and hands out checked handles, so callers never touch raw pointers.

use crate::heap::{class_of, class_size, large_order, order_bytes};
use crate::{Heap, PAGE};
use alloc::vec::Vec;
use core::alloc::Layout;
use core::sync::atomic::{AtomicU32, Ordering};

/// Source of per-Sim ids, so a handle from one Sim is foreign to every other.
static NEXT_SIM: AtomicU32 = AtomicU32::new(1);

/// Why a Sim call was refused. Refusals never panic and never change the heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimError {
    /// A zero-byte request, which the Sim does not hand out.
    ZeroSize,
    /// The arena could not be created, or the heap cannot fit the request.
    OutOfMemory,
    /// The handle was not issued by this Sim.
    Foreign,
    /// The handle's block was already freed.
    DoubleFree,
}

/// Opaque reference to one allocation made by a Sim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    sim: u32,
    index: usize,
    gen: u64,
}

/// Lifecycle of a block listed by `Sim::blocks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockState {
    /// Handed out and not yet freed.
    Live,
    /// Freed; its slot has not been reused yet.
    Freed,
}

/// One block the Sim has handed out, as reported by `Sim::block` and `Sim::blocks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// Byte offset of the block from the start of the arena.
    pub offset: usize,
    /// Bytes reserved for the block: the slab class size, or the buddy block size.
    pub size: usize,
    /// Buddy order backing the block; 0 for sub-page slab blocks.
    pub order: u32,
    /// Whether the block is still live or has been freed.
    pub state: BlockState,
}

/// Bookkeeping for one handle slot. Reused slots keep their generation, so stale handles stay detectable.
#[derive(Clone, Copy, Debug)]
struct Slot {
    gen: u64,
    live: bool,
    offset: usize,
    request: usize,
    reserved: usize,
    order: u32,
}

/// A simulated heap over an arena it owns, with generation-checked handles.
pub struct Sim {
    /// Zeroed, never resized after `new`, so the pointers the heap keeps into it stay valid.
    arena: Vec<u8>,
    heap: Heap,
    id: u32,
    slots: Vec<Slot>,
    free_slots: Vec<usize>,
}

impl Sim {
    /// A Sim with about `bytes` of usable arena. The arena is over-allocated by one page so the heap can align inside it.
    pub fn new(bytes: usize) -> Result<Sim, SimError> {
        let total = bytes.checked_add(PAGE).ok_or(SimError::OutOfMemory)?;
        let mut arena = Vec::new();
        arena
            .try_reserve_exact(total)
            .map_err(|_| SimError::OutOfMemory)?;
        arena.resize(total, 0);
        let (base, len) = (arena.as_mut_ptr(), arena.len());
        let mut heap = Heap::empty();
        // SAFETY: `arena` is owned by the Sim, zeroed, and never resized, moved out of or freed while the Sim lives, so `base..base + len` stays valid, writable and unused for the heap's whole life.
        if !unsafe { heap.init(base, len) } {
            return Err(SimError::OutOfMemory);
        }
        let id = NEXT_SIM.fetch_add(1, Ordering::Relaxed);
        Ok(Sim {
            arena,
            heap,
            id,
            slots: Vec::new(),
            free_slots: Vec::new(),
        })
    }

    /// Allocate `size` bytes, returning a handle, or an error if the request is zero-sized or does not fit.
    pub fn alloc(&mut self, size: usize) -> Result<Handle, SimError> {
        if size == 0 {
            return Err(SimError::ZeroSize);
        }
        let layout = Layout::from_size_align(size, 1).map_err(|_| SimError::OutOfMemory)?;
        let (reserved, order) = match class_of(size, 1) {
            Some(c) => (class_size(c), 0),
            None => {
                let order = large_order(layout).ok_or(SimError::OutOfMemory)?;
                (order_bytes(order), order)
            }
        };
        let base = self.arena.as_ptr() as usize;
        let p = self.heap.alloc(layout);
        let offset = match (p as usize).checked_sub(base) {
            Some(o) if !p.is_null() && o < self.arena.len() => o,
            _ => return Err(SimError::OutOfMemory),
        };
        let index = self.free_slots.last().copied().unwrap_or(self.slots.len());
        let gen = self.slots.get(index).map_or(0, |s| s.gen);
        let slot = Slot {
            gen,
            live: true,
            offset,
            request: size,
            reserved,
            order,
        };
        let index = match self.slots.get_mut(index) {
            Some(s) => {
                *s = slot;
                self.free_slots.pop();
                index
            }
            None => {
                self.slots.push(slot);
                self.slots.len() - 1
            }
        };
        Ok(Handle {
            sim: self.id,
            index,
            gen,
        })
    }

    /// Free the block behind `handle`. Handles this Sim did not issue give `Foreign`; handles already freed give `DoubleFree`.
    pub fn free(&mut self, handle: Handle) -> Result<(), SimError> {
        let index = self.resolve(handle)?;
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(SimError::Foreign);
        };
        let layout = Layout::from_size_align(slot.request, 1).map_err(|_| SimError::Foreign)?;
        let p = self.arena.as_mut_ptr().wrapping_add(slot.offset);
        // SAFETY: `resolve` proved this slot is live, so `p` is the pointer `Heap::alloc` returned for this exact layout and it has not been freed; the slot is marked freed below, so a second free is refused before it reaches here.
        unsafe { self.heap.dealloc(p, layout) };
        slot.live = false;
        slot.gen = slot.gen.wrapping_add(1);
        self.free_slots.push(index);
        Ok(())
    }

    /// The block behind a live handle, or the same errors `free` gives for a dead or foreign handle.
    pub fn block(&self, handle: Handle) -> Result<Block, SimError> {
        let index = self.resolve(handle)?;
        let s = self.slots.get(index).ok_or(SimError::Foreign)?;
        Ok(block_of(s))
    }

    /// Every block handed out and not yet reused, live or freed. A freed block stays listed until its slot is reused.
    pub fn blocks(&self) -> Vec<Block> {
        self.slots.iter().map(block_of).collect()
    }

    /// Usable bytes of the arena, after the heap's order table and alignment are taken out.
    pub fn total_bytes(&self) -> usize {
        self.heap.total_bytes()
    }

    /// Bytes handed out, rounded to slab class or page size.
    pub fn used_bytes(&self) -> usize {
        self.heap.used_bytes()
    }

    /// Verify the heap's free lists and counters; false if any invariant is broken.
    pub fn check(&self) -> bool {
        self.heap.check()
    }

    /// Slot index for a handle that is live and was issued by this Sim.
    fn resolve(&self, h: Handle) -> Result<usize, SimError> {
        if h.sim != self.id {
            return Err(SimError::Foreign);
        }
        let slot = self.slots.get(h.index).ok_or(SimError::Foreign)?;
        if h.gen < slot.gen {
            Err(SimError::DoubleFree)
        } else if h.gen > slot.gen || !slot.live {
            Err(SimError::Foreign)
        } else {
            Ok(h.index)
        }
    }
}

/// Public view of one slot.
fn block_of(s: &Slot) -> Block {
    Block {
        offset: s.offset,
        size: s.reserved,
        order: s.order,
        state: if s.live {
            BlockState::Live
        } else {
            BlockState::Freed
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn refuses_zero_size_and_empty_arena() {
        assert_eq!(Sim::new(0).err(), Some(SimError::OutOfMemory));
        let mut sim = Sim::new(1 << 20).unwrap();
        assert_eq!(sim.alloc(0), Err(SimError::ZeroSize));
    }

    #[test]
    fn double_and_foreign_free_are_errors() {
        let mut sim = Sim::new(1 << 20).unwrap();
        let mut other = Sim::new(1 << 20).unwrap();
        let h = sim.alloc(100).unwrap();
        let foreign = other.alloc(100).unwrap();
        assert_eq!(sim.free(foreign), Err(SimError::Foreign));
        assert_eq!(sim.free(h), Ok(()));
        assert_eq!(sim.free(h), Err(SimError::DoubleFree));
        assert_eq!(sim.block(h), Err(SimError::DoubleFree));
    }

    #[test]
    fn stale_handle_stays_dead_after_slot_reuse() {
        let mut sim = Sim::new(1 << 20).unwrap();
        let old = sim.alloc(64).unwrap();
        sim.free(old).unwrap();
        let new = sim.alloc(64).unwrap();
        assert_eq!(sim.free(old), Err(SimError::DoubleFree));
        assert_eq!(sim.block(new).unwrap().state, BlockState::Live);
        assert_eq!(sim.free(new), Ok(()));
        assert_eq!(sim.blocks()[0].state, BlockState::Freed);
        assert_eq!(sim.used_bytes(), 0);
        assert!(sim.check());
    }

    #[test]
    fn large_block_reports_order_and_alignment() {
        let mut sim = Sim::new(4 << 20).unwrap();
        let h = sim.alloc(3 * PAGE).unwrap();
        let b = sim.block(h).unwrap();
        assert_eq!((b.order, b.size), (2, 4 * PAGE));
        assert_eq!(sim.free(h), Ok(()));
    }
}
