//! Corrupted memory must make `check()` return false or true, never panic, never read outside the region.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use core::alloc::Layout;
use quarry::{Heap, PAGE};

const LEN: usize = 4 << 20;

/// A heap whose class-64 page keeps slot 0 live, plus a live 64 KiB block; slot 1 is free.
struct Fixture {
    arena: Vec<u8>,
    heap: Heap,
    slots: Vec<*mut u8>,
    big: *mut u8,
}

fn fixture() -> Fixture {
    let mut arena = vec![0u8; LEN];
    // SAFETY: the arena is owned by the returned Fixture, which keeps it alive for the heap's life.
    let mut heap = unsafe { Heap::new(arena.as_mut_ptr(), LEN) }.unwrap();
    let small = Layout::from_size_align(64, 8).unwrap();
    let slots: Vec<*mut u8> = (0..5).map(|_| heap.alloc(small)).collect();
    assert!(slots.iter().all(|p| !p.is_null()));
    // SAFETY: slot 1 came from `alloc` with `small` and is freed once; the page stays live.
    unsafe { heap.dealloc(slots[1], small) };
    let big = heap.alloc(Layout::from_size_align(64 << 10, 8).unwrap());
    assert!(!big.is_null());
    assert!(heap.check());
    Fixture {
        arena,
        heap,
        slots,
        big,
    }
}

/// Overwrite the first word of a free slot or block, the place the free list keeps its link.
fn set_word(at: *mut u8, value: usize) {
    // SAFETY: `at` is a free slot or block the test owns inside the arena, so its first word is writable.
    unsafe { at.cast::<usize>().write_unaligned(value) };
}

#[test]
fn listed_corruptions_are_detected() {
    // A link past the end of the region.
    let f = fixture();
    let past = (f.arena.as_ptr() as usize + LEN + 4 * PAGE) & !(PAGE - 1);
    set_word(f.slots[1], past);
    assert!(!f.heap.check(), "link outside the region");

    // A link misaligned for its class.
    let f = fixture();
    set_word(f.slots[1], f.slots[1] as usize + 8);
    assert!(!f.heap.check(), "misaligned link");

    // A link into a page that is not a slab page of this class (a live large block).
    let f = fixture();
    set_word(f.slots[1], f.big as usize + 128);
    assert!(!f.heap.check(), "link into a non-slab page");

    // A cycle: the slot links to itself.
    let f = fixture();
    set_word(f.slots[1], f.slots[1] as usize);
    assert!(!f.heap.check(), "cycle in the free list");

    // A slab page whose live count disagrees with its free slots.
    let f = fixture();
    let header = f.slots[1]
        .wrapping_sub(f.slots[1] as usize % PAGE)
        .cast::<usize>();
    // SAFETY: the header is the first word of a slab page inside the arena, owned by the test.
    unsafe { header.write(header.read() + 1) };
    assert!(!f.heap.check(), "live count mismatch");

    // A slab page with no live slots must have been returned to the buddy.
    let f = fixture();
    let header = f.slots[1]
        .wrapping_sub(f.slots[1] as usize % PAGE)
        .cast::<usize>();
    // SAFETY: as above.
    unsafe { header.write(0) };
    assert!(!f.heap.check(), "empty slab page kept");
}

#[test]
fn random_corruption_never_panics() {
    forall(500, 0xC0DE, |rng: &mut Rng| {
        let mut arena = vec![0u8; LEN];
        // SAFETY: the arena outlives the heap, and the heap is dropped before it.
        let mut heap = unsafe { Heap::new(arena.as_mut_ptr(), LEN) }.unwrap();
        let mut live: Vec<(*mut u8, Layout)> = Vec::new();
        let mut freed_slots: Vec<*mut u8> = Vec::new();
        let mut freed_blocks: Vec<*mut u8> = Vec::new();
        for _ in 0..600 {
            if live.is_empty() || rng.chance(3, 5) {
                let layout = Layout::from_size_align(1 + rng.below(3000), 8).unwrap();
                let p = heap.alloc(layout);
                if !p.is_null() {
                    live.push((p, layout));
                }
            } else {
                let (p, l) = live.swap_remove(rng.below(live.len()));
                // SAFETY: `p` came from `alloc` with `l` and is freed once.
                unsafe { heap.dealloc(p, l) };
                if l.size() <= 2048 {
                    freed_slots.push(p);
                } else {
                    freed_blocks.push(p);
                }
            }
        }
        assert!(heap.check());
        // Write random words into freed slots and freed blocks, then check: no panic, no escape.
        for _ in 0..8 {
            let pool = if freed_slots.is_empty() || rng.chance(1, 3) {
                &freed_blocks
            } else {
                &freed_slots
            };
            if pool.is_empty() {
                continue;
            }
            let p = pool[rng.below(pool.len())];
            set_word(p, rng.u64() as usize);
            let _verdict = heap.check();
        }
    });
}
