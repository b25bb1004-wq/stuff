//! Bookkeeping for the buffers `neon_alloc` hands to the guest. Safe code only, so it builds and
//! tests on the host; the raw-pointer reads and frees live in `wasm.rs`.

/// One buffer handed to the guest: its address, the length the guest asked for, and the capacity
/// actually allocated (the capacity is what `Vec::from_raw_parts` must be given to free it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    /// Start address of the buffer.
    pub ptr: usize,
    /// Length the guest requested.
    pub len: usize,
    /// Capacity of the underlying allocation.
    pub cap: usize,
}

/// Buffers that are alive, looked up by exact `(ptr, len)`.
#[derive(Debug, Default)]
pub struct LiveTable {
    entries: Vec<Live>,
}

impl LiveTable {
    /// An empty table.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Records a buffer that `neon_alloc` just handed out.
    pub fn record(&mut self, live: Live) {
        self.entries.push(live);
    }

    /// Whether `(ptr, len)` is exactly a live buffer.
    pub fn contains(&self, ptr: usize, len: usize) -> bool {
        self.entries.iter().any(|e| e.ptr == ptr && e.len == len)
    }

    /// Removes and returns the buffer matching `(ptr, len)` exactly, so a second free is ignored.
    pub fn take(&mut self, ptr: usize, len: usize) -> Option<Live> {
        let i = self
            .entries
            .iter()
            .position(|e| e.ptr == ptr && e.len == len)?;
        Some(self.entries.swap_remove(i))
    }
}

/// Reserves `len` bytes without aborting on failure. Returns `(address, capacity)`, or `None` when
/// the allocation cannot be made. The buffer is leaked on purpose; free it through [`LiveTable::take`].
pub fn reserve(len: usize) -> Option<(usize, usize)> {
    let mut buf = Vec::<u8>::new();
    buf.try_reserve_exact(len).ok()?;
    let out = (buf.as_mut_ptr() as usize, buf.capacity());
    core::mem::forget(buf);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_request_fails_without_aborting() {
        assert!(reserve(usize::MAX).is_none());
    }

    #[test]
    fn only_exact_live_pairs_are_accepted() {
        let mut t = LiveTable::new();
        t.record(Live {
            ptr: 0x1000,
            len: 64,
            cap: 64,
        });
        // Hostile pairs from the report: a guest pointer the host never handed out, and a real
        // address with the wrong length.
        assert!(!t.contains(0x10, 1000));
        assert!(t.take(0x1000, 16).is_none());
        assert!(t.contains(0x1000, 64));
        assert_eq!(t.take(0x1000, 64).map(|l| l.cap), Some(64));
        assert!(t.take(0x1000, 64).is_none());
    }
}
