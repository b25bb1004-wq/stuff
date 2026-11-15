//! `Locked<Heap>` as the `#[global_allocator]`, under threads, with tag-byte checks.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::addr_of_mut;
use midas::{Heap, Locked, MAX_ALIGN};
use std::collections::HashMap;

/// Usable bytes handed to the heap; the arena has one extra alignment step of slack.
const LEN: usize = 64 << 20;

/// Raw arena, not zeroed by the compiler: the heap writes every byte it reads.
static mut ARENA: core::mem::MaybeUninit<[u8; LEN + MAX_ALIGN]> = core::mem::MaybeUninit::uninit();

/// The lazily initialised global heap.
struct Global(Locked<Heap>);

/// Start of the arena rounded up to `MAX_ALIGN`.
fn region() -> *mut u8 {
    let start = addr_of_mut!(ARENA).cast::<u8>() as usize;
    let aligned = (start + MAX_ALIGN - 1) & !(MAX_ALIGN - 1);
    addr_of_mut!(ARENA)
        .cast::<u8>()
        .wrapping_add(aligned - start)
}

// SAFETY: every call goes through the `Locked` lock, and the arena is used only through that heap.
unsafe impl GlobalAlloc for Global {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the arena is used only as this region; `init` does nothing after its first success.
        unsafe { self.0.init(region(), LEN) };
        // SAFETY: GlobalAlloc's contract for `alloc` is the heap's contract.
        unsafe { self.0.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: GlobalAlloc's contract: `ptr` came from `alloc` with this layout.
        unsafe { self.0.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Global = Global(Locked::new(Heap::empty()));

struct Blk {
    ptr: *mut u8,
    layout: Layout,
    tag: u8,
}

fn check_tag(b: &Blk) {
    // SAFETY: the block is live, so its bytes are readable until this thread frees it.
    let bytes = unsafe { core::slice::from_raw_parts(b.ptr, b.layout.size()) };
    assert!(bytes.iter().all(|&x| x == b.tag), "tag byte overwritten");
}

fn churn(seed: u64, ops: usize) {
    let mut rng = Rng::new(seed);
    let mut live: Vec<Blk> = Vec::new();
    for _ in 0..ops {
        if live.is_empty() || (live.len() < 200 && rng.chance(3, 5)) {
            let size = rng.range(1, 20_001) as usize;
            let layout = Layout::from_size_align(size, 1 << rng.below(13)).unwrap();
            // SAFETY: the layout is valid, and GlobalAlloc::alloc has no other precondition.
            let ptr = unsafe { GlobalAlloc::alloc(&GLOBAL, layout) };
            if ptr.is_null() {
                continue;
            }
            assert_eq!(ptr as usize % layout.align(), 0, "misaligned block");
            let tag = (rng.u64() as u8) | 1;
            // SAFETY: `ptr` is a fresh block of `size` bytes owned by this thread.
            unsafe { core::ptr::write_bytes(ptr, tag, size) };
            live.push(Blk { ptr, layout, tag });
        } else {
            let b = live.swap_remove(rng.below(live.len()));
            check_tag(&b);
            // SAFETY: `b` came from GLOBAL.alloc with its layout, is removed from the list, and is freed once.
            unsafe { GlobalAlloc::dealloc(&GLOBAL, b.ptr, b.layout) };
        }
    }
    for b in live.drain(..) {
        check_tag(&b);
        // SAFETY: as above; every live block is freed exactly once.
        unsafe { GlobalAlloc::dealloc(&GLOBAL, b.ptr, b.layout) };
    }
}

#[test]
fn std_collections_work() {
    let mut v: Vec<u64> = (0..10_000).collect();
    v.reverse();
    assert_eq!(v[0], 9_999);
    let s: String = (0..1000u32)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    assert_eq!(s.len(), 1000);
    let b = Box::new([7u8; 4096]);
    assert_eq!(b[100], 7);
    let mut m = HashMap::new();
    for i in 0..5000u32 {
        m.insert(i, i * 2);
    }
    assert_eq!(m.get(&4999), Some(&9998));
    // Read under the lock, assert after it is released: a panic allocates, and would spin on it.
    let ok = GLOBAL.0.lock().check();
    assert!(ok, "heap check failed");
}

#[test]
fn four_threads_churn_with_tags() {
    let handles: Vec<_> = (0..4u64)
        .map(|t| std::thread::spawn(move || churn(t + 1, 5000)))
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // The harness keeps its own blocks in this heap, so used_bytes is not 0 here; check() is.
    let ok = GLOBAL.0.lock().check();
    assert!(ok, "heap check failed after the churn");
}
