# titan

titan is the Rust kernel for the Silicon machine. This crate's library holds the
memory and trap logic that the kernel is built from, and it can be tested on a
normal host. The library is `no_std` (with `alloc`) outside of tests and forbids
`unsafe` code. This README covers the host-side modules only: `frames`, `sv39`
and `trap`. The kernel binary is not part of this demo.

Keeping page-table and frame logic in a library means it can be tested and run
on the host, outside the kernel.

## Features

- Bitmap physical frame allocator for 4 KiB frames, up to `MAX_FRAMES` (131 072).
  It rejects misaligned and out-of-range frees and reports double frees.
- Sv39 three-level page tables: `map`, `unmap` (which frees intermediate tables
  that become empty), and `translate`.
- Leaf flag checks: a leaf needs R or X, and W without R is rejected.
- `FramePool`, a host-memory frame source that implements the `FrameAlloc` trait
  so page tables can run without hardware.
- `satp` value for Sv39 with ASID 0.
- `scause` decoding for interrupts and exceptions, with a display name for each
  cause and a page-fault check.

## Public API

| Item | Purpose |
| --- | --- |
| `frames::FrameAllocator` | Bitmap allocator: `new`, `init`, `alloc`, `free`, counts, `is_allocated`. |
| `frames::Error` | `Misaligned`, `OutOfRange`, `DoubleFree`. |
| `sv39::PageTable<A>` | `new`, `from_root`, `root`, `satp`, `map`, `unmap`, `translate`, allocator accessors. |
| `sv39::FrameAlloc` | Trait: `alloc`, `free`, `read_pte`, `write_pte`. |
| `sv39::FramePool` | Host frame pool: `new(base, count)`, `free_count`, `used_count`, `total_count`. |
| `sv39::Pte` | PTE bits (`V R W X U G A D`), `new`, `pa`, `ppn`, `flags`, `is_leaf`. |
| `sv39::Error` | `MisalignedAddress`, `InvalidAddress`, `InvalidFlags`, `OutOfMemory`, `NotMapped`, `SuperpageConflict`, `FrameFree`. |
| `trap::Cause` | `decode` (or `from`), `code`, `scause`, `is_interrupt`, `is_exception`, `is_page_fault`. |

## Demo

```
cargo run -p titan --example demo
```

Output (program stdout):

```
frame pool: 16 frames, 16 free
root table at 0x80000000, satp 0x8000000000080000
frames in use after root: 1
map 0x400000 -> 0x80100000 ok, flags 0x06
map 0x401000 -> 0x80101000 ok, flags 0x0a
map 0x10000000 -> 0x80200000 ok, flags 0x06
frames in use after 3 maps: 4
misaligned map rejected: address is not 4 KiB aligned
write-only map rejected: invalid Sv39 leaf PTE flags
translate 0x400000 -> 0x80100000 flags 0x07
translate 0x401000 -> 0x80101000 flags 0x0b
translate 0x700000 -> not mapped
unmapped 0x10000000, was 0x80200000
frames in use after unmap: 3
scause 0xd: load page fault (page fault: true)
scause 0xf: store/AMO page fault (page fault: true)
scause 0x2: illegal instruction (page fault: false)
scause 0x8000000000000005: supervisor timer interrupt (page fault: false)
scause 0x2a: unknown exception (42) (page fault: false)
```

The count of four is the root table, one middle table, and one leaf table for each
of the two regions (`0x400000` and `0x10000000`). Unmapping `0x10000000` empties its
leaf table, which is freed, so the count drops to three.

## Tests

```
cargo test -p titan
```
