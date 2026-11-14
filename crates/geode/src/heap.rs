//! The garbage-collected object heap: slot table, LIFO free list, and iterative mark-sweep.

use crate::value::{Handle, Obj, Trap, Value};
use alloc::vec::Vec;

/// Minimum allocation threshold (1 MiB) before triggering a periodic collection.
const MIN_GC_THRESHOLD: usize = 1024 * 1024;
/// Default maximum heap size (64 MiB).
pub(crate) const DEFAULT_MAX_HEAP_BYTES: usize = 64 * 1024 * 1024;

/// Compute the accounting size of a heap object: 16 bytes header plus payload.
pub(crate) fn obj_size(obj: &Obj) -> usize {
    let payload = match obj {
        Obj::Str(s) => s.len(),
        Obj::Bytes(b) => b.len(),
        Obj::Array(v) | Obj::Record(v) | Obj::Variant { fields: v, .. } => {
            v.len().saturating_mul(16)
        }
    };
    16usize.saturating_add(payload)
}

#[derive(Debug, Clone)]
enum Slot {
    Free { next_free: Option<u32> },
    Live { obj: Obj, size: usize, marked: bool },
}

/// The VM object heap, managed by a mark-and-sweep collector over a slot table.
#[derive(Debug, Clone)]
pub struct Heap {
    slots: Vec<Slot>,
    free_head: Option<u32>,
    live_objects: usize,
    live_bytes: usize,
    max_heap_bytes: usize,
    bytes_since_gc: usize,
    live_after_last_gc: usize,
    dirty_slot: Option<u32>,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_HEAP_BYTES)
    }
}

impl Heap {
    /// Create an empty heap with the given byte limit.
    pub(crate) fn new(max_heap_bytes: usize) -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            live_objects: 0,
            live_bytes: 0,
            max_heap_bytes,
            bytes_since_gc: 0,
            live_after_last_gc: 0,
            dirty_slot: None,
        }
    }

    /// Update the heap byte limit for a run.
    pub(crate) fn set_max_bytes(&mut self, max_heap_bytes: usize) {
        self.sync_dirty();
        self.max_heap_bytes = max_heap_bytes;
    }

    /// Reconcile `live_bytes` if a caller mutated an object via `get_mut`.
    pub(crate) fn sync_dirty(&mut self) {
        if let Some(idx) = self.dirty_slot.take() {
            if let Some(Slot::Live { obj, size, .. }) = self.slots.get_mut(idx as usize) {
                let new_size = obj_size(obj);
                if new_size >= *size {
                    let diff = new_size - *size;
                    self.live_bytes = self.live_bytes.saturating_add(diff);
                    self.bytes_since_gc = self.bytes_since_gc.saturating_add(diff);
                } else {
                    let diff = *size - new_size;
                    self.live_bytes = self.live_bytes.saturating_sub(diff);
                }
                *size = new_size;
            }
        }
    }

    /// Allocate `obj` on the heap, returning `OutOfMemory` if over the limit without collecting.
    pub fn alloc(&mut self, obj: Obj) -> Result<Handle, Trap> {
        self.sync_dirty();
        let size = obj_size(&obj);
        if self
            .live_bytes
            .checked_add(size)
            .is_none_or(|total| total > self.max_heap_bytes)
        {
            return Err(Trap::OutOfMemory);
        }
        self.alloc_with_size(obj, size)
    }

    /// Borrow the object referenced by `h`, or trap with `TypeError` if `h` is free or out of range.
    pub fn get(&self, h: Handle) -> Result<&Obj, Trap> {
        match self.slots.get(h.0 as usize) {
            Some(Slot::Live { obj, .. }) => Ok(obj),
            _ => Err(Trap::TypeError),
        }
    }

    /// Mutably borrow the object referenced by `h`, or trap with `TypeError` if `h` is free or out of range.
    pub fn get_mut(&mut self, h: Handle) -> Result<&mut Obj, Trap> {
        self.sync_dirty();
        match self.slots.get_mut(h.0 as usize) {
            Some(Slot::Live { obj, .. }) => {
                self.dirty_slot = Some(h.0);
                Ok(obj)
            }
            _ => Err(Trap::TypeError),
        }
    }

    /// Number of currently live objects on the heap.
    pub fn live_objects(&self) -> usize {
        self.live_objects
    }

    /// Total accounting bytes of all currently live objects on the heap.
    pub fn live_bytes(&self) -> usize {
        if let Some(idx) = self.dirty_slot {
            if let Some(Slot::Live { obj, size, .. }) = self.slots.get(idx as usize) {
                let new_size = obj_size(obj);
                if new_size >= *size {
                    return self.live_bytes.saturating_add(new_size - *size);
                } else {
                    return self.live_bytes.saturating_sub(*size - new_size);
                }
            }
        }
        self.live_bytes
    }

    /// Ensure `need` additional bytes can fit within `max_heap_bytes`, collecting at most once if needed.
    pub(crate) fn ensure_space(
        &mut self,
        need: usize,
        roots: &[Value],
        extra: Option<Value>,
    ) -> Result<(), Trap> {
        self.sync_dirty();
        let threshold = MIN_GC_THRESHOLD.max(self.live_after_last_gc);
        let over_threshold = self
            .bytes_since_gc
            .checked_add(need)
            .is_none_or(|b| b > threshold);
        let over_limit = self
            .live_bytes
            .checked_add(need)
            .is_none_or(|b| b > self.max_heap_bytes);

        if over_threshold || over_limit {
            self.collect(roots, extra);
        }
        if self
            .live_bytes
            .checked_add(need)
            .is_none_or(|b| b > self.max_heap_bytes)
        {
            return Err(Trap::OutOfMemory);
        }
        Ok(())
    }

    /// Insert an object whose size has already been checked by `ensure_space`.
    pub(crate) fn alloc_with_size(&mut self, obj: Obj, size: usize) -> Result<Handle, Trap> {
        let idx = if let Some(free_idx) = self.free_head {
            let next = match self.slots.get(free_idx as usize) {
                Some(Slot::Free { next_free }) => *next_free,
                _ => None,
            };
            self.free_head = next;
            if let Some(slot) = self.slots.get_mut(free_idx as usize) {
                *slot = Slot::Live {
                    obj,
                    size,
                    marked: false,
                };
            }
            free_idx
        } else {
            let next_idx = u32::try_from(self.slots.len()).map_err(|_| Trap::OutOfMemory)?;
            self.slots.push(Slot::Live {
                obj,
                size,
                marked: false,
            });
            next_idx
        };
        self.live_objects = self.live_objects.saturating_add(1);
        self.live_bytes = self.live_bytes.saturating_add(size);
        self.bytes_since_gc = self.bytes_since_gc.saturating_add(size);
        Ok(Handle(idx))
    }

    /// Run a mark-and-sweep collection rooted at `roots` and `extra`.
    pub(crate) fn collect(&mut self, roots: &[Value], extra: Option<Value>) {
        self.sync_dirty();
        let mut worklist: Vec<u32> = Vec::new();

        for &val in roots {
            Self::mark_val(val, &mut self.slots, &mut worklist);
        }
        if let Some(val) = extra {
            Self::mark_val(val, &mut self.slots, &mut worklist);
        }

        while let Some(idx) = worklist.pop() {
            let len = match self.slots.get(idx as usize) {
                Some(Slot::Live {
                    obj: Obj::Array(v) | Obj::Record(v) | Obj::Variant { fields: v, .. },
                    ..
                }) => v.len(),
                _ => 0,
            };
            for i in 0..len {
                let child = match self.slots.get(idx as usize) {
                    Some(Slot::Live {
                        obj: Obj::Array(v) | Obj::Record(v) | Obj::Variant { fields: v, .. },
                        ..
                    }) => v.get(i).copied().unwrap_or(Value::Unit),
                    _ => Value::Unit,
                };
                Self::mark_val(child, &mut self.slots, &mut worklist);
            }
        }

        for idx in 0..self.slots.len() {
            let free_it = match self.slots.get_mut(idx) {
                Some(Slot::Live {
                    obj,
                    size,
                    marked: true,
                }) => {
                    let actual = obj_size(obj);
                    *size = actual;
                    false
                }
                Some(Slot::Live {
                    size,
                    marked: false,
                    ..
                }) => {
                    self.live_objects = self.live_objects.saturating_sub(1);
                    self.live_bytes = self.live_bytes.saturating_sub(*size);
                    true
                }
                _ => false,
            };
            if free_it {
                if let Ok(idx_u32) = u32::try_from(idx) {
                    if let Some(slot) = self.slots.get_mut(idx) {
                        *slot = Slot::Free {
                            next_free: self.free_head,
                        };
                        self.free_head = Some(idx_u32);
                    }
                }
            } else if let Some(Slot::Live { marked, .. }) = self.slots.get_mut(idx) {
                *marked = false;
            }
        }

        self.bytes_since_gc = 0;
        self.live_after_last_gc = self.live_bytes;
    }

    fn mark_val(val: Value, slots: &mut [Slot], worklist: &mut Vec<u32>) {
        if let Value::Ref(Handle(idx)) = val {
            if let Some(Slot::Live { marked, .. }) = slots.get_mut(idx as usize) {
                if !*marked {
                    *marked = true;
                    worklist.push(idx);
                }
            }
        }
    }
}
