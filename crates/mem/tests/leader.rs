//! Leader-run check: an independent shadow model over the public API only.
//! `STRATA_SEED` and `STRATA_CASES` change the run without editing the file.
#![allow(clippy::unwrap_used)]
use core::alloc::Layout;
use strata_mem::Heap;
use strata_testkit::{forall, Rng};

struct Live {
    ptr: *mut u8,
    layout: Layout,
    tag: u8,
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn release(heap: &mut Heap, b: Live) {
    // SAFETY: `b.ptr` is a live block of `b.layout.size()` bytes that this test allocated.
    let bytes = unsafe { core::slice::from_raw_parts(b.ptr, b.layout.size()) };
    assert!(
        bytes.iter().all(|x| *x == b.tag),
        "block contents were overwritten"
    );
    // SAFETY: allocated from this heap with this layout and not freed before.
    unsafe { heap.dealloc(b.ptr, b.layout) };
}

fn one_case(rng: &mut Rng) {
    const ARENA: usize = 8 << 20;
    let mut arena = vec![0u8; ARENA].into_boxed_slice();
    let (lo, hi) = (arena.as_ptr() as usize, arena.as_ptr() as usize + ARENA);
    // SAFETY: `arena` outlives `heap` (dropped at the end of this function) and nothing else uses it.
    let mut heap = unsafe { Heap::new(arena.as_mut_ptr(), ARENA) }.unwrap();
    let mut live: Vec<Live> = Vec::new();
    for op in 0..3000 {
        if live.is_empty() || rng.chance(3, 5) {
            let size = match rng.below(4) {
                0 => rng.range(1, 64),
                1 => rng.range(1, 2049),
                2 => rng.range(2049, 20_001),
                _ => rng.range(1, 300_000),
            } as usize;
            let layout = Layout::from_size_align(size, 1 << rng.below(13)).unwrap();
            let ptr = heap.alloc(layout);
            if ptr.is_null() {
                continue;
            }
            let (a, b) = (ptr as usize, ptr as usize + size);
            assert!(a >= lo && b <= hi, "block outside the arena");
            assert_eq!(a % layout.align(), 0, "misaligned block");
            for other in &live {
                let (oa, ob) = (other.ptr as usize, other.ptr as usize + other.layout.size());
                assert!(b <= oa || ob <= a, "two live blocks overlap");
            }
            let tag = rng.u64() as u8;
            // SAFETY: `ptr` is a fresh block of `size` bytes owned by this test.
            unsafe { core::ptr::write_bytes(ptr, tag, size) };
            live.push(Live { ptr, layout, tag });
        } else {
            let victim = live.swap_remove(rng.below(live.len()));
            release(&mut heap, victim);
        }
        if op % 250 == 0 {
            assert!(heap.check(), "heap invariant broken");
        }
    }
    for b in live.drain(..) {
        release(&mut heap, b);
    }
    assert!(heap.check());
    assert_eq!(heap.used_bytes(), 0, "memory leaked");
    let big = Layout::from_size_align(2 << 20, 4096).unwrap();
    let p = heap.alloc(big);
    assert!(!p.is_null(), "free memory did not merge back");
    // SAFETY: just allocated with this layout.
    unsafe { heap.dealloc(p, big) };
}

#[test]
fn shadow_model() {
    forall(
        env("STRATA_CASES", 20) as u32,
        env("STRATA_SEED", 0x57A7A),
        one_case,
    );
}
