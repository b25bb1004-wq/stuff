//! Test-only helpers: arena builders and a churn driver shared by the heap and locked-heap tests.
#![allow(clippy::unwrap_used)]

use crate::{Buddy, Heap, Locked};
use core::alloc::{GlobalAlloc, Layout};
use strata_testkit::Rng;

/// Anything that hands out and takes back blocks, so one driver tests both heap flavours.
pub trait Api {
    /// Allocate `layout`; null on failure.
    fn alloc(&mut self, layout: Layout) -> *mut u8;
    /// Free a block from `alloc` with the same layout.
    ///
    /// # Safety
    /// `ptr` must have come from `alloc` with `layout` and must not be freed yet.
    unsafe fn free(&mut self, ptr: *mut u8, layout: Layout);
    /// True when the invariant check passes.
    fn ok(&self) -> bool;
}

impl Api for Heap {
    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        Heap::alloc(self, layout)
    }
    unsafe fn free(&mut self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded from the caller of `Api::free`, which promises the same contract.
        unsafe { Heap::dealloc(self, ptr, layout) }
    }
    fn ok(&self) -> bool {
        self.check()
    }
}

/// The `Locked<Heap>` seen through the `GlobalAlloc` trait.
pub struct Shared<'a>(pub &'a Locked<Heap>);

impl Api for Shared<'_> {
    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        // SAFETY: the layout is valid by construction, and GlobalAlloc::alloc has no other precondition.
        unsafe { GlobalAlloc::alloc(self.0, layout) }
    }
    unsafe fn free(&mut self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded from the caller of `Api::free`, which promises the same contract.
        unsafe { GlobalAlloc::dealloc(self.0, ptr, layout) }
    }
    fn ok(&self) -> bool {
        self.0.lock().check()
    }
}

/// A heap over `arena`. The arena must outlive the heap.
pub fn heap_over(arena: &mut [u8]) -> Heap {
    // SAFETY: the test owns `arena`, keeps it alive and untouched while the heap is used, and drops the heap first.
    unsafe { Heap::new(arena.as_mut_ptr(), arena.len()) }.unwrap()
}

/// A buddy over `arena`, or `None` if it is too small.
pub fn buddy_over(arena: &mut [u8]) -> Option<Buddy> {
    // SAFETY: the test owns `arena`, keeps it alive and untouched while the buddy is used, and drops the buddy first.
    unsafe { Buddy::new(arena.as_mut_ptr(), arena.len()) }
}

struct Blk {
    ptr: *mut u8,
    layout: Layout,
    tag: u8,
}

fn overlaps(a: &Blk, p: *mut u8, size: usize) -> bool {
    let (a0, a1) = (a.ptr as usize, a.ptr as usize + a.layout.size());
    let (b0, b1) = (p as usize, p as usize + size);
    a0 < b1 && b0 < a1
}

fn assert_block(b: &Blk) {
    // SAFETY: the block is live, so its bytes are readable and owned by the test until it is freed.
    let bytes = unsafe { core::slice::from_raw_parts(b.ptr, b.layout.size()) };
    assert!(bytes.iter().all(|&x| x == b.tag), "tag byte overwritten");
}

/// Random alloc/free churn with a shadow list: `ops` steps, at most `cap` live blocks.
/// Checks alignment, overlap, tag bytes, and `ok()` every 100 steps; frees everything at the end.
pub fn churn<A: Api>(api: &mut A, rng: &mut Rng, ops: usize, cap: usize) {
    let mut live: Vec<Blk> = Vec::new();
    for step in 0..ops {
        if live.is_empty() || (live.len() < cap && rng.chance(3, 5)) {
            let size = rng.range(1, 20_001) as usize;
            let align = 1usize << rng.below(13);
            let layout = Layout::from_size_align(size, align).unwrap();
            let p = api.alloc(layout);
            if !p.is_null() {
                assert_eq!(p as usize % align, 0, "misaligned block");
                assert!(
                    live.iter().all(|b| !overlaps(b, p, size)),
                    "overlapping live blocks"
                );
                let tag = (rng.u64() as u8) | 1;
                // SAFETY: `p` is a fresh block of `size` bytes, owned by this test until it is freed.
                unsafe { core::ptr::write_bytes(p, tag, size) };
                live.push(Blk {
                    ptr: p,
                    layout,
                    tag,
                });
            }
        } else {
            let b = live.swap_remove(rng.below(live.len()));
            assert_block(&b);
            // SAFETY: `b` came from `alloc` with `b.layout` and is removed from the list, so it is freed once.
            unsafe { api.free(b.ptr, b.layout) };
        }
        assert!(
            (step + 1) % 100 != 0 || api.ok(),
            "invariant check failed at step {step}"
        );
    }
    while let Some(b) = live.pop() {
        assert_block(&b);
        // SAFETY: as above; each live block is removed from the list and freed once.
        unsafe { api.free(b.ptr, b.layout) };
    }
    assert!(api.ok());
}
