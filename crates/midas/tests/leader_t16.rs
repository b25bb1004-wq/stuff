//! Leader check for T16 (public API only, fresh seeds): absolute big alignments on arenas that
//! start at odd page offsets, the empty -> init life cycle through a static, targeted corruptions
//! that `check()` must report, and random corruption that must never panic it.
//! `STRATA_SEED` and `STRATA_CASES` change the run.
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use core::alloc::Layout;
use midas::{Buddy, Heap, Locked, MAX_ALIGN, PAGE};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A leaked arena; returns a pointer `skew` bytes past a `MAX_ALIGN` boundary.
fn arena(len: usize, skew: usize) -> *mut u8 {
    let raw = Box::leak(vec![0u8; len + 2 * MAX_ALIGN].into_boxed_slice()).as_mut_ptr();
    let up = (raw as usize).next_multiple_of(MAX_ALIGN) - raw as usize;
    raw.wrapping_add(up + skew)
}

struct Blk {
    p: *mut u8,
    l: Layout,
    tag: u8,
}

fn fill(b: &Blk) {
    // SAFETY: `p` is a live block of `l.size()` bytes owned by this test.
    unsafe { core::ptr::write_bytes(b.p, b.tag, b.l.size()) };
}

fn intact(b: &Blk) -> bool {
    // SAFETY: as in `fill`.
    unsafe { core::slice::from_raw_parts(b.p, b.l.size()) }
        .iter()
        .all(|&x| x == b.tag)
}

#[test]
fn leader_big_alignments_are_absolute() {
    let mut big = 0usize;
    forall(
        env("STRATA_CASES", 40) as u32,
        env("STRATA_SEED", 0x16a1_2026_1114),
        |r| {
            // Odd page offsets, sometimes not even page aligned.
            let skew = r.below(97) * PAGE + if r.chance(1, 3) { r.below(PAGE) } else { 0 };
            let len = 24 << 20;
            let base = arena(len, skew);
            // SAFETY: the leaked arena is ours alone for the rest of the process.
            let mut h = unsafe { Heap::new(base, len) }.unwrap();
            let mut live: Vec<Blk> = Vec::new();
            for step in 0..400 {
                if live.is_empty() || (live.len() < 40 && r.chance(3, 5)) {
                    let align = 1usize << r.below(23); // up to 4 MiB
                    let size = r.range(1, (1 << 20) + 1) as usize;
                    let l = Layout::from_size_align(size, align).unwrap();
                    let p = h.alloc(l);
                    if p.is_null() {
                        continue;
                    }
                    assert_eq!(
                        p as usize % align,
                        0,
                        "align {align} size {size} skew {skew}"
                    );
                    let (s, e) = (p as usize, p as usize + size);
                    assert!(
                        s >= base as usize && e <= base as usize + len,
                        "outside the arena"
                    );
                    for o in &live {
                        let (os, oe) = (o.p as usize, o.p as usize + o.l.size());
                        assert!(e <= os || s >= oe, "overlap");
                    }
                    if align > PAGE {
                        big += 1;
                    }
                    let b = Blk {
                        p,
                        l,
                        tag: (r.u64() as u8) | 1,
                    };
                    fill(&b);
                    live.push(b);
                } else {
                    let b = live.swap_remove(r.below(live.len()));
                    assert!(intact(&b), "tag overwritten");
                    // SAFETY: from `alloc` with this layout, freed once.
                    unsafe { h.dealloc(b.p, b.l) };
                }
                if step % 50 == 0 {
                    assert!(h.check(), "check failed at step {step}");
                }
            }
            for b in live.drain(..) {
                assert!(intact(&b));
                // SAFETY: as above.
                unsafe { h.dealloc(b.p, b.l) };
            }
            assert!(h.check());
            assert_eq!(h.used_bytes(), 0);
            // After a full free, a 4 MiB block at 4 MiB alignment must fit again.
            let l = Layout::from_size_align(MAX_ALIGN, MAX_ALIGN).unwrap();
            let p = h.alloc(l);
            assert!(
                !p.is_null() && (p as usize).is_multiple_of(MAX_ALIGN),
                "no 4 MiB block after full free"
            );
            // SAFETY: just allocated with this layout.
            unsafe { h.dealloc(p, l) };
            let over = Layout::from_size_align(PAGE, 2 * MAX_ALIGN).unwrap();
            assert!(
                h.alloc(over).is_null(),
                "alignment above MAX_ALIGN must be refused"
            );
            let huge = Layout::from_size_align(MAX_ALIGN + 1, 8).unwrap();
            assert!(
                h.alloc(huge).is_null(),
                "size above the largest block must be refused"
            );
            assert_eq!(h.used_bytes(), 0);
        },
    );
    eprintln!("{big} allocations with alignment above a page");
    assert!(big > 1000, "too few big-alignment allocations: {big}");
}

static STATIC_HEAP: Locked<Heap> = Locked::new(Heap::empty());

#[test]
fn leader_empty_then_init() {
    let b = Buddy::empty();
    assert_eq!((b.free_bytes(), b.total_bytes()), (0, 0));
    assert!(b.check());
    let mut b = b;
    assert!(b.alloc_pages(0).is_none());

    let mut h = Heap::empty();
    assert!(!h.is_ready() && h.check() && h.total_bytes() == 0);
    assert!(h.alloc(Layout::from_size_align(8, 8).unwrap()).is_null());
    let z = Layout::from_size_align(0, 64).unwrap();
    let zp = h.alloc(z);
    assert!(
        !zp.is_null() && (zp as usize).is_multiple_of(64),
        "zero size gets a dangling aligned pointer"
    );
    // SAFETY: zero-size deallocation is ignored.
    unsafe { h.dealloc(zp, z) };
    let tiny = arena(PAGE, 0);
    // SAFETY: leaked arena.
    assert!(
        !unsafe { h.init(tiny, 100) },
        "a 100-byte region is too small"
    );
    assert!(!h.is_ready());

    let base = arena(8 << 20, 3 * PAGE);
    // SAFETY: leaked arena, ours alone.
    assert!(unsafe { STATIC_HEAP.init(base, 8 << 20) });
    let total = STATIC_HEAP.lock().total_bytes();
    assert!(total > (7 << 20) && total <= 8 << 20, "total {total}");
    let other = arena(8 << 20, 0);
    // SAFETY: leaked arena.
    assert!(
        !unsafe { STATIC_HEAP.init(other, 8 << 20) },
        "second init must fail"
    );
    let mut g = STATIC_HEAP.lock();
    assert_eq!(g.total_bytes(), total, "second init changed the heap");
    let l = Layout::from_size_align(3000, 16).unwrap();
    let p = g.alloc(l);
    assert!(p as usize >= base as usize && (p as usize) < base as usize + (8 << 20));
    // SAFETY: just allocated.
    unsafe { g.dealloc(p, l) };
    assert!(g.check() && g.used_bytes() == 0);
}

/// Heap over a fresh arena, then a churn whose freed blocks are returned.
fn churned(r: &mut Rng, len: usize) -> (Heap, Vec<(usize, usize)>, Vec<Blk>) {
    let base = arena(len, r.below(5) * PAGE);
    // SAFETY: leaked arena.
    let mut h = unsafe { Heap::new(base, len) }.unwrap();
    let (mut live, mut freed) = (Vec::new(), Vec::new());
    for _ in 0..300 {
        if live.is_empty() || r.chance(3, 5) {
            let size = r.range(8, 20_000) as usize;
            let l = Layout::from_size_align(size, 1 << r.below(10)).unwrap();
            let p = h.alloc(l);
            if !p.is_null() {
                live.push(Blk { p, l, tag: 1 });
            }
        } else {
            let b: Blk = live.swap_remove(r.below(live.len()));
            freed.push((b.p as usize, b.l.size()));
            // SAFETY: from `alloc`, freed once.
            unsafe { h.dealloc(b.p, b.l) };
        }
    }
    (h, freed, live)
}

#[test]
fn leader_targeted_corruption_is_reported() {
    let base = arena(4 << 20, 0);
    // SAFETY: leaked arena.
    let mut h = unsafe { Heap::new(base, 4 << 20) }.unwrap();
    let l = Layout::from_size_align(64, 8).unwrap();
    let (a, b) = (h.alloc(l), h.alloc(l));
    // SAFETY: both from `alloc`; `a` is freed, so its first word is now a free-list link.
    unsafe { h.dealloc(a, l) };
    assert!(h.check());
    let link = a.cast::<usize>();
    // SAFETY: `a` points into our own arena; the heap reads this word through its own pointer.
    let saved = unsafe { link.read_unaligned() };
    for bad in [
        usize::MAX & !7,
        0x10,
        a as usize + 1,
        a as usize,
        base as usize + 8,
    ] {
        // SAFETY: as above.
        unsafe { link.write_unaligned(bad) };
        assert!(!h.check(), "bad free-slot link {bad:#x} not reported");
    }
    // SAFETY: as above.
    unsafe { link.write_unaligned(saved) };
    assert!(
        h.check(),
        "restoring the link must make the heap sound again"
    );
    // SAFETY: `b` is live.
    unsafe { h.dealloc(b, l) };

    // Smash the first 8 bytes of every page: free-list links and the order table both go bad.
    assert!(h.check());
    for i in 0..(4 << 20) / PAGE {
        // SAFETY: inside our arena.
        unsafe { core::ptr::write_bytes(base.add(i * PAGE), 0xab, 8) };
    }
    assert!(!h.check(), "smashed page headers not reported");
}

#[test]
fn leader_random_corruption_never_panics() {
    let mut caught = 0u32;
    let cases = env("STRATA_CASES", 300) as u32;
    forall(cases, env("STRATA_SEED", 0xc0_2026_1114), |r| {
        let (h, freed, _live) = churned(r, 2 << 20);
        if freed.is_empty() {
            return;
        }
        for _ in 0..r.range(1, 6) {
            let (p, n) = *r.pick(&freed);
            let off = r.below(n.saturating_sub(8).max(1)) & !7;
            let word = match r.below(4) {
                0 => r.u64(),
                1 => p as u64 + r.below(1 << 16) as u64,
                2 => r.u64() & 0xffff_ffff,
                _ => u64::MAX,
            };
            // SAFETY: inside a block we freed; the arena belongs to this test.
            unsafe { ((p + off) as *mut u64).write_unaligned(word) };
        }
        if !h.check() {
            caught += 1;
        }
    });
    eprintln!("check() reported {caught} of {cases} randomly corrupted heaps");
    assert!(caught > 0, "random corruption was never reported");
}
