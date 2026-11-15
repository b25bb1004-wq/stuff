//! Leader check for T16: midas as the real `#[global_allocator]` of this test binary, with 8
//! threads driving std collections (growth goes through realloc) and raw over-aligned blocks.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use core::alloc::{GlobalAlloc, Layout};
use midas::{Heap, Locked, MAX_ALIGN};
use std::collections::HashMap;

const LEN: usize = 128 << 20;
static mut ARENA: core::mem::MaybeUninit<[u8; LEN + MAX_ALIGN]> = core::mem::MaybeUninit::uninit();

struct Q(Locked<Heap>);

// SAFETY: all access goes through the spin lock; the arena is used only by this heap.
unsafe impl GlobalAlloc for Q {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let mut h = self.0.lock();
        if !h.is_ready() {
            let start = core::ptr::addr_of_mut!(ARENA).cast::<u8>();
            // SAFETY: the static arena is handed over once, on the first allocation.
            unsafe { h.init(start, LEN + MAX_ALIGN) };
        }
        h.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: GlobalAlloc's contract: `p` came from `alloc` with `l`.
        unsafe { self.0.lock().dealloc(p, l) }
    }
}

#[global_allocator]
static Q_HEAP: Q = Q(Locked::new(Heap::empty()));

fn worker(seed: u64) -> u64 {
    let mut r = Rng::new(seed);
    let mut sum = 0u64;
    let mut map: HashMap<u64, Vec<u8>> = HashMap::new();
    let mut strings: Vec<String> = Vec::new();
    for i in 0..3000u64 {
        let k = r.u64() % 500;
        let n = r.below(3000);
        let v = vec![(k as u8) | 1; n];
        if let Some(old) = map.insert(k, v) {
            assert!(
                old.iter().all(|&b| b == (k as u8) | 1),
                "map value corrupted"
            );
        }
        let mut s = String::new();
        for _ in 0..r.below(200) {
            s.push(char::from(b'a' + (i % 26) as u8));
        }
        strings.push(s);
        if strings.len() > 300 {
            let s = strings.swap_remove(r.below(strings.len()));
            sum += s.len() as u64;
        }
        if i % 100 == 0 {
            let align = 1usize << r.below(23);
            let l = Layout::from_size_align(r.range(1, 70_000) as usize, align).unwrap();
            // SAFETY: valid layout.
            let p = unsafe { std::alloc::alloc(l) };
            assert!(!p.is_null(), "aligned alloc {l:?} failed");
            assert_eq!(p as usize % align, 0);
            // SAFETY: from `alloc` with `l`.
            unsafe { std::alloc::dealloc(p, l) };
        }
    }
    sum + map.len() as u64
}

#[test]
fn leader_quarry_as_global_allocator_under_threads() {
    let hs: Vec<_> = (0..8u64)
        .map(|t| std::thread::spawn(move || worker(0x6106_0000 + t)))
        .collect();
    let total: u64 = hs.into_iter().map(|h| h.join().unwrap()).sum();
    let used = Q_HEAP.0.lock().used_bytes();
    eprintln!("threads done: total {total}, midas used_bytes {used}");
    assert!(total > 100_000, "workers did too little: {total}");
    assert!(used > 0, "midas holds nothing, so it is not the allocator");
    let ok = Q_HEAP.0.lock().check();
    assert!(ok, "check failed after the threads");
}
