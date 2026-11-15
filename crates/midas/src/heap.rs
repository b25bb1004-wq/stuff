//! Size-class slab heap on top of the buddy allocator.

use crate::buddy::{Buddy, MAX_ORDER};
use crate::{MAX_ALIGN, PAGE};
use core::alloc::Layout;
use core::ptr::{self, NonNull};

/// Smallest slab class in bytes.
const MIN_CLASS: usize = 16;
/// Largest slab class in bytes; bigger requests go straight to the buddy.
const MAX_CLASS: usize = 2048;
/// Number of slab classes: 16, 32, ..., 2048.
const CLASSES: usize = 8;
/// Buddy page tag of a slab page: `SLAB_TAG | (class + 1)`, never a free-block order.
const SLAB_TAG: u8 = 0x80;

/// Size-class slab heap that takes whole pages from a `Buddy`.
///
/// Each slab page keeps a live-slot count in its first word, and slot 0 of every page is that
/// header. A page whose count drops to zero goes back to the buddy.
pub struct Heap {
    buddy: Buddy,
    /// Head of each class's free-slot list; the first word of a free slot links to the next.
    slabs: [*mut u8; CLASSES],
    /// Bytes handed to callers, rounded to class or page size.
    used: usize,
}

// SAFETY: the Heap exclusively owns its region and its raw pointers never escape, so it may move between threads.
unsafe impl Send for Heap {}

impl Heap {
    /// A Heap with no memory: every non-zero-size `alloc` returns null until `init` succeeds.
    pub const fn empty() -> Heap {
        Heap {
            buddy: Buddy::empty(),
            slabs: [ptr::null_mut(); CLASSES],
            used: 0,
        }
    }

    /// Build a heap over `base..base + len`, or `None` if the region is too small.
    ///
    /// # Safety
    /// Same contract as `Buddy::new`: the region must stay valid, writable and unused for the heap's whole life.
    pub unsafe fn new(base: *mut u8, len: usize) -> Option<Heap> {
        // SAFETY: the caller's promise about the region is exactly the one `Buddy::new` needs.
        let buddy = unsafe { Buddy::new(base, len) }?;
        Some(Heap {
            buddy,
            slabs: [ptr::null_mut(); CLASSES],
            used: 0,
        })
    }

    /// Give an empty heap its region. Returns false, and changes nothing, if it already has one or the region is too small.
    ///
    /// # Safety
    /// Same contract as `Heap::new`.
    pub unsafe fn init(&mut self, base: *mut u8, len: usize) -> bool {
        if self.is_ready() {
            return false;
        }
        // SAFETY: the caller's promise about the region is exactly the one `Buddy::new` needs.
        match unsafe { Buddy::new(base, len) } {
            Some(buddy) => {
                self.buddy = buddy;
                true
            }
            None => false,
        }
    }

    /// True once the heap owns a region, after a successful `new` or `init`.
    pub fn is_ready(&self) -> bool {
        self.buddy.is_ready()
    }

    /// Usable bytes of the region; 0 when the heap is empty.
    pub fn total_bytes(&self) -> usize {
        self.buddy.total_bytes()
    }

    /// Allocate memory for `layout`, or return null when out of memory. Zero-size layouts get a dangling aligned pointer.
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let (size, align) = (layout.size(), layout.align());
        if size == 0 {
            return align as *mut u8;
        }
        if let Some(c) = class_of(size, align) {
            return self.alloc_slot(c);
        }
        let Some(order) = large_order(layout) else {
            return ptr::null_mut();
        };
        match self.buddy.alloc_pages(order) {
            Some(p) => {
                self.used += order_bytes(order);
                p.as_ptr()
            }
            None => ptr::null_mut(),
        }
    }

    /// Free a block from `alloc` with the same layout. Zero-size layouts are ignored.
    ///
    /// # Safety
    /// `ptr` must have come from `alloc(layout)` on this Heap and must not be freed yet.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        if size == 0 || ptr.is_null() {
            return;
        }
        if let Some(c) = class_of(size, layout.align()) {
            self.free_slot(c, ptr);
        } else if let (Some(order), Some(p)) = (large_order(layout), NonNull::new(ptr)) {
            // SAFETY: the caller promises `ptr` is a live block from `alloc` with this layout, so it is a block of `order` on our buddy.
            unsafe { self.buddy.free_pages(p, order) };
            self.used = self.used.saturating_sub(order_bytes(order));
        }
    }

    /// Bytes handed to callers, rounded to class or page size.
    pub fn used_bytes(&self) -> usize {
        self.used
    }

    /// Walk every free list (buddy and slabs) and verify the invariants; false if any is broken.
    /// Safe on corrupted memory: every link is range-checked before it is read, and walks are bounded.
    pub fn check(&self) -> bool {
        if !self.buddy.check() {
            return false;
        }
        let limit = self.buddy.total_bytes() / MIN_CLASS + 1;
        for (c, &head) in self.slabs.iter().enumerate() {
            let size = class_size(c);
            let tag = SLAB_TAG | (c as u8 + 1);
            // Every link must name a slot of this class inside a slab page of this class.
            let mut cur = head;
            let mut steps = 0usize;
            while !cur.is_null() {
                steps += 1;
                let addr = cur as usize;
                let off = addr % PAGE;
                if steps > limit
                    || off < size
                    || !addr.is_multiple_of(size)
                    || self.buddy.tag_of(cur.wrapping_sub(off)) != tag
                {
                    return false;
                }
                // SAFETY: `cur` passed the checks above, so it is a free slot of this heap whose first word is a link.
                cur = unsafe { cur.cast::<*mut u8>().read() };
            }
            // Each slab page: live slots plus its free slots must fill the page exactly.
            let expected = PAGE / size - 1;
            for i in 0..self.buddy.page_count() {
                let page = self.buddy.page_at(i);
                if self.buddy.tag_of(page) != tag {
                    continue;
                }
                // SAFETY: `page` is a slab page of this heap inside the region, so its first word is the live count.
                let live = unsafe { page.cast::<usize>().read() };
                if live == 0 || live > expected || live + free_in(head, page, limit) != expected {
                    return false;
                }
            }
        }
        true
    }

    fn alloc_slot(&mut self, c: usize) -> *mut u8 {
        if self.slabs[c].is_null() && !self.refill(c) {
            return ptr::null_mut();
        }
        let slot = self.slabs[c];
        // SAFETY: `slot` is a free slot of class `c`, so its first word holds the link to the next free slot.
        self.slabs[c] = unsafe { slot.cast::<*mut u8>().read() };
        let page = slot.wrapping_sub(slot as usize % PAGE);
        // SAFETY: `page` is a slab page of this heap, and its first word is the live-slot count.
        unsafe { page.cast::<usize>().write(page.cast::<usize>().read() + 1) };
        self.used += class_size(c);
        slot
    }

    /// Take one page from the buddy, tag it as a class-`c` slab page, and carve its slots 1.. onto class `c`'s free list.
    fn refill(&mut self, c: usize) -> bool {
        let Some(page) = self.buddy.alloc_pages(0) else {
            return false;
        };
        let page = page.as_ptr();
        self.buddy.set_tag(page, SLAB_TAG | (c as u8 + 1));
        let size = class_size(c);
        // SAFETY: `page` is a fresh, page-aligned PAGE-byte block owned by this heap, so the header word and every slot `k * size` (k < PAGE / size) are in bounds and aligned to `size`.
        unsafe {
            page.cast::<usize>().write(0);
            for k in (1..PAGE / size).rev() {
                let slot = page.add(k * size);
                slot.cast::<*mut u8>().write(self.slabs[c]);
                self.slabs[c] = slot;
            }
        }
        true
    }

    /// Push a dead slot of class `c` back, and return its page to the buddy when it empties.
    fn free_slot(&mut self, c: usize, slot: *mut u8) {
        // SAFETY: the caller returns a slot of class `c` that is no longer live, so its first word can hold the link.
        unsafe { slot.cast::<*mut u8>().write(self.slabs[c]) };
        self.slabs[c] = slot;
        let page = slot.wrapping_sub(slot as usize % PAGE);
        // SAFETY: `page` is a slab page of this heap, so its header word is valid.
        let live = unsafe { page.cast::<usize>().read() }.saturating_sub(1);
        // SAFETY: same page header as above.
        unsafe { page.cast::<usize>().write(live) };
        if live == 0 {
            self.reclaim(c, page);
        }
        self.used = self.used.saturating_sub(class_size(c));
    }

    /// Drop every free slot of `page` from class `c`'s list, then give the page to the buddy.
    fn reclaim(&mut self, c: usize, page: *mut u8) {
        let mut keep: *mut u8 = ptr::null_mut();
        let mut cur = self.slabs[c];
        while !cur.is_null() {
            // SAFETY: `cur` is a free slot of class `c`, so its first word is a valid link.
            let next = unsafe { cur.cast::<*mut u8>().read() };
            if (cur as usize).wrapping_sub(page as usize) >= PAGE {
                // SAFETY: as above; `cur` is a free slot we keep on the list.
                unsafe { cur.cast::<*mut u8>().write(keep) };
                keep = cur;
            }
            cur = next;
        }
        self.slabs[c] = keep;
        if let Some(p) = NonNull::new(page) {
            // SAFETY: `page` came from `alloc_pages(0)` in `refill`, and none of its slots are live any more.
            unsafe { self.buddy.free_pages(p, 0) };
        }
    }
}

/// Count the slots on the list from `head` that lie in `page`. The list must already be validated, and `limit` bounds the walk.
fn free_in(head: *mut u8, page: *mut u8, limit: usize) -> usize {
    let (mut n, mut steps, mut cur) = (0usize, 0usize, head);
    while !cur.is_null() && steps <= limit {
        steps += 1;
        if (cur as usize).wrapping_sub(page as usize) < PAGE {
            n += 1;
        }
        // SAFETY: the list was validated by `Heap::check`, so `cur` is a readable free slot.
        cur = unsafe { cur.cast::<*mut u8>().read() };
    }
    n
}

/// Slab class index for a request, or `None` if it must go to the buddy.
pub(crate) fn class_of(size: usize, align: usize) -> Option<usize> {
    let need = size.max(MIN_CLASS).checked_next_power_of_two()?;
    if need > MAX_CLASS || align > need {
        return None;
    }
    Some((need.trailing_zeros() - MIN_CLASS.trailing_zeros()) as usize)
}

pub(crate) fn class_size(c: usize) -> usize {
    MIN_CLASS << c
}

/// Buddy order that covers a large request, or `None` if it is too big or over-aligned.
pub(crate) fn large_order(layout: Layout) -> Option<u32> {
    if layout.align() > MAX_ALIGN {
        return None;
    }
    let bytes = layout.size().max(layout.align());
    let pages = bytes.checked_add(PAGE - 1)? / PAGE;
    let order = pages.checked_next_power_of_two()?.trailing_zeros();
    (order as usize <= MAX_ORDER).then_some(order)
}

pub(crate) fn order_bytes(order: u32) -> usize {
    (1usize << order) * PAGE
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::testutil::{churn, heap_over};
    use assay::forall;

    const ARENA: usize = 16 << 20;

    #[test]
    fn used_bytes_counts_class_sizes() {
        let mut arena = vec![0u8; 1 << 20];
        let mut heap = heap_over(&mut arena);
        let l = Layout::from_size_align(20, 1).unwrap();
        let p = heap.alloc(l);
        assert!(!p.is_null());
        assert_eq!(heap.used_bytes(), 32);
        // SAFETY: `p` came from `alloc` with `l` and is freed once.
        unsafe { heap.dealloc(p, l) };
        assert_eq!(heap.used_bytes(), 0);
        assert!(heap.check());
    }

    #[test]
    fn zero_size_and_over_aligned_requests() {
        let mut arena = vec![0u8; 1 << 20];
        let mut heap = heap_over(&mut arena);
        let z = Layout::from_size_align(0, 32).unwrap();
        let p = heap.alloc(z);
        assert_eq!(p as usize, 32);
        // SAFETY: zero-size blocks are ignored by dealloc, so this is a no-op.
        unsafe { heap.dealloc(p, z) };
        assert!(heap
            .alloc(Layout::from_size_align(64, 2 * MAX_ALIGN).unwrap())
            .is_null());
        assert_eq!(heap.used_bytes(), 0);
    }

    #[test]
    fn out_of_memory_returns_null_and_heap_recovers() {
        let mut arena = vec![0u8; 64 << 10];
        let mut heap = heap_over(&mut arena);
        assert!(heap
            .alloc(Layout::from_size_align(1 << 20, 8).unwrap())
            .is_null());
        assert!(heap
            .alloc(Layout::from_size_align(usize::MAX / 2, 1).unwrap())
            .is_null());
        let l = Layout::from_size_align(2048, 8).unwrap();
        let mut got = Vec::new();
        loop {
            let p = heap.alloc(l);
            if p.is_null() {
                break;
            }
            got.push(p);
        }
        assert!(!got.is_empty());
        for p in got {
            // SAFETY: each pointer came from `alloc` with `l` and is freed once.
            unsafe { heap.dealloc(p, l) };
        }
        assert_eq!(heap.used_bytes(), 0);
        assert!(heap.check());
        let p = heap.alloc(l);
        assert!(!p.is_null());
        // SAFETY: `p` came from `alloc` with `l` and is freed once.
        unsafe { heap.dealloc(p, l) };
    }

    #[test]
    fn random_churn_property() {
        forall(200, 0x4EA9, |rng| {
            let mut arena = vec![0u8; ARENA + PAGE];
            let mut heap = heap_over(&mut arena);
            churn(&mut heap, rng, 2000, 300);
            assert_eq!(heap.used_bytes(), 0);
            assert!(heap.check());
            // Everything is free again, so the buddy must hand out a max-order block.
            let max = Layout::from_size_align(order_bytes(MAX_ORDER as u32), PAGE).unwrap();
            let p = heap.alloc(max);
            assert!(!p.is_null(), "max-size block after freeing everything");
            // SAFETY: `p` came from `alloc` with `max` and is freed once.
            unsafe { heap.dealloc(p, max) };
            assert!(heap.check());
        });
    }

    #[test]
    fn empty_heap_is_inert_until_init() {
        let mut heap = Heap::empty();
        assert!(heap.check());
        assert!(!heap.is_ready());
        assert_eq!(heap.total_bytes(), 0);
        let l = Layout::from_size_align(8, 8).unwrap();
        assert!(heap.alloc(l).is_null());
        let mut arena = vec![0u8; 1 << 20];
        // SAFETY: the arena outlives the heap and nothing else uses it.
        assert!(unsafe { heap.init(arena.as_mut_ptr(), arena.len()) });
        assert!(heap.is_ready());
        // SAFETY: as above; a second init must fail and leave the heap alone.
        assert!(!unsafe { heap.init(arena.as_mut_ptr(), arena.len()) });
        let p = heap.alloc(l);
        assert!(!p.is_null());
        // SAFETY: `p` came from `alloc` with `l` and is freed once.
        unsafe { heap.dealloc(p, l) };
    }
}
