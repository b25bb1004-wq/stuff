//! Big alignments up to `MAX_ALIGN`, over arenas that start at odd page offsets.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use core::alloc::Layout;
use core::ptr::NonNull;
use quarry::{Buddy, Heap, MAX_ALIGN, PAGE};

const REGION: usize = 16 << 20;

/// Start of a region that begins at an odd page offset (plus a few bytes) inside `arena`.
fn odd_base(arena: &mut [u8], rng: &mut Rng) -> *mut u8 {
    let off = (2 * rng.below(1500) + 1) * PAGE + rng.below(PAGE);
    arena[off..].as_mut_ptr()
}

#[test]
fn max_align_and_oversize_requests() {
    let mut arena = vec![0u8; REGION + PAGE];
    // SAFETY: the arena outlives the heap and nothing else uses it.
    let mut heap = unsafe { Heap::new(arena.as_mut_ptr(), REGION + PAGE) }.unwrap();
    let l = Layout::from_size_align(64, MAX_ALIGN).unwrap();
    let p = heap.alloc(l);
    assert!(!p.is_null());
    assert_eq!(p as usize % MAX_ALIGN, 0);
    // SAFETY: `p` came from `alloc` with `l` and is freed once.
    unsafe { heap.dealloc(p, l) };
    let too_big_align = Layout::from_size_align(64, 2 * MAX_ALIGN).unwrap();
    assert!(heap.alloc(too_big_align).is_null());
    let too_big_size = Layout::from_size_align(MAX_ALIGN + 1, 8).unwrap();
    assert!(heap.alloc(too_big_size).is_null());
    assert_eq!(heap.used_bytes(), 0);
    assert!(heap.check());
}

#[test]
fn big_alignments_on_odd_offsets() {
    forall(200, 0xA11C, |rng| {
        let mut arena = vec![0u8; 40 << 20];
        let base = odd_base(&mut arena, rng);
        // SAFETY: the region lies inside `arena`, which outlives the heap and is used only through it.
        let mut heap = unsafe { Heap::new(base, REGION) }.unwrap();
        let lo = base as usize;
        let mut live: Vec<(usize, usize, Layout, u8)> = Vec::new();
        for _ in 0..300 {
            if live.is_empty() || (live.len() < 40 && rng.chance(3, 5)) {
                // Log-uniform sizes in 1..=1 MiB keep the tag checks cheap.
                let e = rng.below(21) as u32;
                let size = rng.range(1, (1u64 << e) + 1) as usize;
                let align = 1usize << rng.below(23);
                let layout = Layout::from_size_align(size, align).unwrap();
                let p = heap.alloc(layout);
                if p.is_null() {
                    continue;
                }
                let a = p as usize;
                assert_eq!(a % align, 0, "block is not absolutely aligned");
                assert!(
                    a >= lo && a + size <= lo + REGION,
                    "block outside the region"
                );
                assert!(
                    live.iter().all(|&(b, s, _, _)| a + size <= b || b + s <= a),
                    "live blocks overlap"
                );
                let tag = (rng.u64() as u8) | 1;
                // SAFETY: `p` is a fresh block of `size` bytes owned by this test.
                unsafe { core::ptr::write_bytes(p, tag, size) };
                live.push((a, size, layout, tag));
            } else {
                let (a, size, layout, tag) = live.swap_remove(rng.below(live.len()));
                // SAFETY: `a` is a live block of `size` bytes that this test owns.
                let bytes = unsafe { core::slice::from_raw_parts(a as *const u8, size) };
                assert!(bytes.iter().all(|&x| x == tag), "tag byte overwritten");
                // SAFETY: `a` came from `alloc` with `layout`, is removed from the list, and is freed once.
                unsafe { heap.dealloc(a as *mut u8, layout) };
            }
        }
        assert!(heap.check());
        while let Some((a, size, layout, tag)) = live.pop() {
            // SAFETY: as above; each live block is freed exactly once.
            let bytes = unsafe { core::slice::from_raw_parts(a as *const u8, size) };
            assert!(bytes.iter().all(|&x| x == tag), "tag byte overwritten");
            // SAFETY: as above.
            unsafe { heap.dealloc(a as *mut u8, layout) };
        }
        assert_eq!(heap.used_bytes(), 0);
        assert!(heap.check());
        let max = Layout::from_size_align(1, MAX_ALIGN).unwrap();
        let p = heap.alloc(max);
        assert!(!p.is_null(), "a MAX_ALIGN block after freeing everything");
        // SAFETY: `p` came from `alloc` with `max` and is freed once.
        unsafe { heap.dealloc(p, max) };
    });
}

#[test]
fn buddy_blocks_are_absolutely_aligned() {
    forall(200, 0xB1D, |rng| {
        let mut arena = vec![0u8; 40 << 20];
        let base = odd_base(&mut arena, rng);
        // SAFETY: the region lies inside `arena`, which outlives the buddy and is used only through it.
        let mut b = unsafe { Buddy::new(base, REGION) }.unwrap();
        let start = b.free_bytes();
        let mut live: Vec<(NonNull<u8>, u32)> = Vec::new();
        for _ in 0..300 {
            if live.is_empty() || rng.chance(1, 2) {
                let order = rng.below(11) as u32;
                if let Some(p) = b.alloc_pages(order) {
                    assert_eq!(
                        p.as_ptr() as usize % (PAGE << order),
                        0,
                        "block not aligned"
                    );
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
            // SAFETY: as above; each live block is freed exactly once.
            unsafe { b.free_pages(p, o) };
        }
        assert_eq!(b.free_bytes(), start);
        assert!(b.check());
    });
}
