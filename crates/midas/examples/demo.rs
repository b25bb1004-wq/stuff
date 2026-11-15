//! Demo: a heap over a fixed arena, small and large allocations, the raw buddy, and the spin lock.
//! No clock, no randomness, no OS memory beyond the two arenas this program owns.

use std::alloc::Layout;

use midas::{Buddy, Heap, Locked, MAX_ALIGN, PAGE};

fn main() {
    // A 1 MiB arena. It outlives every pointer taken from the heap below.
    let mut arena = vec![0u8; 1 << 20];
    // SAFETY: the arena is valid, writable and used by nothing else for the rest of `main`.
    let Some(mut heap) = (unsafe { Heap::new(arena.as_mut_ptr(), arena.len()) }) else {
        println!("arena too small");
        return;
    };
    println!("page size {PAGE} bytes, largest block {MAX_ALIGN} bytes");
    println!(
        "heap ready: {}, usable bytes: {}",
        heap.is_ready(),
        heap.total_bytes()
    );
    println!("invariants hold at start: {}", heap.check());

    // 100 bytes is served from the 128-byte slab class.
    let small = Layout::from_size_align(100, 8).expect("valid layout");
    let a = heap.alloc(small);
    println!(
        "small alloc (100 bytes): null={}, used={}",
        a.is_null(),
        heap.used_bytes()
    );

    // 64 KiB is larger than any slab class, so it is a 16-page block from the buddy.
    let large = Layout::from_size_align(64 << 10, PAGE).expect("valid layout");
    let b = heap.alloc(large);
    println!(
        "large alloc (64 KiB): null={}, used={}",
        b.is_null(),
        heap.used_bytes()
    );

    // SAFETY: `a` and `b` came from `alloc` with these exact layouts and are freed once.
    unsafe {
        heap.dealloc(a, small);
        heap.dealloc(b, large);
    }
    println!(
        "after freeing both: used={}, invariants hold: {}",
        heap.used_bytes(),
        heap.check()
    );

    // The buddy allocator underneath, used directly: four pages, then give them back.
    let mut buddy_arena = vec![0u8; 256 << 10];
    // SAFETY: same contract as the heap arena above.
    let Some(mut buddy) = (unsafe { Buddy::new(buddy_arena.as_mut_ptr(), buddy_arena.len()) })
    else {
        println!("buddy arena too small");
        return;
    };
    let before = buddy.free_bytes();
    let pages = buddy.alloc_pages(2);
    println!(
        "buddy: free before {before}, got block: {}, free after {}",
        pages.is_some(),
        buddy.free_bytes()
    );
    if let Some(p) = pages {
        // SAFETY: `p` came from `alloc_pages(2)` on this buddy and is freed once.
        unsafe { buddy.free_pages(p, 2) };
    }
    println!(
        "buddy: free after giving it back {}, check {}",
        buddy.free_bytes(),
        buddy.check()
    );

    // The lock wraps a heap so it can be the global allocator. Here it is used directly.
    let mut lock_arena = vec![0u8; 256 << 10];
    let shared = Locked::new(Heap::empty());
    // SAFETY: same contract as the heap arena above.
    let ready = unsafe { shared.init(lock_arena.as_mut_ptr(), lock_arena.len()) };
    let c = shared.lock().alloc(small);
    println!("locked heap: init {ready}, alloc null={}", c.is_null());
    // SAFETY: `c` came from `alloc(small)` on this heap and is freed once.
    unsafe { shared.lock().dealloc(c, small) };
    println!(
        "locked heap: used after free {}",
        shared.lock().used_bytes()
    );
}
