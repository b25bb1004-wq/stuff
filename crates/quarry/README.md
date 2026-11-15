# quarry

quarry is a memory allocator for code that owns one fixed region of memory and never asks the
operating system for more. You give it a byte range; it hands out blocks from that range and takes
them back. It is part of Strata and has no external crates.

It is built in two layers: a buddy allocator that manages whole 4096-byte pages, and a slab heap on
top that carves small objects out of those pages. Nothing in the crate calls the OS.

## Features

- Buddy allocator over one caller-supplied region, in 4096-byte pages.
- Blocks of 2^order pages, up to 4 MiB, aligned to their own size.
- Slab heap with eight size classes, 16 to 2048 bytes, for small requests.
- Requests above 2048 bytes, or whose alignment is larger than their class, go straight to the buddy.
  Alignment is capped at 4 MiB; larger requests return null.
- Zero-size layouts return an aligned dangling pointer and free nothing.
- `check()` walks every free list and reports false if an invariant is broken.
- `Locked<T>` is a spin lock that lets a `Heap` serve as a `GlobalAlloc`.
- No dependencies outside the standard library.

## Public API

| Item | Purpose |
| --- | --- |
| `Heap::empty`, `Heap::new` (unsafe), `Heap::init` (unsafe) | Create a heap, or give an empty one its region |
| `Heap::alloc`, `Heap::dealloc` (unsafe) | Allocate for a `Layout`, free with the same layout |
| `Heap::used_bytes`, `Heap::total_bytes`, `Heap::is_ready`, `Heap::check` | Usage, capacity, state and invariant check |
| `Buddy::empty`, `Buddy::new` (unsafe) | Page-level allocator over a region |
| `Buddy::alloc_pages`, `Buddy::free_pages` (unsafe) | Take and return `2^order` pages |
| `Buddy::free_bytes`, `Buddy::total_bytes`, `Buddy::check` | Free bytes, capacity and invariant check |
| `Locked::new`, `Locked::lock`, `Locked::init` (unsafe) | Spin lock around a value; `init` for `Locked<Heap>` |
| `PAGE`, `MAX_ALIGN` | Page size (4096) and largest block and alignment (4 MiB) |

The unsafe functions require that the region stays valid, writable and unused for as long as the
allocator lives, and that frees match an earlier allocation.

## Demo

Run the demo with:

```
cargo run -p quarry --example demo
```

Real output:

```
page size 4096 bytes, largest block 4194304 bytes
heap ready: true, usable bytes: 1040384
invariants hold at start: true
small alloc (100 bytes): null=false, used=128
large alloc (64 KiB): null=false, used=65664
after freeing both: used=0, invariants hold: true
buddy: free before 253952, got block: true, free after 237568
buddy: free after giving it back 253952, check true
locked heap: init true, alloc null=false
locked heap: used after free 0
```

The demo uses two 256 KiB and one 1 MiB arena of zeroed `Vec<u8>`, so it needs no OS memory
beyond those.

## Tests

```
cargo test -p quarry
```

Unit tests sit beside the code, and integration tests in `crates/quarry/tests` cover alignment,
corruption detection, global-allocator use, threads and a soak run. The gate is
`bash tools/check.sh quarry`.
