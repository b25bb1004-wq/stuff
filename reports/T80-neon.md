# T80 security review: neon (wasm exports and session wrapper)
Method: code reading only, nothing executed. Sampled: src/wasm.rs (lines 14-56), src/session.rs boot path (36-55), unsafe grep. Not read: bin/titan.rs, playground.rs, pack.rs. Threat model: wasm exports are callable by any script on the page.
HIGH
- wasm.rs:27 (copy_slice_from_guest): from_raw_parts(ptr, len) on a pointer and length supplied by the JS caller. The SAFETY comment assumes they came from neon_alloc, and nothing enforces it. Input: ptr=0x10 (non-null), len=1000 from JS. Result: UB, reads arbitrary linear memory. Fix: keep buffers in an internal table keyed by handle, not raw pointers.
- wasm.rs:49-50 (neon_free): Vec::from_raw_parts(ptr, 0, len) for any (ptr, len). Input: neon_free(pointer of another buffer, 16). Result: invalid dealloc and heap corruption. Fix: same handle table, or reject unknown (ptr, len) pairs.
MEDIUM
- wasm.rs:36 (neon_alloc): Vec::with_capacity(len) panics or aborts on a huge len instead of returning null. Input: neon_alloc(usize::MAX). Fix: try_reserve_exact and return null on error.
LOW: none
No finding: session.rs:42-43 validates the disk and maps elf::read errors to Err instead of panicking.
