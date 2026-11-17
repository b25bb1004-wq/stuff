//! Physical frame management, Sv39 page tables, and checked user-memory copy helpers.

use alloc::collections::BTreeMap;
use core::arch::asm;
use core::ptr::{copy_nonoverlapping, read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicU64, Ordering};

use midas::Locked;
use titan::frames::{FrameAllocator, PAGE_SIZE};
use titan::sv39::{self, FrameAlloc, PageTable, Pte, SATP_MODE_SV39};
use titan::uaccess::{check_user_range, Errno, USER_MAX};

pub const RAM_BASE: u64 = 0x8000_0000;
pub const FINISHER_ADDR: u64 = 0x0010_0000;
pub const CLINT_BASE: u64 = 0x0200_0000;
pub const CLINT_SIZE: u64 = 0x0001_0000;
pub const PLIC_BASE: u64 = 0x0C00_0000;
pub const UART_BASE: u64 = 0x1000_0000;
pub const BLOCK_BASE: u64 = 0x1000_1000;
pub const KEYS_BASE: u64 = 0x1000_2000;
pub const DISPLAY_BASE: u64 = 0x1000_3000;
pub const DOOR_BASE: u64 = 0x1000_4000;
pub const FB_BASE: u64 = 0x3000_0000;
pub const FB_MAP_SIZE: u64 = 320 * 240 * 4;

pub const USER_STACK_PAGES: u64 = 4;
pub const USER_STACK_TOP: u64 = USER_MAX;
pub const USER_STACK_BOTTOM: u64 = USER_STACK_TOP - USER_STACK_PAGES * PAGE_SIZE;

pub static FRAMES: Locked<PhysFrames> = Locked::new(PhysFrames {
    inner: FrameAllocator::new(0, 0),
});

pub static KERNEL_PT_ROOT: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_SATP: AtomicU64 = AtomicU64::new(0);
pub static RAM_END: AtomicU64 = AtomicU64::new(0);

pub struct PhysFrames {
    pub inner: FrameAllocator,
}

impl FrameAlloc for PhysFrames {
    fn alloc(&mut self) -> Option<u64> {
        self.inner.alloc()
    }

    fn free(&mut self, pa: u64) -> Result<(), sv39::Error> {
        self.inner.free(pa).map_err(sv39::Error::FrameFree)
    }

    fn read_pte(&self, frame_pa: u64, index: usize) -> u64 {
        let ptr = (frame_pa as *const u64).wrapping_add(index);
        // SAFETY: `frame_pa` is a 4 KiB-aligned page-table frame in identity-mapped RAM and `index < 512`.
        unsafe { read_volatile(ptr) }
    }

    fn write_pte(&mut self, frame_pa: u64, index: usize, val: u64) {
        let ptr = (frame_pa as *mut u64).wrapping_add(index);
        // SAFETY: `frame_pa` is a 4 KiB-aligned page-table frame in identity-mapped RAM and `index < 512`.
        unsafe { write_volatile(ptr, val) }
    }
}

pub fn sfence_vma() {
    // SAFETY: `sfence.vma` flushes the S-mode TLB after page-table updates.
    unsafe { asm!("sfence.vma zero, zero", options(nostack)) };
}

pub fn write_satp(satp: u64) {
    // SAFETY: `satp` points to a valid Sv39 root table containing all active kernel mappings.
    unsafe {
        asm!(
            "csrw satp, {satp}",
            "sfence.vma zero, zero",
            satp = in(reg) satp,
            options(nostack),
        );
    }
}

pub fn switch_to_kernel_pt() {
    let satp = KERNEL_SATP.load(Ordering::Acquire);
    if satp != 0 {
        write_satp(satp);
    }
}

pub fn satp_for_root(root: u64) -> u64 {
    SATP_MODE_SV39 | (root >> 12)
}

pub fn free_frame_count() -> usize {
    FRAMES.lock().inner.free_count()
}

pub fn zero_frame(pa: u64) {
    // SAFETY: `pa` is a newly allocated 4 KiB RAM frame in identity-mapped memory.
    unsafe { write_bytes(pa as *mut u8, 0, PAGE_SIZE as usize) };
}

pub fn copy_into_frame(pa: u64, offset: usize, src: &[u8]) {
    let dst = (pa as *mut u8).wrapping_add(offset);
    // SAFETY: `pa` is a valid 4 KiB RAM frame and `offset + src.len() <= PAGE_SIZE`.
    unsafe { copy_nonoverlapping(src.as_ptr(), dst, src.len()) };
}

pub fn map_range<A: FrameAlloc>(
    pt: &mut PageTable<A>,
    start: u64,
    end: u64,
    flags: Pte,
) -> Result<(), sv39::Error> {
    let mut addr = start & !(PAGE_SIZE - 1);
    let limit = (end.saturating_add(PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
    while addr < limit {
        pt.map(addr, addr, flags)?;
        addr += PAGE_SIZE;
    }
    Ok(())
}

fn unmap_range_no_free<A: FrameAlloc>(pt: &mut PageTable<A>, start: u64, end: u64) {
    let mut addr = start & !(PAGE_SIZE - 1);
    let limit = (end.saturating_add(PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
    while addr < limit {
        let _ = pt.unmap(addr);
        addr += PAGE_SIZE;
    }
}

const MMIO_RANGES: [(u64, u64); 9] = [
    (CLINT_BASE, CLINT_BASE + CLINT_SIZE),
    (PLIC_BASE, PLIC_BASE + 4 * PAGE_SIZE),
    (
        PLIC_BASE + 0x0020_0000,
        PLIC_BASE + 0x0020_0000 + 4 * PAGE_SIZE,
    ),
    (UART_BASE, UART_BASE + PAGE_SIZE),
    (BLOCK_BASE, BLOCK_BASE + PAGE_SIZE),
    (KEYS_BASE, KEYS_BASE + PAGE_SIZE),
    (DISPLAY_BASE, DISPLAY_BASE + PAGE_SIZE),
    (DOOR_BASE, DOOR_BASE + PAGE_SIZE),
    (FB_BASE, FB_BASE + FB_MAP_SIZE),
];

/// Create a per-process Sv39 root page table containing kernel RAM (`U = 0`) and MMIO (`U = 0`).
pub fn create_user_pt() -> Option<u64> {
    let kernel_root = KERNEL_PT_ROOT.load(Ordering::Acquire);
    let mut frames_guard = FRAMES.lock();
    let mut pt = PageTable::new(&mut *frames_guard).ok()?;
    let root = pt.root();

    // Share top-level L2 entries >= 2 (all kernel RAM at 0x8000_0000..ram_end, U = 0).
    for i in 2..512 {
        let pte = pt.allocator().read_pte(kernel_root, i);
        if pte != 0 {
            pt.allocator_mut().write_pte(root, i, pte);
        }
    }

    let rw_mmio = Pte::V | Pte::R | Pte::W | Pte::G | Pte::A | Pte::D;
    for &(start, end) in &MMIO_RANGES {
        if map_range(&mut pt, start, end, rw_mmio).is_err() {
            for &(s, e) in &MMIO_RANGES {
                unmap_range_no_free(&mut pt, s, e);
            }
            for i in 2..512 {
                pt.allocator_mut().write_pte(root, i, 0);
            }
            let _ = pt.allocator_mut().free(root);
            return None;
        }
    }

    Some(root)
}

/// Unmap all user pages (freeing their physical frames), unmap kernel MMIO, detach shared RAM L2s,
/// and free the root page-table frame.
pub fn destroy_user_pt(root: u64, user_pages: &BTreeMap<u64, u64>) {
    let mut frames_guard = FRAMES.lock();
    let mut pt = PageTable::from_root(root, &mut *frames_guard);

    for (&va, &pa) in user_pages {
        let _ = pt.unmap(va);
        let _ = pt.allocator_mut().free(pa);
    }
    for &(start, end) in &MMIO_RANGES {
        unmap_range_no_free(&mut pt, start, end);
    }
    for i in 2..512 {
        pt.allocator_mut().write_pte(root, i, 0);
    }
    let _ = pt.allocator_mut().free(root);
}

fn validate_pages(root: u64, ptr: u64, len: u64, need_write: bool) -> Result<(), Errno> {
    check_user_range(ptr, len)?;
    if len == 0 {
        return Ok(());
    }
    let ram_end = RAM_END.load(Ordering::Acquire);
    let required = if need_write {
        Pte::V | Pte::U | Pte::R | Pte::W
    } else {
        Pte::V | Pte::U | Pte::R
    };
    let end = ptr + len;
    let mut page_va = ptr & !(PAGE_SIZE - 1);
    let mut frames_guard = FRAMES.lock();
    let pt = PageTable::from_root(root, &mut *frames_guard);
    while page_va < end {
        let Some((pa, flags)) = pt.translate(page_va) else {
            return Err(Errno::BadArg);
        };
        if !flags.contains(required) || pa < RAM_BASE || pa + PAGE_SIZE > ram_end {
            return Err(Errno::BadArg);
        }
        page_va += PAGE_SIZE;
    }
    Ok(())
}

/// Validate that `[ptr, ptr + len)` is mapped `U | R` in `root`.
pub fn validate_user_read(root: u64, ptr: u64, len: u64) -> Result<(), Errno> {
    validate_pages(root, ptr, len, false)
}

/// Validate that `[ptr, ptr + len)` is mapped `U | R | W` in `root`.
pub fn validate_user_write(root: u64, ptr: u64, len: u64) -> Result<(), Errno> {
    validate_pages(root, ptr, len, true)
}

/// Copy `dst.len()` bytes from user memory at `ptr` in `root` into `dst`.
pub fn copy_from_user(root: u64, ptr: u64, dst: &mut [u8]) -> Result<(), Errno> {
    validate_pages(root, ptr, dst.len() as u64, false)?;
    if dst.is_empty() {
        return Ok(());
    }
    let mut va = ptr;
    let mut off = 0usize;
    let mut frames_guard = FRAMES.lock();
    let pt = PageTable::from_root(root, &mut *frames_guard);
    while off < dst.len() {
        let page_off = (va & (PAGE_SIZE - 1)) as usize;
        let chunk = (dst.len() - off).min((PAGE_SIZE as usize) - page_off);
        let (pa_base, _) = pt.translate(va & !(PAGE_SIZE - 1)).ok_or(Errno::BadArg)?;
        let src = (pa_base as *const u8).wrapping_add(page_off);
        // SAFETY: `validate_pages` verified `pa_base..pa_base+PAGE_SIZE` is mapped RAM and `chunk <= PAGE_SIZE - page_off`.
        unsafe { copy_nonoverlapping(src, dst.as_mut_ptr().add(off), chunk) };
        va += chunk as u64;
        off += chunk;
    }
    Ok(())
}

/// Copy `src` bytes into user memory at `ptr` in `root`.
pub fn copy_to_user(root: u64, ptr: u64, src: &[u8]) -> Result<(), Errno> {
    validate_pages(root, ptr, src.len() as u64, true)?;
    if src.is_empty() {
        return Ok(());
    }
    let mut va = ptr;
    let mut off = 0usize;
    let mut frames_guard = FRAMES.lock();
    let pt = PageTable::from_root(root, &mut *frames_guard);
    while off < src.len() {
        let page_off = (va & (PAGE_SIZE - 1)) as usize;
        let chunk = (src.len() - off).min((PAGE_SIZE as usize) - page_off);
        let (pa_base, _) = pt.translate(va & !(PAGE_SIZE - 1)).ok_or(Errno::BadArg)?;
        let dst = (pa_base as *mut u8).wrapping_add(page_off);
        // SAFETY: `validate_pages` verified `pa_base..pa_base+PAGE_SIZE` is mapped writable user RAM.
        unsafe { copy_nonoverlapping(src.as_ptr().add(off), dst, chunk) };
        va += chunk as u64;
        off += chunk;
    }
    Ok(())
}
