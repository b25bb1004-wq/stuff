//! Spin lock that lets a `Heap` serve as the global allocator.

use crate::heap::Heap;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// A value behind a spin lock.
pub struct Locked<T> {
    flag: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: `flag` serialises every access to `value`, so the lock may be shared across threads whenever `T` may be sent between them.
unsafe impl<T: Send> Sync for Locked<T> {}

impl<T> Locked<T> {
    /// Wrap `value` behind an unlocked spin lock.
    pub const fn new(value: T) -> Self {
        Locked {
            flag: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// Spin until the lock is free, then return a guard that releases it on drop.
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        while self
            .flag
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.flag.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        Guard { lock: self }
    }
}

/// Proof that the lock is held; releases it when dropped.
struct Guard<'a, T> {
    lock: &'a Locked<T>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the guard exists only while the lock is held, so no other reference to the value is live.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the guard exists only while the lock is held, so this is the only live reference.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.flag.store(false, Ordering::Release);
    }
}

// SAFETY: each call holds the lock for its whole duration, so the heap sees one caller at a time, and the heap upholds the GlobalAlloc contract.
unsafe impl GlobalAlloc for Locked<Heap> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = self.lock();
        heap.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut heap = self.lock();
        // SAFETY: GlobalAlloc's contract says `ptr` came from `alloc` with this layout, which is the Heap's contract.
        unsafe { heap.dealloc(ptr, layout) };
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::testutil::{churn, heap_over};
    use assay::Rng;

    const ARENA: usize = 16 << 20;

    #[test]
    fn locked_heap_is_thread_safe() {
        let mut arena = vec![0u8; ARENA];
        let locked = Locked::new(heap_over(&mut arena));
        let shared = &locked;
        std::thread::scope(|s| {
            for t in 0..4u64 {
                s.spawn(move || {
                    let mut api = crate::testutil::Shared(shared);
                    churn(&mut api, &mut Rng::new(t + 1), 5000, 200);
                });
            }
        });
        assert!(locked.lock().check());
        assert_eq!(locked.lock().used_bytes(), 0);
    }
}
