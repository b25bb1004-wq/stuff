//! Binary buddy allocator: page-granular blocks, intrusive free lists, merge on free.

use crate::PAGE;
use core::ptr::{self, NonNull};

/// Largest block order: `2^MAX_ORDER` pages.
pub(crate) const MAX_ORDER: usize = 10;
/// End-of-list marker for the intrusive links.
const NIL: u32 = u32::MAX;

/// Binary buddy allocator over one caller-supplied memory region.
pub struct Buddy {
    /// Page-aligned start of the region. The order table occupies its first `pages` bytes.
    base: *mut u8,
    /// Whole pages in the region, metadata pages included.
    pages: usize,
    /// Leading pages that hold the order table and are never handed out.
    meta_pages: usize,
    /// First page index of each order's free list, or `NIL`.
    heads: [u32; MAX_ORDER + 1],
    /// Bytes currently free.
    free: usize,
}

// SAFETY: the Buddy exclusively owns its region and the raw pointer never escapes, so it may move between threads.
unsafe impl Send for Buddy {}

impl Buddy {
    /// Build a buddy allocator over `base..base + len`, or `None` if no usable page remains.
    ///
    /// # Safety
    /// `base..base + len` must be valid, writable and unused memory for the Buddy's whole life.
    pub unsafe fn new(base: *mut u8, len: usize) -> Option<Buddy> {
        let start = base as usize;
        let aligned = start.checked_add(PAGE - 1)? & !(PAGE - 1);
        let end = start.checked_add(len)? & !(PAGE - 1);
        if end <= aligned {
            return None;
        }
        let pages = (end - aligned) / PAGE;
        if pages >= NIL as usize {
            return None;
        }
        // The order table has one byte per page, so it needs this many leading pages.
        let meta_pages = pages.div_ceil(PAGE);
        if meta_pages >= pages {
            return None;
        }
        let base = base.wrapping_add(aligned - start);
        // SAFETY: the table is the first `pages` bytes of the region, which the caller promised is writable.
        unsafe { ptr::write_bytes(base, 0, pages) };
        let mut buddy = Buddy {
            base,
            pages,
            meta_pages,
            heads: [NIL; MAX_ORDER + 1],
            free: (pages - meta_pages) * PAGE,
        };
        // Tile the usable pages with the largest aligned blocks, low addresses first.
        let mut i = meta_pages;
        while i < pages {
            let mut k = MAX_ORDER;
            while k > 0 && (!i.is_multiple_of(1 << k) || i + (1 << k) > pages) {
                k -= 1;
            }
            buddy.push(k, i);
            i += 1 << k;
        }
        Some(buddy)
    }

    /// Allocate `2^order` contiguous, page-aligned pages, or `None` if no block that large is free.
    pub fn alloc_pages(&mut self, order: u32) -> Option<NonNull<u8>> {
        let k = order as usize;
        if k > MAX_ORDER {
            return None;
        }
        let j = (k..=MAX_ORDER).find(|&j| self.heads[j] != NIL)?;
        let i = self.heads[j] as usize;
        self.unlink(j, i);
        // Split down to order k: keep the low half, free each high half on the way.
        for c in (k..j).rev() {
            self.push(c, i + (1usize << c));
        }
        self.free = self.free.saturating_sub((1usize << k) * PAGE);
        NonNull::new(self.page_ptr(i))
    }

    /// Return a block from `alloc_pages(order)`, merging it with free buddies.
    ///
    /// # Safety
    /// `ptr` must have come from `alloc_pages(order)` on this Buddy and must not be freed yet.
    pub unsafe fn free_pages(&mut self, ptr: NonNull<u8>, order: u32) {
        let k0 = order as usize;
        if k0 > MAX_ORDER {
            return;
        }
        let off = (ptr.as_ptr() as usize).wrapping_sub(self.base as usize);
        if !off.is_multiple_of(PAGE) {
            return;
        }
        let mut i = off / PAGE;
        let size = 1usize << k0;
        // Reject anything that is not a whole block inside the usable pages.
        if i < self.meta_pages
            || i >= self.pages
            || !i.is_multiple_of(size)
            || size > self.pages - i
        {
            return;
        }
        self.free += size * PAGE;
        let mut k = k0;
        while k < MAX_ORDER {
            let b = i ^ (1 << k);
            if self.tbl(b) as usize != k + 1 {
                break;
            }
            self.unlink(k, b);
            i = i.min(b);
            k += 1;
        }
        self.push(k, i);
    }

    /// Bytes currently free.
    pub fn free_bytes(&self) -> usize {
        self.free
    }

    /// Usable bytes after the order table is subtracted.
    pub fn total_bytes(&self) -> usize {
        (self.pages - self.meta_pages) * PAGE
    }

    /// True if `p` points into a usable page of this Buddy.
    pub(crate) fn contains(&self, p: *mut u8) -> bool {
        let off = (p as usize).wrapping_sub(self.base as usize);
        off < self.pages * PAGE && off / PAGE >= self.meta_pages
    }

    /// Walk every free list and verify the invariants; false if any is broken.
    pub(crate) fn check(&self) -> bool {
        let mut sum = 0usize;
        for k in 0..=MAX_ORDER {
            let size = 1usize << k;
            let mut prev = NIL;
            let mut cur = self.heads[k];
            let mut steps = 0usize;
            while cur != NIL {
                steps += 1;
                let i = cur as usize;
                if steps > self.pages
                    || i < self.meta_pages
                    || i + size > self.pages
                    || !i.is_multiple_of(size)
                    || self.tbl(i) as usize != k + 1
                    || self.word(i, 0) != prev
                {
                    return false;
                }
                prev = cur;
                cur = self.word(i, 4);
                sum += size * PAGE;
            }
        }
        sum == self.free && self.free <= self.total_bytes()
    }

    fn page_ptr(&self, i: usize) -> *mut u8 {
        self.base.wrapping_add(i * PAGE)
    }

    /// Order-table entry for page `i`: 0 unless a free block of order `e - 1` starts there.
    fn tbl(&self, i: usize) -> u8 {
        if i >= self.pages {
            return 0;
        }
        // SAFETY: `i < pages`, so the byte lies inside the table at the start of the region.
        unsafe { *self.base.add(i) }
    }

    fn set_tbl(&mut self, i: usize, v: u8) {
        if i < self.pages {
            // SAFETY: `i < pages`, so the byte lies inside the table at the start of the region.
            unsafe { *self.base.add(i) = v };
        }
    }

    /// Read link word `off` (0 = prev, 4 = next) of free block `page`.
    fn word(&self, page: usize, off: usize) -> u32 {
        if page >= self.pages {
            return NIL;
        }
        // SAFETY: `page` is inside the region and page-aligned, so `off` (0 or 4) is a 4-aligned
        // word inside that page, and only free blocks (whose first word is ours) are read.
        unsafe { self.page_ptr(page).add(off).cast::<u32>().read() }
    }

    fn set_word(&mut self, page: usize, off: usize, v: u32) {
        if page < self.pages {
            // SAFETY: as in `word`; only free blocks we own are written.
            unsafe { self.page_ptr(page).add(off).cast::<u32>().write(v) };
        }
    }

    /// Put block `i` of order `k` at the head of its free list.
    fn push(&mut self, k: usize, i: usize) {
        let head = self.heads[k];
        self.set_word(i, 0, NIL);
        self.set_word(i, 4, head);
        if head != NIL {
            self.set_word(head as usize, 0, i as u32);
        }
        self.heads[k] = i as u32;
        self.set_tbl(i, (k + 1) as u8);
    }

    /// Remove block `i` of order `k` from its free list.
    fn unlink(&mut self, k: usize, i: usize) {
        let (prev, next) = (self.word(i, 0), self.word(i, 4));
        if prev == NIL {
            self.heads[k] = next;
        } else {
            self.set_word(prev as usize, 4, next);
        }
        if next != NIL {
            self.set_word(next as usize, 0, prev);
        }
        self.set_tbl(i, 0);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::testutil::buddy_over;
    use strata_testkit::{forall, Rng};

    const ARENA: usize = 16 << 20;

    /// Start and end address of an order-`o` block.
    fn span(p: NonNull<u8>, o: u32) -> (usize, usize) {
        let s = p.as_ptr() as usize;
        (s, s + (PAGE << o))
    }

    #[test]
    fn full_merge_after_random_free() {
        let mut arena = vec![0u8; ARENA];
        let mut b = buddy_over(&mut arena).unwrap();
        let start = b.free_bytes();
        assert_eq!(start, b.total_bytes());
        let mut blocks: Vec<(NonNull<u8>, u32)> = (0..=MAX_ORDER as u32)
            .map(|o| (b.alloc_pages(o).unwrap(), o))
            .collect();
        assert!(b.free_bytes() < start);
        let mut rng = Rng::new(7);
        for i in (1..blocks.len()).rev() {
            blocks.swap(i, rng.below(i + 1));
        }
        for (p, o) in blocks {
            // SAFETY: each block came from alloc_pages(o) above and is freed exactly once.
            unsafe { b.free_pages(p, o) };
        }
        assert_eq!(b.free_bytes(), start);
        assert!(b.check());
        assert!(b.alloc_pages(MAX_ORDER as u32).is_some());
        assert!(b.alloc_pages(MAX_ORDER as u32 + 1).is_none());
    }

    #[test]
    fn exhaustion_returns_none_and_recovers() {
        let mut arena = vec![0u8; 1 << 20];
        let mut b = buddy_over(&mut arena).unwrap();
        let total = b.total_bytes();
        let mut got = Vec::new();
        while let Some(p) = b.alloc_pages(0) {
            got.push(p);
        }
        assert_eq!(got.len() * PAGE, total);
        assert_eq!(b.free_bytes(), 0);
        assert!(b.alloc_pages(0).is_none());
        for p in got {
            // SAFETY: each pointer came from alloc_pages(0) and is freed exactly once.
            unsafe { b.free_pages(p, 0) };
        }
        assert_eq!(b.free_bytes(), total);
        assert!(b.check());
    }

    #[test]
    fn too_small_region_is_rejected() {
        let mut page = vec![0u8; PAGE];
        assert!(buddy_over(&mut page).is_none());
        let mut tiny = vec![0u8; 16];
        assert!(buddy_over(&mut tiny).is_none());
    }

    #[test]
    fn random_churn_keeps_blocks_disjoint() {
        forall(100, 0xB0DD, |rng| {
            let mut arena = vec![0u8; ARENA];
            let mut b = buddy_over(&mut arena).unwrap();
            let mut live: Vec<(NonNull<u8>, u32)> = Vec::new();
            for _ in 0..300 {
                if live.is_empty() || rng.chance(1, 2) {
                    let order = rng.below(MAX_ORDER + 1) as u32;
                    if let Some(p) = b.alloc_pages(order) {
                        assert_eq!(p.as_ptr() as usize % PAGE, 0);
                        let (s, e) = span(p, order);
                        assert!(live.iter().all(|&(q, o)| {
                            let (qs, qe) = span(q, o);
                            e <= qs || qe <= s
                        }));
                        live.push((p, order));
                    }
                } else {
                    let (p, o) = live.swap_remove(rng.below(live.len()));
                    // SAFETY: `p` came from alloc_pages(o), is still live, and is freed once.
                    unsafe { b.free_pages(p, o) };
                }
                assert!(b.check());
            }
            while let Some((p, o)) = live.pop() {
                // SAFETY: as above; every live block is freed exactly once.
                unsafe { b.free_pages(p, o) };
            }
            assert_eq!(b.free_bytes(), b.total_bytes());
            assert!(b.check());
        });
    }
}
