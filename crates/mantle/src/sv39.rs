//! Sv39 3-level 4 KiB page-table management over a [`FrameAlloc`] backing store.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, Not};

/// Size of a 4 KiB Sv39 base page in bytes.
pub const PAGE_SIZE: u64 = 4096;

/// Log2 of [`PAGE_SIZE`].
pub const PAGE_SHIFT: u32 = 12;

/// Number of page-table levels in Sv39.
pub const LEVELS: usize = 3;

/// Number of 64-bit PTEs in one 4 KiB page-table frame.
pub const PTES_PER_PAGE: usize = 512;

/// `satp.MODE` field value for Sv39 (`8 << 60`).
pub const SATP_MODE_SV39: u64 = 8u64 << 60;

/// Sv39 Page Table Entry and permission flag bits (`V R W X U G A D`).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pte(pub u64);

impl Pte {
    /// Valid bit (bit 0).
    pub const V: Self = Self(1 << 0);
    /// Readable bit (bit 1).
    pub const R: Self = Self(1 << 1);
    /// Writable bit (bit 2).
    pub const W: Self = Self(1 << 2);
    /// Executable bit (bit 3).
    pub const X: Self = Self(1 << 3);
    /// User-mode accessible bit (bit 4).
    pub const U: Self = Self(1 << 4);
    /// Global mapping bit (bit 5).
    pub const G: Self = Self(1 << 5);
    /// Accessed bit (bit 6).
    pub const A: Self = Self(1 << 6);
    /// Dirty bit (bit 7).
    pub const D: Self = Self(1 << 7);

    /// Mask of the 8 standard permission/status flag bits (`0xff`).
    pub const FLAGS_MASK: u64 = 0xff;

    /// Empty (invalid) PTE.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Construct a [`Pte`] from raw 64-bit bits.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Return the raw 64-bit value of this [`Pte`].
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Build a [`Pte`] from a 4 KiB-aligned physical address and flag bits.
    pub const fn new(pa: u64, flags: Self) -> Self {
        let ppn = (pa >> PAGE_SHIFT) & ((1u64 << 44) - 1);
        Self((ppn << 10) | (flags.0 & Self::FLAGS_MASK))
    }

    /// Return true if [`Pte::V`] is set.
    pub const fn is_valid(self) -> bool {
        (self.0 & Self::V.0) != 0
    }

    /// Return true if any of `R`, `W`, or `X` is set (marking a leaf PTE).
    pub const fn is_leaf(self) -> bool {
        (self.0 & (Self::R.0 | Self::W.0 | Self::X.0)) != 0
    }

    /// Return true if all bits in `other` are set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Extract the low 8 flag bits (`V R W X U G A D`).
    pub const fn flags(self) -> Self {
        Self(self.0 & Self::FLAGS_MASK)
    }

    /// Extract the 44-bit Physical Page Number (PPN).
    pub const fn ppn(self) -> u64 {
        (self.0 >> 10) & ((1u64 << 44) - 1)
    }

    /// Extract the 4 KiB-aligned physical address (`ppn << 12`).
    pub const fn pa(self) -> u64 {
        self.ppn() << PAGE_SHIFT
    }
}

/// Valid flag constant (`Pte::V`).
pub const V: Pte = Pte::V;
/// Readable flag constant (`Pte::R`).
pub const R: Pte = Pte::R;
/// Writable flag constant (`Pte::W`).
pub const W: Pte = Pte::W;
/// Executable flag constant (`Pte::X`).
pub const X: Pte = Pte::X;
/// User flag constant (`Pte::U`).
pub const U: Pte = Pte::U;
/// Global flag constant (`Pte::G`).
pub const G: Pte = Pte::G;
/// Accessed flag constant (`Pte::A`).
pub const A: Pte = Pte::A;
/// Dirty flag constant (`Pte::D`).
pub const D: Pte = Pte::D;

impl BitOr for Pte {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Pte {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Pte {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for Pte {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl BitXor for Pte {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl Not for Pte {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

/// Errors returned by Sv39 page-table operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Virtual or physical address is not aligned to a 4 KiB boundary.
    MisalignedAddress,
    /// Virtual address is non-canonical or physical address exceeds 56 bits.
    InvalidAddress,
    /// Leaf flags are invalid (e.g., neither R nor X is set, W without R, or unknown bits).
    InvalidFlags,
    /// Frame allocator has no free frames remaining.
    OutOfMemory,
    /// Target virtual address is not currently mapped.
    NotMapped,
    /// An intermediate PTE is a superpage leaf rather than a branch pointer.
    SuperpageConflict,
    /// Deallocating a page-table frame failed.
    FrameFree(crate::frames::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MisalignedAddress => write!(f, "address is not 4 KiB aligned"),
            Self::InvalidAddress => write!(f, "non-canonical VA or out-of-range Sv39 PA"),
            Self::InvalidFlags => write!(f, "invalid Sv39 leaf PTE flags"),
            Self::OutOfMemory => write!(f, "out of page-table frames"),
            Self::NotMapped => write!(f, "virtual address is not mapped"),
            Self::SuperpageConflict => write!(f, "superpage leaf encountered on walk"),
            Self::FrameFree(e) => write!(f, "frame free error: {e}"),
        }
    }
}

/// Return true if `addr` is aligned to a 4 KiB page boundary.
pub const fn is_page_aligned(addr: u64) -> bool {
    (addr & (PAGE_SIZE - 1)) == 0
}

/// Return true if `va` is a canonical Sv39 virtual address (bits 63..=39 all equal bit 38).
pub const fn is_canonical_va(va: u64) -> bool {
    (((va as i64) << 25) >> 25) as u64 == va
}

/// Return true if `pa` fits in the 56-bit Sv39 physical address space.
pub const fn is_valid_pa(pa: u64) -> bool {
    (pa >> 56) == 0
}

/// Extract `(vpn0, vpn1, vpn2)` indices (`0..512`) from a virtual address.
pub const fn vpn(va: u64) -> [usize; 3] {
    [
        ((va >> 12) & 0x1ff) as usize,
        ((va >> 21) & 0x1ff) as usize,
        ((va >> 30) & 0x1ff) as usize,
    ]
}

/// Trait for allocating, freeing, and reading/writing 4 KiB page-table frames.
pub trait FrameAlloc {
    /// Allocate a 4 KiB frame and return its physical address.
    fn alloc(&mut self) -> Option<u64>;

    /// Free a previously allocated 4 KiB frame at `pa`.
    fn free(&mut self, pa: u64) -> Result<(), Error>;

    /// Read the 64-bit PTE at `index` (`0..512`) inside the frame at `frame_pa`.
    fn read_pte(&self, frame_pa: u64, index: usize) -> u64;

    /// Write `val` to the 64-bit PTE at `index` (`0..512`) inside the frame at `frame_pa`.
    fn write_pte(&mut self, frame_pa: u64, index: usize, val: u64);
}

impl<A: FrameAlloc + ?Sized> FrameAlloc for &mut A {
    fn alloc(&mut self) -> Option<u64> {
        (**self).alloc()
    }

    fn free(&mut self, pa: u64) -> Result<(), Error> {
        (**self).free(pa)
    }

    fn read_pte(&self, frame_pa: u64, index: usize) -> u64 {
        (**self).read_pte(frame_pa, index)
    }

    fn write_pte(&mut self, frame_pa: u64, index: usize, val: u64) {
        (**self).write_pte(frame_pa, index, val);
    }
}

/// Safe host-memory frame pool backed by a `Vec<u64>` and [`crate::frames::FrameAllocator`].
#[derive(Debug)]
pub struct FramePool {
    base: u64,
    frames: crate::frames::FrameAllocator,
    words: Vec<u64>,
}

impl FramePool {
    /// Create a pool of `count` 4 KiB frames starting at physical address `base`.
    pub fn new(base: u64, count: usize) -> Self {
        let aligned_base = base & !(PAGE_SIZE - 1);
        let end = aligned_base.saturating_add((count as u64).saturating_mul(PAGE_SIZE));
        let frames = crate::frames::FrameAllocator::new(aligned_base, end);
        let actual = frames.total_count();
        Self {
            base: frames.start(),
            frames,
            words: vec![0u64; actual * PTES_PER_PAGE],
        }
    }

    /// Return the number of free frames in the pool.
    pub const fn free_count(&self) -> usize {
        self.frames.free_count()
    }

    /// Return the number of allocated frames in the pool.
    pub const fn used_count(&self) -> usize {
        self.frames.used_count()
    }

    /// Return the total number of frames in the pool.
    pub const fn total_count(&self) -> usize {
        self.frames.total_count()
    }

    fn word_offset(&self, frame_pa: u64, index: usize) -> Option<usize> {
        if (frame_pa & (PAGE_SIZE - 1)) != 0 || index >= PTES_PER_PAGE {
            return None;
        }
        let off = frame_pa.checked_sub(self.base)?;
        let frame_idx = usize::try_from(off / PAGE_SIZE).ok()?;
        if frame_idx >= self.frames.total_count() {
            return None;
        }
        frame_idx
            .checked_mul(PTES_PER_PAGE)
            .and_then(|base_word| base_word.checked_add(index))
    }
}

impl FrameAlloc for FramePool {
    fn alloc(&mut self) -> Option<u64> {
        let pa = self.frames.alloc()?;
        if let Some(start) = self.word_offset(pa, 0) {
            if let Some(slice) = self.words.get_mut(start..start + PTES_PER_PAGE) {
                slice.fill(0);
            }
        }
        Some(pa)
    }

    fn free(&mut self, pa: u64) -> Result<(), Error> {
        self.frames.free(pa).map_err(Error::FrameFree)
    }

    fn read_pte(&self, frame_pa: u64, index: usize) -> u64 {
        self.word_offset(frame_pa, index)
            .and_then(|i| self.words.get(i).copied())
            .unwrap_or(0)
    }

    fn write_pte(&mut self, frame_pa: u64, index: usize, val: u64) {
        if let Some(i) = self.word_offset(frame_pa, index) {
            if let Some(slot) = self.words.get_mut(i) {
                *slot = val;
            }
        }
    }
}

/// Sv39 3-level page table parameterized over a [`FrameAlloc`].
#[derive(Debug)]
pub struct PageTable<A> {
    root: u64,
    alloc: A,
}

impl<A: FrameAlloc> PageTable<A> {
    /// Allocate a zeroed root (L2) page-table frame from `alloc` and return a new [`PageTable`].
    pub fn new(mut alloc: A) -> Result<Self, Error> {
        let root = alloc.alloc().ok_or(Error::OutOfMemory)?;
        for i in 0..PTES_PER_PAGE {
            alloc.write_pte(root, i, 0);
        }
        Ok(Self { root, alloc })
    }

    /// Wrap an existing root page-table physical address and allocator.
    pub const fn from_root(root: u64, alloc: A) -> Self {
        Self { root, alloc }
    }

    /// Return the physical address of the root (L2) page-table frame.
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// Return the `satp` CSR value (`MODE = Sv39`, `ASID = 0`, `PPN = root >> 12`).
    pub const fn satp(&self) -> u64 {
        SATP_MODE_SV39 | (self.root >> PAGE_SHIFT)
    }

    /// Borrow the underlying [`FrameAlloc`].
    pub const fn allocator(&self) -> &A {
        &self.alloc
    }

    /// Mutably borrow the underlying [`FrameAlloc`].
    pub fn allocator_mut(&mut self) -> &mut A {
        &mut self.alloc
    }

    /// Consume the page table and return its underlying [`FrameAlloc`].
    pub fn into_allocator(self) -> A {
        self.alloc
    }

    fn alloc_zeroed_frame(&mut self) -> Result<u64, Error> {
        let pa = self.alloc.alloc().ok_or(Error::OutOfMemory)?;
        for i in 0..PTES_PER_PAGE {
            self.alloc.write_pte(pa, i, 0);
        }
        Ok(pa)
    }

    fn is_table_empty(&self, table_pa: u64) -> bool {
        for i in 0..PTES_PER_PAGE {
            if Pte::from_bits(self.alloc.read_pte(table_pa, i)).is_valid() {
                return false;
            }
        }
        true
    }

    /// Map virtual page `va` to physical frame `pa` with permission `flags` (`V` is set automatically).
    pub fn map(&mut self, va: u64, pa: u64, flags: Pte) -> Result<(), Error> {
        if !is_page_aligned(va) || !is_page_aligned(pa) {
            return Err(Error::MisalignedAddress);
        }
        if !is_canonical_va(va) || !is_valid_pa(pa) {
            return Err(Error::InvalidAddress);
        }
        // a leaf needs R or X (V-only is a pointer); W without R is reserved
        let leaf_ok = flags.contains(Pte::R) || flags.contains(Pte::X);
        if (flags.bits() & !Pte::FLAGS_MASK) != 0
            || !leaf_ok
            || (flags.contains(Pte::W) && !flags.contains(Pte::R))
        {
            return Err(Error::InvalidFlags);
        }
        let leaf_flags = flags | Pte::V;
        let [vpn0, vpn1, vpn2] = vpn(va);

        let pte2 = Pte::from_bits(self.alloc.read_pte(self.root, vpn2));
        let mut new_l1 = None;
        let l1_pa = if pte2.is_valid() {
            if pte2.is_leaf() {
                return Err(Error::SuperpageConflict);
            }
            pte2.pa()
        } else {
            let f1 = self.alloc_zeroed_frame()?;
            new_l1 = Some(f1);
            self.alloc
                .write_pte(self.root, vpn2, Pte::new(f1, Pte::V).bits());
            f1
        };

        let pte1 = Pte::from_bits(self.alloc.read_pte(l1_pa, vpn1));
        let l0_pa = if pte1.is_valid() {
            if pte1.is_leaf() {
                return Err(Error::SuperpageConflict);
            }
            pte1.pa()
        } else {
            match self.alloc_zeroed_frame() {
                Ok(f0) => {
                    self.alloc
                        .write_pte(l1_pa, vpn1, Pte::new(f0, Pte::V).bits());
                    f0
                }
                Err(e) => {
                    if let Some(f1) = new_l1 {
                        self.alloc.write_pte(self.root, vpn2, 0);
                        let _ = self.alloc.free(f1);
                    }
                    return Err(e);
                }
            }
        };

        self.alloc
            .write_pte(l0_pa, vpn0, Pte::new(pa, leaf_flags).bits());
        Ok(())
    }

    /// Unmap the 4 KiB page at `va`, freeing any intermediate tables that become empty.
    pub fn unmap(&mut self, va: u64) -> Result<(u64, Pte), Error> {
        if !is_page_aligned(va) {
            return Err(Error::MisalignedAddress);
        }
        if !is_canonical_va(va) {
            return Err(Error::InvalidAddress);
        }
        let [vpn0, vpn1, vpn2] = vpn(va);

        let pte2 = Pte::from_bits(self.alloc.read_pte(self.root, vpn2));
        if !pte2.is_valid() || pte2.is_leaf() {
            return Err(Error::NotMapped);
        }
        let l1_pa = pte2.pa();

        let pte1 = Pte::from_bits(self.alloc.read_pte(l1_pa, vpn1));
        if !pte1.is_valid() || pte1.is_leaf() {
            return Err(Error::NotMapped);
        }
        let l0_pa = pte1.pa();

        let pte0 = Pte::from_bits(self.alloc.read_pte(l0_pa, vpn0));
        if !pte0.is_valid() {
            return Err(Error::NotMapped);
        }

        self.alloc.write_pte(l0_pa, vpn0, 0);
        if self.is_table_empty(l0_pa) {
            self.alloc.write_pte(l1_pa, vpn1, 0);
            self.alloc.free(l0_pa)?;
            if self.is_table_empty(l1_pa) {
                self.alloc.write_pte(self.root, vpn2, 0);
                self.alloc.free(l1_pa)?;
            }
        }

        Ok((pte0.pa(), pte0.flags()))
    }

    /// Walk the 3-level Sv39 page table to translate `va` into `(pa, flags)`.
    pub fn translate(&self, va: u64) -> Option<(u64, Pte)> {
        if !is_canonical_va(va) {
            return None;
        }
        let [vpn0, vpn1, vpn2] = vpn(va);
        let offset = va & (PAGE_SIZE - 1);

        let pte2 = Pte::from_bits(self.alloc.read_pte(self.root, vpn2));
        if !pte2.is_valid() || pte2.is_leaf() {
            return None;
        }
        let l1_pa = pte2.pa();

        let pte1 = Pte::from_bits(self.alloc.read_pte(l1_pa, vpn1));
        if !pte1.is_valid() || pte1.is_leaf() {
            return None;
        }
        let l0_pa = pte1.pa();

        let pte0 = Pte::from_bits(self.alloc.read_pte(l0_pa, vpn0));
        if !pte0.is_valid() {
            return None;
        }

        Some((pte0.pa() | offset, pte0.flags()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use alloc::collections::BTreeMap;
    use assay::forall;

    #[test]
    fn canonical_address_rules() {
        assert!(is_canonical_va(0));
        assert!(is_canonical_va(0x0000_003F_FFFF_F000));
        assert!(!is_canonical_va(0x0000_0040_0000_0000));
        assert!(!is_canonical_va(0xFFFF_FFBF_FFFF_F000));
        assert!(is_canonical_va(0xFFFF_FFC0_0000_0000));
        assert!(is_canonical_va(0xFFFF_FFFF_FFFF_F000));

        let pool = FramePool::new(0x8000_0000, 16);
        let mut pt = PageTable::new(pool).unwrap();

        // Misaligned VA or PA rejected.
        assert_eq!(
            pt.map(0x8000_0001, 0x8000_0000, R | W),
            Err(Error::MisalignedAddress)
        );
        assert_eq!(
            pt.map(0x8000_0000, 0x8000_0001, R | W),
            Err(Error::MisalignedAddress)
        );
        assert_eq!(pt.unmap(0x8000_0001), Err(Error::MisalignedAddress));

        // Non-canonical VA rejected.
        let non_canon = 1u64 << 38;
        assert_eq!(
            pt.map(non_canon, 0x8000_0000, R | W),
            Err(Error::InvalidAddress)
        );
        assert_eq!(pt.unmap(non_canon), Err(Error::InvalidAddress));
        assert_eq!(pt.translate(non_canon), None);

        // Out-of-range PA (>= 2^56) rejected.
        assert_eq!(
            pt.map(0x8000_0000, 1u64 << 56, R | W),
            Err(Error::InvalidAddress)
        );

        // Flags with bits outside 0xff rejected.
        assert_eq!(
            pt.map(0x8000_0000, 0x8000_0000, Pte(0x100)),
            Err(Error::InvalidFlags)
        );
    }

    #[test]
    fn map_remap_unmap_reclaims_frames() {
        let pool = FramePool::new(0x8000_0000, 16);
        let mut pt = PageTable::new(pool).unwrap();
        assert_eq!(pt.allocator().used_count(), 1); // root L2 frame

        let va = 0xFFFF_FFC0_0000_0000;
        let pa1 = 0x8020_0000;
        let pa2 = 0x8040_0000;
        pt.map(va, pa1, R | W | A | D).unwrap();
        assert_eq!(pt.allocator().used_count(), 3); // L2 + L1 + L0
        assert_eq!(pt.translate(va), Some((pa1, V | R | W | A | D)));
        assert_eq!(
            pt.translate(va + 0x123),
            Some((pa1 + 0x123, V | R | W | A | D))
        );

        // Remap same VA to pa2 with RX: no extra frames allocated.
        pt.map(va, pa2, R | X | G | A).unwrap();
        assert_eq!(pt.allocator().used_count(), 3);
        assert_eq!(pt.translate(va), Some((pa2, V | R | X | G | A)));

        // Unmap frees both L0 and L1 back to the pool.
        assert_eq!(pt.unmap(va), Ok((pa2, V | R | X | G | A)));
        assert_eq!(pt.translate(va), None);
        assert_eq!(pt.allocator().used_count(), 1);
        assert_eq!(pt.unmap(va), Err(Error::NotMapped));
    }

    #[test]
    fn oom_rollback_does_not_leak() {
        // Pool with 2 frames: 1 for root L2, 1 for L1, 0 left for L0.
        let pool = FramePool::new(0x8000_0000, 2);
        let mut pt = PageTable::new(pool).unwrap();
        assert_eq!(pt.allocator().used_count(), 1);
        assert_eq!(
            pt.map(0x8000_0000, 0x8000_0000, R | W),
            Err(Error::OutOfMemory)
        );
        // L1 must have been rolled back and freed.
        assert_eq!(pt.allocator().used_count(), 1);
        assert_eq!(pt.translate(0x8000_0000), None);
    }

    #[test]
    fn shadow_model_forall() {
        const VALID_FLAG_COMBOS: [Pte; 6] = [
            Pte(V.0 | R.0),
            Pte(V.0 | R.0 | W.0),
            Pte(V.0 | X.0),
            Pte(V.0 | R.0 | X.0),
            Pte(V.0 | R.0 | W.0 | X.0 | U.0 | A.0 | D.0),
            Pte(V.0 | R.0 | G.0 | A.0),
        ];

        forall(50, 0x5739_BEEF, |rng| {
            let pool = FramePool::new(0x8000_0000, 256);
            let mut pt = PageTable::new(pool).unwrap();
            let mut shadow: BTreeMap<u64, (u64, Pte)> = BTreeMap::new();

            // Pick a compact set of candidate canonical VAs across both halves of address space
            // so remaps and shared L1/L0 tables happen frequently.
            let mut va_candidates = Vec::with_capacity(48);
            for _ in 0..48 {
                let high = rng.chance(1, 2);
                let vpn2 = if high {
                    rng.range(256, 512)
                } else {
                    rng.range(0, 256)
                };
                let vpn1 = rng.range(0, 4);
                let vpn0 = rng.range(0, 8);
                let raw = (vpn2 << 30) | (vpn1 << 21) | (vpn0 << 12);
                let va = (((raw as i64) << 25) >> 25) as u64;
                va_candidates.push(va);
            }

            for _ in 0..250 {
                let va = *rng.pick(&va_candidates);
                let op = rng.below(10);
                if op < 5 {
                    // Map or remap
                    let pa = rng.range(0x8000_0000 >> 12, 0x9000_0000 >> 12) << 12;
                    let flags = *rng.pick(&VALID_FLAG_COMBOS);
                    pt.map(va, pa, flags).unwrap();
                    shadow.insert(va, (pa, flags));
                } else if op < 8 {
                    // Unmap
                    let res = pt.unmap(va);
                    if let Some(expected) = shadow.remove(&va) {
                        assert_eq!(res, Ok(expected));
                    } else {
                        assert_eq!(res, Err(Error::NotMapped));
                    }
                } else {
                    // Translate with a random intra-page offset
                    let off = rng.range(0, PAGE_SIZE);
                    let got = pt.translate(va | off);
                    let exp = shadow.get(&va).map(|&(pa, fl)| (pa | off, fl));
                    assert_eq!(got, exp);
                }
            }

            // Verify every candidate VA against the shadow BTreeMap.
            for &va in &va_candidates {
                assert_eq!(pt.translate(va), shadow.get(&va).copied());
            }

            // Unmap all remaining entries and verify zero frames leaked.
            let remaining: Vec<u64> = shadow.keys().copied().collect();
            for va in remaining {
                let exp = shadow.remove(&va).unwrap();
                assert_eq!(pt.unmap(va), Ok(exp));
                assert_eq!(pt.translate(va), None);
            }
            assert_eq!(pt.allocator().used_count(), 1);
            assert_eq!(pt.allocator().free_count(), 255);
        });
    }
}
