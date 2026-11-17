//! Bump global allocator over `sbrk`. Frees nothing; user programs are single-threaded.
use super::calls::sbrk;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;

const MIN_GROW: usize = 64 * 1024;

struct State {
    pos: usize,
    limit: usize,
}

/// Global allocator for titan programs.
pub struct Heap {
    state: UnsafeCell<State>,
}

impl Heap {
    /// Creates an empty heap; the first allocation asks the kernel for the break.
    pub const fn new() -> Self {
        Self {
            state: UnsafeCell::new(State { pos: 0, limit: 0 }),
        }
    }
}

impl State {
    fn take(&mut self, layout: Layout) -> *mut u8 {
        if self.limit == 0 {
            let base = sbrk(0);
            if base < 0 {
                return null_mut();
            }
            self.pos = base as usize;
            self.limit = base as usize;
        }
        let align = layout.align().max(8);
        let Some(aligned) = self.pos.checked_add(align - 1).map(|v| v & !(align - 1)) else {
            return null_mut();
        };
        let Some(end) = aligned.checked_add(layout.size()) else {
            return null_mut();
        };
        if end > self.limit {
            let grow = (end - self.limit).max(MIN_GROW);
            let Ok(delta) = isize::try_from(grow) else {
                return null_mut();
            };
            if sbrk(delta) < 0 {
                return null_mut();
            }
            self.limit += grow;
        }
        self.pos = end;
        aligned as *mut u8
    }
}

// SAFETY: a titan user program runs one thread, so `state` is never accessed concurrently.
unsafe impl Sync for Heap {}

// SAFETY: returned blocks are aligned, lie inside `[pos, limit)` of memory grown by `sbrk`,
// and are never handed out twice because `pos` only moves forward.
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: single-threaded, and no reference to `state` outlives this call.
        let state = unsafe { &mut *self.state.get() };
        state.take(layout)
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
