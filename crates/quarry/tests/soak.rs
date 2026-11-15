//! Soak run for the Buddy and Heap allocators.
//!
//! Every target runs randomized cases until `STRATA_SOAK_MS` (default 300) milliseconds have
//! passed. Seed: `STRATA_SOAK_SEED`, decimal or `0x` hex, default `0x05EE_D7A2`.
#![allow(clippy::unwrap_used)]

use assay::Rng;
use core::alloc::Layout;
use core::ptr::{self, NonNull};
use quarry::{Buddy, Heap, MAX_ALIGN, PAGE};
use std::thread;
use std::time::{Duration, Instant};

/// Largest buddy order, computed from the public constants (2^10 pages = `MAX_ALIGN`).
const MAX_ORDER: u32 = (MAX_ALIGN / PAGE).trailing_zeros();
/// Upper bound on any single request, per the soak contract.
const MEM_CAP: usize = 64 << 20;
/// Wall-clock ceiling per case.
const CASE_LIMIT: Duration = Duration::from_secs(1);
/// Live blocks kept per case, so the model checks stay cheap.
const LIVE_CAP: usize = 200;
/// Regions this large always hold an aligned largest block, so a full-merge probe must succeed.
const PROBE_LEN: usize = 16 << 20;
/// Byte stride for tag stamps; keeps 8 MiB blocks cheap to stamp and verify.
const STRIDE: usize = 251;
/// Longest script in bytes (about 1200 operations).
const MAX_SCRIPT: usize = 4 * 1200;

fn env_u64(name: &str, default: u64) -> u64 {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    let t = raw.trim();
    let parsed = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16),
        None => t.parse::<u64>(),
    };
    parsed.unwrap_or_else(|_| panic!("{name} is not a u64: {raw:?}"))
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Prints the failing case's seed when the case panics.
struct Replay {
    name: &'static str,
    case_seed: u64,
}

impl Drop for Replay {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!(
                "soak: {} failed; replay seed {:#018x}",
                self.name, self.case_seed
            );
        }
    }
}

/// Run `case` on fresh random streams until the per-target budget is spent.
fn soak(name: &'static str, mut case: impl FnMut(&mut Rng)) {
    let seed = env_u64("STRATA_SOAK_SEED", 0x05EE_D7A2);
    let budget = Duration::from_millis(env_u64("STRATA_SOAK_MS", 300));
    let mut master = Rng::new(seed ^ fnv(name));
    let start = Instant::now();
    let mut cases = 0u64;
    loop {
        let case_seed = master.u64();
        let _guard = Replay { name, case_seed };
        let t = Instant::now();
        case(&mut Rng::new(case_seed));
        let took = t.elapsed();
        assert!(
            took < CASE_LIMIT,
            "soak: {name} case {case_seed:#x} took {took:?}"
        );
        cases += 1;
        if start.elapsed() >= budget {
            break;
        }
    }
    eprintln!(
        "soak: {name} seed={seed:#x} budget_ms={} cases={cases}",
        budget.as_millis()
    );
}

/// A zeroed arena, a random misalignment of the region start, and the region length.
fn make_arena(rng: &mut Rng) -> (Vec<u8>, usize, usize) {
    let len = *rng.pick(&[64 << 10, 256 << 10, 1 << 20, 4 << 20, PROBE_LEN]);
    let off = rng.below(PAGE);
    (vec![0u8; off + len], off, len)
}

/// Random bytes or printable text, up to `MAX_SCRIPT` long.
fn random_script(rng: &mut Rng) -> Vec<u8> {
    if rng.chance(1, 2) {
        rng.bytes(MAX_SCRIPT)
    } else {
        (0..rng.below(MAX_SCRIPT + 1))
            .map(|_| b' ' + rng.below(95) as u8)
            .collect()
    }
}

/// Well-formed looking scripts: small size selectors and alignments, as a real caller sends.
fn valid_script(rng: &mut Rng) -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..8 + rng.below(400) {
        v.extend([
            rng.u64() as u8,
            rng.u64() as u8,
            rng.below(8) as u8,
            rng.below(14) as u8,
        ]);
    }
    v
}

/// Bit flips, truncation, chunk splices and repeated chunks applied to a valid script.
fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..1 + rng.below(6) {
        if v.is_empty() {
            v.push(rng.u64() as u8);
            continue;
        }
        match rng.below(5) {
            0 => {
                let i = rng.below(v.len());
                v[i] ^= 1 << rng.below(8);
            }
            1 => {
                let n = rng.below(v.len() + 1);
                v.truncate(n);
            }
            2 | 3 => {
                let from = rng.below(v.len());
                let len = rng.below((v.len() - from).min(64) + 1);
                let chunk = v[from..from + len].to_vec();
                let at = rng.below(v.len() + 1);
                let times = if rng.chance(1, 2) {
                    1
                } else {
                    1 + rng.below(8)
                };
                v.splice(at..at, chunk.repeat(times));
            }
            _ => {
                let i = rng.below(v.len());
                v[i] = rng.u64() as u8;
            }
        }
        v.truncate(MAX_SCRIPT);
    }
    v
}

/// Half random or text bytes, half mutations of a valid script.
fn script(rng: &mut Rng) -> Vec<u8> {
    if rng.chance(1, 2) {
        random_script(rng)
    } else {
        let base = valid_script(rng);
        mutate(rng, &base)
    }
}

/// A size from byte `x`, shifted left by a amount taken from byte `s` (at most 8 MiB).
fn sized(x: u8, s: u8) -> usize {
    (x as usize + 1) << (s % 16)
}

#[derive(Clone, Copy)]
enum HOp {
    Alloc { size: usize, align: usize },
    Free(usize),
    Realloc { pick: usize, size: usize },
    Check,
}

/// Decode 4-byte groups into heap operations. Every byte string decodes.
fn decode_heap(bytes: &[u8]) -> Vec<HOp> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| {
            let (k, x, y, z) = (c[0], c[1], c[2], c[3]);
            match k % 8 {
                0..=3 => HOp::Alloc {
                    size: sized(x, y),
                    align: 1 << (z % 24),
                },
                4 | 5 => HOp::Free(x as usize),
                6 => HOp::Realloc {
                    pick: x as usize,
                    size: sized(y, z),
                },
                // Over-aligned (above MAX_ALIGN) requests, which must return null.
                _ if x % 2 == 0 => HOp::Alloc {
                    size: 64 * (y as usize + 1),
                    align: (MAX_ALIGN * 2) << (z % 2),
                },
                _ => HOp::Check,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
enum BOp {
    Alloc(u32),
    Free(usize),
    Check,
}

/// Decode 4-byte groups into buddy operations. Orders above `MAX_ORDER` must return `None`.
fn decode_buddy(bytes: &[u8]) -> Vec<BOp> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| match c[0] % 4 {
            0 | 1 => BOp::Alloc(u32::from(c[2] % 13)),
            2 => BOp::Free(c[1] as usize),
            _ => BOp::Check,
        })
        .collect()
}

/// A block the model believes is live: its layout and the byte it was stamped with.
struct Blk {
    ptr: *mut u8,
    layout: Layout,
    tag: u8,
}

impl Blk {
    fn start(&self) -> usize {
        self.ptr as usize
    }
    fn end(&self) -> usize {
        self.ptr as usize + self.layout.size()
    }
}

fn tag_at(step: usize) -> u8 {
    (step as u8).wrapping_mul(89) | 1
}

/// Byte offsets sampled by `stamp` and `verify`: a stride plus the last byte.
fn sample(size: usize) -> impl Iterator<Item = usize> {
    (0..size).step_by(STRIDE).chain(size.checked_sub(1))
}

fn stamp(p: *mut u8, size: usize, tag: u8) {
    for i in sample(size) {
        // SAFETY: `i < size` and the caller owns the `size` bytes at `p`.
        unsafe { p.add(i).write(tag) };
    }
}

fn verify_at(p: *const u8, offsets: impl Iterator<Item = usize>, tag: u8) {
    for i in offsets {
        // SAFETY: every offset lies inside a block the model owns, so the byte is readable.
        let got = unsafe { p.add(i).read() };
        assert_eq!(got, tag, "tag byte lost at offset {i}");
    }
}

fn verify(b: &Blk) {
    verify_at(b.ptr, sample(b.layout.size()), b.tag);
}

/// Assert a fresh block is aligned and disjoint from every block in `others`.
fn fresh(p: *mut u8, layout: Layout, others: &[Blk]) {
    let (s, e) = (p as usize, p as usize + layout.size());
    assert_eq!(s % layout.align(), 0, "misaligned block for {layout:?}");
    assert!(
        others.iter().all(|b| e <= b.start() || b.end() <= s),
        "overlap at {s:#x}..{e:#x}"
    );
}

fn alloc_into(heap: &mut Heap, size: usize, align: usize, tag: u8, live: &mut Vec<Blk>) {
    assert!(size <= MEM_CAP, "request of {size} bytes exceeds the cap");
    let layout = Layout::from_size_align(size, align).unwrap();
    let p = heap.alloc(layout);
    if align > MAX_ALIGN {
        assert!(
            p.is_null(),
            "over-aligned request ({align}) returned memory"
        );
        return;
    }
    if p.is_null() {
        return;
    }
    fresh(p, layout, live);
    stamp(p, size, tag);
    live.push(Blk {
        ptr: p,
        layout,
        tag,
    });
}

/// Run heap operations against the model. With `finish`, frees everything and checks the
/// heap returns to empty; otherwise the live blocks are handed back and left allocated.
fn run_heap(heap: &mut Heap, ops: &[HOp], finish: bool, probe: bool) -> Vec<Blk> {
    let mut live: Vec<Blk> = Vec::new();
    for (step, &op) in ops.iter().enumerate() {
        let tag = tag_at(step);
        match op {
            HOp::Alloc { size, align } => {
                if live.len() < LIVE_CAP {
                    alloc_into(heap, size, align, tag, &mut live);
                }
            }
            HOp::Free(pick) => {
                if !live.is_empty() {
                    let b = live.swap_remove(pick % live.len());
                    verify(&b);
                    // SAFETY: `b` came from `heap.alloc` with `b.layout`, left the model, and is freed once.
                    unsafe { heap.dealloc(b.ptr, b.layout) };
                }
            }
            HOp::Realloc { pick, size } => {
                if !live.is_empty() && live.len() <= LIVE_CAP {
                    let old = live.swap_remove(pick % live.len());
                    verify(&old);
                    let nl = Layout::from_size_align(size, old.layout.align()).unwrap();
                    let p = heap.alloc(nl);
                    if p.is_null() {
                        live.push(old);
                    } else {
                        fresh(p, nl, &live);
                        fresh(p, nl, std::slice::from_ref(&old));
                        let keep = old.layout.size().min(size);
                        // SAFETY: the new block is disjoint from the old one and both hold `keep` bytes.
                        unsafe { ptr::copy_nonoverlapping(old.ptr, p, keep) };
                        verify_at(p, sample(old.layout.size()).filter(|&i| i < keep), old.tag);
                        stamp(p, size, tag);
                        // SAFETY: `old` came from `heap.alloc` with `old.layout`, left the model, and is freed once.
                        unsafe { heap.dealloc(old.ptr, old.layout) };
                        live.push(Blk {
                            ptr: p,
                            layout: nl,
                            tag,
                        });
                    }
                }
            }
            HOp::Check => assert!(heap.check(), "heap check failed at step {step}"),
        }
        let sum: usize = live.iter().map(|b| b.layout.size()).sum();
        assert!(
            heap.used_bytes() >= sum,
            "used {} below live {sum} at step {step}",
            heap.used_bytes()
        );
        if step % 16 == 15 {
            assert!(heap.check(), "heap check failed at step {step}");
        }
    }
    if !finish {
        return live;
    }
    while let Some(b) = live.pop() {
        verify(&b);
        // SAFETY: `b` came from `heap.alloc` with `b.layout`, left the model, and is freed once.
        unsafe { heap.dealloc(b.ptr, b.layout) };
    }
    assert_eq!(
        heap.used_bytes(),
        0,
        "bytes still counted as used after freeing everything"
    );
    assert!(heap.check(), "heap check failed after freeing everything");
    if probe {
        let big = Layout::from_size_align(MAX_ALIGN, PAGE).unwrap();
        let p = heap.alloc(big);
        assert!(!p.is_null(), "no max-size block after freeing everything");
        // SAFETY: `p` came from `heap.alloc` with `big` and is freed once.
        unsafe { heap.dealloc(p, big) };
        assert!(heap.check());
    }
    Vec::new()
}

/// Run buddy operations against the model; same `finish` and `probe` meaning as `run_heap`.
fn run_buddy(b: &mut Buddy, ops: &[BOp], finish: bool, probe: bool) -> Vec<Blk> {
    let total = b.total_bytes();
    let mut live: Vec<Blk> = Vec::new();
    for (step, &op) in ops.iter().enumerate() {
        let tag = tag_at(step);
        match op {
            BOp::Alloc(order) => {
                if order > MAX_ORDER {
                    assert!(b.alloc_pages(order).is_none(), "order {order} accepted");
                } else if live.len() < LIVE_CAP {
                    if let Some(p) = b.alloc_pages(order) {
                        let size = PAGE << order;
                        let layout = Layout::from_size_align(size, size).unwrap();
                        fresh(p.as_ptr(), layout, &live);
                        stamp(p.as_ptr(), size, tag);
                        live.push(Blk {
                            ptr: p.as_ptr(),
                            layout,
                            tag,
                        });
                    }
                }
            }
            BOp::Free(pick) => {
                if !live.is_empty() {
                    let blk = live.swap_remove(pick % live.len());
                    verify(&blk);
                    let order = (blk.layout.size() / PAGE).trailing_zeros();
                    let p = NonNull::new(blk.ptr).unwrap();
                    // SAFETY: `p` came from `alloc_pages(order)`, left the model, and is freed once.
                    unsafe { b.free_pages(p, order) };
                }
            }
            BOp::Check => assert!(b.check(), "buddy check failed at step {step}"),
        }
        let sum: usize = live.iter().map(|x| x.layout.size()).sum();
        assert_eq!(
            b.free_bytes() + sum,
            total,
            "free accounting off at step {step}"
        );
        if step % 16 == 15 {
            assert!(b.check(), "buddy check failed at step {step}");
        }
    }
    if !finish {
        return live;
    }
    while let Some(blk) = live.pop() {
        verify(&blk);
        let order = (blk.layout.size() / PAGE).trailing_zeros();
        let p = NonNull::new(blk.ptr).unwrap();
        // SAFETY: `p` came from `alloc_pages(order)`, left the model, and is freed once.
        unsafe { b.free_pages(p, order) };
    }
    assert_eq!(
        b.free_bytes(),
        total,
        "memory not back after freeing everything"
    );
    assert!(b.check(), "buddy check failed after freeing everything");
    if probe {
        let p = b
            .alloc_pages(MAX_ORDER)
            .expect("no max-size block after freeing everything");
        // SAFETY: `p` came from `alloc_pages(MAX_ORDER)` and is freed once.
        unsafe { b.free_pages(p, MAX_ORDER) };
        assert!(b.check());
    }
    Vec::new()
}

/// Scribble random bytes over a region, half of the hits in the first 256 bytes (the order table).
fn corrupt(rng: &mut Rng, base: *mut u8, len: usize) {
    for _ in 0..1 + rng.below(64) {
        let span = if rng.chance(1, 2) {
            usize::min(256, len - 8)
        } else {
            len - 8
        };
        let off = rng.below(span);
        let mut buf = [0u8; 8];
        rng.fill(&mut buf);
        let n = if rng.chance(1, 2) { 1 } else { 8 };
        // SAFETY: `off + n <= len`, so the write stays inside the region the test owns.
        unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), base.add(off), n) };
    }
}

#[test]
fn soak_buddy() {
    soak("buddy", |rng| {
        let (mut arena, off, len) = make_arena(rng);
        let base = arena.as_mut_ptr().wrapping_add(off);
        // SAFETY: the arena outlives the buddy, and nothing else touches the region.
        let mut buddy = unsafe { Buddy::new(base, len) }.expect("region holds pages");
        let ops = decode_buddy(&script(rng));
        run_buddy(&mut buddy, &ops, true, len >= PROBE_LEN);
    });
}

#[test]
fn soak_heap() {
    soak("heap", |rng| {
        let (mut arena, off, len) = make_arena(rng);
        let base = arena.as_mut_ptr().wrapping_add(off);
        // SAFETY: the arena outlives the heap, and nothing else touches the region.
        let mut heap = unsafe { Heap::new(base, len) }.expect("region holds pages");
        let ops = decode_heap(&script(rng));
        run_heap(&mut heap, &ops, true, len >= PROBE_LEN);
    });
}

#[test]
fn soak_corrupt_metadata() {
    soak("corrupt", |rng| {
        let (mut arena, off, len) = make_arena(rng);
        let base = arena.as_mut_ptr().wrapping_add(off);
        let ops = script(rng);
        if rng.chance(1, 2) {
            // SAFETY: the arena outlives the heap, and nothing else touches the region.
            let mut heap = unsafe { Heap::new(base, len) }.expect("region holds pages");
            let _live = run_heap(&mut heap, &decode_heap(&ops), false, false);
            assert!(
                heap.check(),
                "valid heap failed its check before corruption"
            );
            corrupt(rng, base, len);
            // Results may be true or false; the property is that none of these panic.
            let _ = (
                heap.check(),
                heap.used_bytes(),
                heap.total_bytes(),
                heap.is_ready(),
            );
        } else {
            // SAFETY: the arena outlives the buddy, and nothing else touches the region.
            let mut buddy = unsafe { Buddy::new(base, len) }.expect("region holds pages");
            let _live = run_buddy(&mut buddy, &decode_buddy(&ops), false, false);
            assert!(
                buddy.check(),
                "valid buddy failed its check before corruption"
            );
            corrupt(rng, base, len);
            let _ = (buddy.check(), buddy.free_bytes(), buddy.total_bytes());
        }
    });
}
