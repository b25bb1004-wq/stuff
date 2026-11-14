//! quarry: buddy page allocator, size-class slab heap and a spin-locked `GlobalAlloc` wrapper.
//!
//! Everything works inside one caller-supplied memory region; nothing here asks the OS for memory.
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

mod buddy;
mod heap;
mod lock;
#[cfg(test)]
mod testutil;

pub use buddy::Buddy;
pub use heap::Heap;
pub use lock::Locked;

/// Page size in bytes: the smallest block the buddy allocator hands out.
pub const PAGE: usize = 4096;

/// Largest supported alignment and largest buddy block (4 MiB); bigger requests return null.
pub const MAX_ALIGN: usize = PAGE << 10;
