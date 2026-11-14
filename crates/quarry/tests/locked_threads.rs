//! Four threads churn one `Locked<Heap>` through `GlobalAlloc`, with tag-byte checks.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use core::alloc::{GlobalAlloc, Layout};
use quarry::{Heap, Locked};

const LEN: usize = 64 << 20;

/// Shared arena, aligned by hand to `PAGE` so the test needs no `unsafe` static.
fn arena() -> Vec<u8> {
    vec![0u8; LEN + 4096]
}

fn churn(lock: &Locked<Heap>, seed: u64, ops: usize) {
    let mut rng = Rng::new(seed);
    let mut live: Vec<(*mut u8, Layout, u8)> = Vec::new();
    for step in 0..ops {
        if live.is_empty() || (live.len() < 200 && rng.chance(3, 5)) {
            let size = rng.range(1, 20_001) as usize;
            let align = 1usize << rng.below(13);
            let layout = Layout::from_size_align(size, align).unwrap();
            // SAFETY: the layout is valid, and GlobalAlloc::alloc has no other precondition.
            let p = unsafe { GlobalAlloc::alloc(lock, layout) };
            if p.is_null() {
                continue;
            }
            assert_eq!(
                p as usize % align,
                0,
                "misaligned at step {step}, size {size}, align {align}"
            );
            let tag = (rng.u64() as u8) | 1;
            // SAFETY: `p` is a fresh block of `size` bytes owned by this thread.
            unsafe { core::ptr::write_bytes(p, tag, size) };
            live.push((p, layout, tag));
        } else {
            let (p, layout, tag) = live.swap_remove(rng.below(live.len()));
            // SAFETY: `p` is a live block of `layout.size()` bytes owned by this thread.
            let bytes = unsafe { core::slice::from_raw_parts(p, layout.size()) };
            assert!(
                bytes.iter().all(|&x| x == tag),
                "tag overwritten at step {step}"
            );
            // SAFETY: `p` came from alloc with `layout`, is removed from the list, and is freed once.
            unsafe { GlobalAlloc::dealloc(lock, p, layout) };
        }
    }
    for (p, layout, _) in live {
        // SAFETY: each live block is freed exactly once.
        unsafe { GlobalAlloc::dealloc(lock, p, layout) };
    }
}

#[test]
fn four_threads_on_plain_locked_heap() {
    let mut mem = arena();
    let base = mem.as_mut_ptr();
    let lock = Locked::new(Heap::empty());
    // SAFETY: `mem` outlives the scope below and is used only through this lock.
    assert!(unsafe { lock.init(base, LEN) });
    std::thread::scope(|s| {
        for t in 0..4u64 {
            let lock = &lock;
            s.spawn(move || churn(lock, t + 1, 5000));
        }
    });
    assert!(lock.lock().check());
    assert_eq!(lock.lock().used_bytes(), 0);
}
