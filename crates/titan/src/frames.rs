//! Bitmap physical page-frame allocator over a contiguous 4 KiB-aligned physical address range.

use core::fmt;

/// Size of one physical page frame in bytes (4 KiB).
pub const PAGE_SIZE: u64 = 4096;

/// Maximum number of 4 KiB frames tracked by a [`FrameAllocator`] (512 MiB of frames).
pub const MAX_FRAMES: usize = 131_072;

const WORDS: usize = MAX_FRAMES / 64;

/// Errors returned by [`FrameAllocator::free`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Physical address is not aligned to [`PAGE_SIZE`].
    Misaligned(u64),
    /// Physical address falls outside the managed physical range.
    OutOfRange(u64),
    /// Physical frame is already marked free (double free or never allocated).
    DoubleFree(u64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Misaligned(pa) => write!(f, "misaligned frame address {pa:#x}"),
            Self::OutOfRange(pa) => write!(f, "frame address {pa:#x} out of range"),
            Self::DoubleFree(pa) => write!(f, "double free of frame {pa:#x}"),
        }
    }
}

/// Bitmap allocator managing 4 KiB physical frames in `[start, end)`.
pub struct FrameAllocator {
    base: u64,
    total: usize,
    free: usize,
    hint: usize,
    bits: [u64; WORDS],
}

/// Type alias for [`FrameAllocator`].
pub type Bitmap = FrameAllocator;

/// Type alias for [`FrameAllocator`].
pub type Frames = FrameAllocator;

impl fmt::Debug for FrameAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameAllocator")
            .field("base", &self.base)
            .field("total", &self.total)
            .field("free", &self.free)
            .finish()
    }
}

impl FrameAllocator {
    /// Create a bitmap frame allocator over `[start, end)`, rounding inward to 4 KiB boundaries.
    pub const fn new(start: u64, end: u64) -> Self {
        let base = start.saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let limit = end & !(PAGE_SIZE - 1);
        let total = if limit > base {
            let count = (limit - base) / PAGE_SIZE;
            if count > MAX_FRAMES as u64 {
                MAX_FRAMES
            } else {
                count as usize
            }
        } else {
            0
        };
        Self {
            base,
            total,
            free: total,
            hint: 0,
            bits: [0u64; WORDS],
        }
    }

    /// Re-initialize the allocator in place over `[start, end)`.
    pub fn init(&mut self, start: u64, end: u64) {
        let base = start.saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let limit = end & !(PAGE_SIZE - 1);
        let total = if limit > base {
            let count = (limit - base) / PAGE_SIZE;
            if count > MAX_FRAMES as u64 {
                MAX_FRAMES
            } else {
                count as usize
            }
        } else {
            0
        };
        self.base = base;
        self.total = total;
        self.free = total;
        self.hint = 0;
        let active_words = total.div_ceil(64);
        if let Some(slice) = self.bits.get_mut(..active_words) {
            slice.fill(0);
        }
    }

    /// Allocate the lowest-addressed free 4 KiB frame, or `None` if all frames are in use.
    pub fn alloc(&mut self) -> Option<u64> {
        if self.free == 0 {
            return None;
        }
        let active_words = self.total.div_ceil(64);
        let start_word = self.hint.min(active_words);
        for word_idx in start_word..active_words {
            let word = self.bits.get_mut(word_idx)?;
            if *word != u64::MAX {
                let bit = word.trailing_ones() as usize;
                let idx = word_idx * 64 + bit;
                if idx >= self.total {
                    return None;
                }
                *word |= 1u64 << bit;
                self.free -= 1;
                self.hint = if *word == u64::MAX {
                    word_idx + 1
                } else {
                    word_idx
                };
                return Some(self.base + (idx as u64) * PAGE_SIZE);
            }
        }
        None
    }

    /// Free a previously allocated 4 KiB frame at physical address `pa`.
    pub fn free(&mut self, pa: u64) -> Result<(), Error> {
        if (pa & (PAGE_SIZE - 1)) != 0 {
            return Err(Error::Misaligned(pa));
        }
        let Some(off) = pa.checked_sub(self.base) else {
            return Err(Error::OutOfRange(pa));
        };
        let idx_u64 = off / PAGE_SIZE;
        if idx_u64 >= self.total as u64 {
            return Err(Error::OutOfRange(pa));
        }
        let idx = idx_u64 as usize;
        let word_idx = idx / 64;
        let mask = 1u64 << (idx % 64);
        let Some(word) = self.bits.get_mut(word_idx) else {
            return Err(Error::OutOfRange(pa));
        };
        if (*word & mask) == 0 {
            return Err(Error::DoubleFree(pa));
        }
        *word &= !mask;
        self.free += 1;
        if word_idx < self.hint {
            self.hint = word_idx;
        }
        Ok(())
    }

    /// Return the number of currently free 4 KiB frames.
    pub const fn free_count(&self) -> usize {
        self.free
    }

    /// Return the number of currently allocated 4 KiB frames.
    pub const fn used_count(&self) -> usize {
        self.total - self.free
    }

    /// Return the total number of 4 KiB frames managed by this allocator.
    pub const fn total_count(&self) -> usize {
        self.total
    }

    /// Return the aligned start physical address of the managed range.
    pub const fn start(&self) -> u64 {
        self.base
    }

    /// Return the aligned end physical address (exclusive) of the managed range.
    pub const fn end(&self) -> u64 {
        self.base + (self.total as u64) * PAGE_SIZE
    }

    /// Return true if `pa` is a valid frame address currently marked allocated.
    pub fn is_allocated(&self, pa: u64) -> bool {
        if (pa & (PAGE_SIZE - 1)) != 0 {
            return false;
        }
        let Some(off) = pa.checked_sub(self.base) else {
            return false;
        };
        let idx_u64 = off / PAGE_SIZE;
        if idx_u64 >= self.total as u64 {
            return false;
        }
        let idx = idx_u64 as usize;
        let Some(&word) = self.bits.get(idx / 64) else {
            return false;
        };
        (word & (1u64 << (idx % 64))) != 0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use alloc::collections::BTreeSet;
    use alloc::vec::Vec;
    use assay::forall;

    #[test]
    fn basic_alloc_and_free() {
        let base = 0x8060_0000;
        let end = base + 10 * PAGE_SIZE;
        let mut fa = FrameAllocator::new(base, end);
        assert_eq!(fa.total_count(), 10);
        assert_eq!(fa.free_count(), 10);
        assert_eq!(fa.used_count(), 0);

        let mut addrs = Vec::new();
        for i in 0..10 {
            let pa = fa.alloc().unwrap();
            assert_eq!(pa, base + i * PAGE_SIZE);
            assert!(fa.is_allocated(pa));
            addrs.push(pa);
        }
        assert_eq!(fa.free_count(), 0);
        assert_eq!(fa.alloc(), None);

        // Free one in the middle and reallocate it.
        assert_eq!(fa.free(addrs[4]), Ok(()));
        assert!(!fa.is_allocated(addrs[4]));
        assert_eq!(fa.free_count(), 1);
        assert_eq!(fa.free(addrs[4]), Err(Error::DoubleFree(addrs[4])));

        assert_eq!(fa.alloc(), Some(addrs[4]));
        assert_eq!(fa.free_count(), 0);

        // Out-of-range and misaligned frees fail.
        assert_eq!(
            fa.free(base - PAGE_SIZE),
            Err(Error::OutOfRange(base - PAGE_SIZE))
        );
        assert_eq!(fa.free(end), Err(Error::OutOfRange(end)));
        assert_eq!(fa.free(base + 123), Err(Error::Misaligned(base + 123)));

        for pa in addrs {
            assert_eq!(fa.free(pa), Ok(()));
        }
        assert_eq!(fa.free_count(), 10);
    }

    #[test]
    fn non_multiple_of_64_frames() {
        let base = 0x8000_0000;
        let end = base + 70 * PAGE_SIZE;
        let mut fa = FrameAllocator::new(base, end);
        assert_eq!(fa.total_count(), 70);
        let mut count = 0;
        while fa.alloc().is_some() {
            count += 1;
        }
        assert_eq!(count, 70);
        assert_eq!(fa.free_count(), 0);
    }

    #[test]
    fn shadow_set_property() {
        forall(64, 0xF2A3_E501, |rng| {
            let cap = rng.range(1, 200) as usize;
            let base = 0x8000_0000;
            let end = base + (cap as u64) * PAGE_SIZE;
            let mut fa = FrameAllocator::new(base, end);
            let mut live: BTreeSet<u64> = BTreeSet::new();

            for _ in 0..300 {
                if rng.chance(3, 5) {
                    let res = fa.alloc();
                    if live.len() == cap {
                        assert_eq!(res, None);
                    } else {
                        let pa = res.unwrap();
                        assert!(pa >= base && pa < end);
                        assert_eq!(pa & (PAGE_SIZE - 1), 0);
                        assert!(live.insert(pa));
                    }
                } else if !live.is_empty() && rng.chance(4, 5) {
                    let idx = rng.below(live.len());
                    let pa = *live.iter().nth(idx).unwrap();
                    assert_eq!(fa.free(pa), Ok(()));
                    live.remove(&pa);
                    // Immediate double-free must fail.
                    assert_eq!(fa.free(pa), Err(Error::DoubleFree(pa)));
                } else {
                    let k = rng.range(0, cap as u64 + 5);
                    let pa = base + k * PAGE_SIZE;
                    if k >= cap as u64 {
                        assert_eq!(fa.free(pa), Err(Error::OutOfRange(pa)));
                    } else if live.contains(&pa) {
                        assert_eq!(fa.free(pa), Ok(()));
                        live.remove(&pa);
                    } else {
                        assert_eq!(fa.free(pa), Err(Error::DoubleFree(pa)));
                    }
                }
                assert_eq!(fa.used_count(), live.len());
                assert_eq!(fa.free_count(), cap - live.len());
            }

            for pa in live {
                assert_eq!(fa.free(pa), Ok(()));
            }
            assert_eq!(fa.free_count(), cap);
        });
    }
}
