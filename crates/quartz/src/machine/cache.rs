//! Direct-mapped decoded-instruction cache and Sv39 fetch TLB for `Machine::step`.
use alloc::vec;
use alloc::vec::Vec;

use crate::inst::Inst;
use crate::Mode;

const DCACHE_LEN: usize = 8192;
const DCACHE_MASK: usize = DCACHE_LEN - 1;
const ITLB_LEN: usize = 64;
const ITLB_MASK: usize = ITLB_LEN - 1;

#[derive(Debug, Clone, Copy)]
struct DecodeEntry {
    pa: u64,
    epoch: u64,
    inst: Inst,
    word: u32,
    len: u8,
}

impl Default for DecodeEntry {
    fn default() -> Self {
        Self {
            pa: 0,
            epoch: 0,
            inst: Inst::Ecall,
            word: 0,
            len: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ItlbEntry {
    vpage: u64,
    satp: u64,
    hgatp: u64,
    ppage: u64,
    mode: u8,
    virt: bool,
    epoch: u64,
}

/// Decoded-instruction cache and instruction-fetch TLB.
#[derive(Debug, Clone)]
pub(crate) struct DecodeCache {
    entries: Vec<DecodeEntry>,
    epoch: u64,
    itlb: [ItlbEntry; ITLB_LEN],
    itlb_epoch: u64,
    pub(crate) pt_pages_mask: u64,
}

impl DecodeCache {
    /// Allocate a new empty cache.
    pub(crate) fn new() -> Self {
        Self {
            entries: vec![DecodeEntry::default(); DCACHE_LEN],
            epoch: 1,
            itlb: [ItlbEntry::default(); ITLB_LEN],
            itlb_epoch: 1,
            pt_pages_mask: 0,
        }
    }

    /// Look up a decoded instruction by physical address `pa`.
    #[inline]
    pub(crate) fn lookup_inst(&self, pa: u64) -> Option<(Inst, u32, u64)> {
        let idx = ((pa >> 1) as usize) & DCACHE_MASK;
        let e = &self.entries[idx];
        if e.epoch == self.epoch && e.pa == pa {
            Some((e.inst, e.word, u64::from(e.len)))
        } else {
            None
        }
    }

    /// Insert a decoded instruction at physical address `pa`.
    #[inline]
    pub(crate) fn insert_inst(&mut self, pa: u64, inst: Inst, word: u32, len: u64) {
        let idx = ((pa >> 1) as usize) & DCACHE_MASK;
        self.entries[idx] = DecodeEntry {
            pa,
            epoch: self.epoch,
            inst,
            word,
            len: len as u8,
        };
    }

    /// Look up a cached Sv39 instruction-fetch translation for `(vpage, satp, hgatp, mode, virt)`.
    #[inline]
    pub(crate) fn lookup_itlb(
        &self,
        vpage: u64,
        (satp, hgatp): (u64, u64),
        mode: Mode,
        virt: bool,
    ) -> Option<u64> {
        let idx = (vpage as usize) & ITLB_MASK;
        let e = &self.itlb[idx];
        if e.epoch == self.itlb_epoch
            && e.vpage == vpage
            && e.satp == satp
            && e.hgatp == hgatp
            && e.mode == mode.level() as u8
            && e.virt == virt
        {
            Some(e.ppage)
        } else {
            None
        }
    }

    /// Insert a validated Sv39 instruction-fetch translation.
    #[inline]
    pub(crate) fn insert_itlb(
        &mut self,
        vpage: u64,
        (satp, hgatp): (u64, u64),
        mode: Mode,
        virt: bool,
        ppage: u64,
    ) {
        let idx = (vpage as usize) & ITLB_MASK;
        self.itlb[idx] = ItlbEntry {
            vpage,
            satp,
            hgatp,
            ppage,
            mode: mode.level() as u8,
            virt,
            epoch: self.itlb_epoch,
        };
    }

    /// Flush all cached instruction-fetch translations.
    pub(crate) fn flush_itlb(&mut self) {
        self.itlb_epoch = self.itlb_epoch.wrapping_add(1);
        if self.itlb_epoch == 0 {
            self.itlb.fill(ItlbEntry::default());
            self.itlb_epoch = 1;
        }
        self.pt_pages_mask = 0;
    }

    /// Invalidate all decoded instructions and translations in O(1).
    pub(crate) fn invalidate_all(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.entries.fill(DecodeEntry::default());
            self.epoch = 1;
        }
        self.flush_itlb();
    }

    /// Invalidate any cached entries overlapping `[pa, pa + len)`.
    #[inline]
    pub(crate) fn invalidate_write(&mut self, pa: u64, len: usize) {
        if len == 0 {
            return;
        }
        if len > 256 {
            self.invalidate_all();
            return;
        }
        let start = (pa & !1).saturating_sub(2);
        let end = pa.saturating_add(len as u64);
        let mut cur = start;
        while cur < end {
            let idx = ((cur >> 1) as usize) & DCACHE_MASK;
            if self.entries[idx].pa == cur {
                self.entries[idx].epoch = 0;
            }
            cur = cur.saturating_add(2);
        }
        if self.pt_pages_mask != 0 {
            let first_page = pa >> 12;
            let last_page = end.saturating_sub(1) >> 12;
            let mut p = first_page;
            loop {
                if self.pt_pages_mask & (1u64 << (p & 63)) != 0 {
                    self.flush_itlb();
                    break;
                }
                if p == last_page {
                    break;
                }
                p = p.saturating_add(1);
            }
        }
    }
}
