//! A `static` spin-locked heap built by `const` constructors and initialised later.
#![allow(clippy::unwrap_used)]
use core::alloc::Layout;
use quarry::{Heap, Locked};

static HEAP: Locked<Heap> = Locked::new(Heap::empty());

#[test]
fn static_heap_initialises_once() {
    assert_eq!(HEAP.lock().total_bytes(), 0);
    let region: &'static mut [u8] = Box::leak(vec![0u8; 1 << 20].into_boxed_slice());
    let (base, len) = (region.as_mut_ptr(), region.len());
    // SAFETY: the region is leaked, so it lives forever, and nothing else touches it.
    assert!(unsafe { HEAP.init(base, len) });
    let total = HEAP.lock().total_bytes();
    assert!(total > 0);
    let other: &'static mut [u8] = Box::leak(vec![0u8; 4 << 20].into_boxed_slice());
    // SAFETY: as above; the second region is refused, so the heap never uses it.
    assert!(!unsafe { HEAP.init(other.as_mut_ptr(), other.len()) });
    assert_eq!(HEAP.lock().total_bytes(), total);
    let mut heap = HEAP.lock();
    let l = Layout::from_size_align(100, 8).unwrap();
    let p = heap.alloc(l);
    assert!(!p.is_null());
    // SAFETY: `p` came from `alloc` with `l` and is freed once.
    unsafe { heap.dealloc(p, l) };
    assert!(heap.check());
}
