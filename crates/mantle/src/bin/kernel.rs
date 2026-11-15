//! Mantle S-mode kernel binary for the Quartz RISC-V machine.
//!
//! Boot protocol: Quartz starts in M-mode at `0x8000_0000` with `a0` (`x10`) = hart id (`0`)
//! and `a1` (`x11`) = guest `ram_size` in bytes (any size from 16 MiB to 256 MiB).
//! The M-mode `_start` shim preserves `a0` and `a1` across `.bss` clearing and CSR setup
//! and passes them directly to `kmain(hart_id, ram_size)` via `mret`.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod kernel {
    extern crate alloc;

    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;
    use core::arch::{asm, global_asm};
    use core::fmt::{self, Write};
    use core::panic::PanicInfo;
    use core::ptr::{read_volatile, write_volatile};
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use mantle::frames::{FrameAllocator, PAGE_SIZE};
    use mantle::sv39::{self, FrameAlloc, PageTable, Pte};
    use mantle::trap::Cause;
    use quarry::{Heap, Locked};

    const RAM_BASE: u64 = 0x8000_0000;
    const DEFAULT_RAM_SIZE: u64 = 16 * 1024 * 1024;
    const HEAP_ALIGN: u64 = 2 * 1024 * 1024;
    const HEAP_SIZE: u64 = 4 * 1024 * 1024;

    const FINISHER_ADDR: u64 = 0x0010_0000;
    const FINISHER_PASS: u32 = 0x5555;
    const FINISHER_FAIL: u32 = (1 << 16) | 0x3333;

    const CLINT_BASE: u64 = 0x0200_0000;
    const CLINT_SIZE: u64 = 0x0001_0000;
    const PLIC_BASE: u64 = 0x0C00_0000;
    const UART_BASE: u64 = 0x1000_0000;
    const BLOCK_BASE: u64 = 0x1000_1000;
    const KEYS_BASE: u64 = 0x1000_2000;
    const DISPLAY_BASE: u64 = 0x1000_3000;
    const FB_BASE: u64 = 0x3000_0000;
    const FB_MAP_SIZE: u64 = 320 * 240 * 4;

    #[global_allocator]
    static HEAP: Locked<Heap> = Locked::new(Heap::empty());

    static FRAMES: Locked<PhysFrames> = Locked::new(PhysFrames {
        inner: FrameAllocator::new(0, 0),
    });

    static TICKS: AtomicU64 = AtomicU64::new(0);
    static FAULT_EXPECTED: AtomicBool = AtomicBool::new(false);
    static LAST_SCAUSE: AtomicU64 = AtomicU64::new(0);
    static LAST_STVAL: AtomicU64 = AtomicU64::new(0);

    extern "C" {
        static _stext: u8;
        static _etext: u8;
        static _srodata: u8;
        static _erodata: u8;
        static _sdata: u8;
        static _end: u8;
    }

    global_asm!(
        ".section .text.entry",
        ".globl _start",
        "_start:",
        "    mv s0, a0; mv s1, a1; la sp, _stack_top",
        "    la t0, _sbss; la t1, _ebss",
        "1:  bgeu t0, t1, 2f; sd zero, 0(t0); addi t0, t0, 8; j 1b",
        "2:  la t0, M_SCRATCH; csrw mscratch, t0",
        "    la t0, m_trap_vector; csrw mtvec, t0",
        "    li t0, 0xf7ff; csrw medeleg, t0",
        "    li t0, 0x222; csrw mideleg, t0",
        "    li t0, 0x0200bff8; ld t1, 0(t0); li t2, 50000; add t1, t1, t2",
        "    li t0, 0x02004000; sd t1, 0(t0)",
        "    li t0, 0x80; csrw mie, t0",
        "    li t0, (1 << 11) | (1 << 7); csrw mstatus, t0",
        "    la t0, kmain; csrw mepc, t0",
        "    mv a0, s0; mv a1, s1; mret",
        ".balign 4",
        "m_trap_vector:",
        "    csrrw tp, mscratch, tp",
        "    sd t0, 0(tp); sd t1, 8(tp); sd t2, 16(tp)",
        "    li t0, 0x0200bff8; ld t1, 0(t0); li t2, 50000; add t1, t1, t2",
        "    li t0, 0x02004000; sd t1, 0(t0)",
        "    li t0, 2; csrs mip, t0",
        "    ld t0, 0(tp); ld t1, 8(tp); ld t2, 16(tp)",
        "    csrrw tp, mscratch, tp; mret",
        ".section .text",
        ".balign 4",
        ".globl s_trap_vector",
        "s_trap_vector:",
        "    addi sp, sp, -288",
        "    sd x1, 8(sp); sd x3, 24(sp); sd x4, 32(sp); sd x5, 40(sp)",
        "    sd x6, 48(sp); sd x7, 56(sp); sd x8, 64(sp); sd x9, 72(sp)",
        "    sd x10, 80(sp); sd x11, 88(sp); sd x12, 96(sp); sd x13, 104(sp)",
        "    sd x14, 112(sp); sd x15, 120(sp); sd x16, 128(sp); sd x17, 136(sp)",
        "    sd x18, 144(sp); sd x19, 152(sp); sd x20, 160(sp); sd x21, 168(sp)",
        "    sd x22, 176(sp); sd x23, 184(sp); sd x24, 192(sp); sd x25, 200(sp)",
        "    sd x26, 208(sp); sd x27, 216(sp); sd x28, 224(sp); sd x29, 232(sp)",
        "    sd x30, 240(sp); sd x31, 248(sp)",
        "    addi t0, sp, 288; sd t0, 16(sp)",
        "    csrr t0, sstatus; sd t0, 256(sp)",
        "    csrr t0, sepc; sd t0, 264(sp)",
        "    csrr t0, scause; sd t0, 272(sp)",
        "    csrr t0, stval; sd t0, 280(sp)",
        "    mv a0, sp; call s_trap_handler",
        "    ld t0, 256(sp); csrw sstatus, t0",
        "    ld t0, 264(sp); csrw sepc, t0",
        "    ld x1, 8(sp); ld x3, 24(sp); ld x4, 32(sp); ld x5, 40(sp)",
        "    ld x6, 48(sp); ld x7, 56(sp); ld x8, 64(sp); ld x9, 72(sp)",
        "    ld x10, 80(sp); ld x11, 88(sp); ld x12, 96(sp); ld x13, 104(sp)",
        "    ld x14, 112(sp); ld x15, 120(sp); ld x16, 128(sp); ld x17, 136(sp)",
        "    ld x18, 144(sp); ld x19, 152(sp); ld x20, 160(sp); ld x21, 168(sp)",
        "    ld x22, 176(sp); ld x23, 184(sp); ld x24, 192(sp); ld x25, 200(sp)",
        "    ld x26, 208(sp); ld x27, 216(sp); ld x28, 224(sp); ld x29, 232(sp)",
        "    ld x30, 240(sp); ld x31, 248(sp)",
        "    addi sp, sp, 288; sret",
        ".section .bss",
        ".balign 16",
        "M_SCRATCH: .space 32",
    );

    #[repr(C)]
    struct TrapFrame {
        regs: [u64; 32],
        sstatus: u64,
        sepc: u64,
        scause: u64,
        stval: u64,
    }

    struct Uart;

    impl Write for Uart {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for b in s.bytes() {
                // SAFETY: `UART_BASE` (`0x1000_0000`) is the memory-mapped UART TX register on Quartz.
                unsafe { write_volatile(UART_BASE as *mut u8, b) };
            }
            Ok(())
        }
    }

    fn finisher_exit(code: u32) -> ! {
        // SAFETY: `FINISHER_ADDR` (`0x0010_0000`) is the 32-bit MMIO test finisher on Quartz.
        unsafe { write_volatile(FINISHER_ADDR as *mut u32, code) };
        loop {
            // SAFETY: `wfi` hints the hart to idle after test completion.
            unsafe { asm!("wfi", options(nomem, nostack)) };
        }
    }

    fn fail(what: &str) -> ! {
        let _ = writeln!(Uart, "FAIL {what}");
        finisher_exit(FINISHER_FAIL)
    }

    struct PhysFrames {
        inner: FrameAllocator,
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

    fn sfence_vma() {
        // SAFETY: `sfence.vma` flushes the TLB in S-mode after page-table updates.
        unsafe { asm!("sfence.vma zero, zero", options(nostack)) };
    }

    fn map_range<A: FrameAlloc>(pt: &mut PageTable<A>, start: u64, end: u64, flags: Pte) {
        let mut addr = start & !(PAGE_SIZE - 1);
        let limit = (end.saturating_add(PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
        while addr < limit {
            if pt.map(addr, addr, flags).is_err() {
                fail("map_range");
            }
            addr += PAGE_SIZE;
        }
    }

    #[no_mangle]
    extern "C" fn s_trap_handler(tf: &mut TrapFrame) {
        let cause = Cause::decode(tf.scause);
        match cause {
            Cause::SupervisorSoft => {
                TICKS.fetch_add(1, Ordering::Relaxed);
                // SAFETY: Clearing `SSIP` (bit 1) in `sip` acknowledges the S-mode software interrupt.
                unsafe { asm!("csrc sip, {mask}", mask = in(reg) 2u64, options(nomem, nostack)) };
            }
            Cause::InstructionPageFault | Cause::LoadPageFault | Cause::StorePageFault => {
                if FAULT_EXPECTED.swap(false, Ordering::SeqCst) {
                    LAST_SCAUSE.store(tf.scause, Ordering::SeqCst);
                    LAST_STVAL.store(tf.stval, Ordering::SeqCst);
                    if cause == Cause::InstructionPageFault {
                        tf.sepc = tf.regs[1];
                    } else {
                        // SAFETY: `tf.sepc` points to the faulting kernel load/store instruction.
                        let half = unsafe { read_volatile(tf.sepc as *const u16) };
                        let len = if (half & 0b11) != 0b11 { 2u64 } else { 4u64 };
                        tf.sepc = tf.sepc.wrapping_add(len);
                    }
                } else {
                    fail("unexpected page fault");
                }
            }
            _ => fail("unexpected trap"),
        }
    }

    #[no_mangle]
    pub extern "C" fn kmain(_hart_id: u64, raw_ram_size: u64) -> ! {
        extern "C" {
            fn s_trap_vector();
        }
        let stvec_val = s_trap_vector as *const () as usize as u64;
        // SAFETY: Setting `stvec` to `s_trap_vector` (4-byte aligned, direct mode) configures the S-mode trap entry.
        unsafe { asm!("csrw stvec, {v}", v = in(reg) stvec_val, options(nomem, nostack)) };

        let ram_size = if raw_ram_size == 0 {
            DEFAULT_RAM_SIZE
        } else {
            raw_ram_size
        };
        let ram_end = RAM_BASE + ram_size;
        let mib = ram_size / (1024 * 1024);

        let stext = &raw const _stext as usize as u64;
        let etext = &raw const _etext as usize as u64;
        let srodata = &raw const _srodata as usize as u64;
        let erodata = &raw const _erodata as usize as u64;
        let sdata = &raw const _sdata as usize as u64;
        let kernel_end = &raw const _end as usize as u64;

        let heap_start = (kernel_end + (HEAP_ALIGN - 1)) & !(HEAP_ALIGN - 1);
        let heap_end = heap_start + HEAP_SIZE;
        if heap_end >= ram_end {
            fail("ram too small");
        }

        let _ = writeln!(Uart, "Mantle v0.1");
        let _ = writeln!(Uart, "ram 0x80000000..0x{ram_end:x} ({mib} MiB)");
        let _ = writeln!(Uart, "heap 0x{heap_start:x}..0x{heap_end:x}");

        let mut frames_guard = FRAMES.lock();
        frames_guard.inner.init(heap_end, ram_end);
        let initial_free = frames_guard.inner.free_count();
        let _ = writeln!(Uart, "frames {initial_free} free");

        let Ok(mut pt) = PageTable::new(&mut *frames_guard) else {
            fail("pt root alloc");
        };

        let rw_mmio = Pte::V | Pte::R | Pte::W | Pte::G | Pte::A | Pte::D;
        map_range(&mut pt, FINISHER_ADDR, FINISHER_ADDR + PAGE_SIZE, rw_mmio);
        map_range(&mut pt, CLINT_BASE, CLINT_BASE + CLINT_SIZE, rw_mmio);
        map_range(&mut pt, PLIC_BASE, PLIC_BASE + 4 * PAGE_SIZE, rw_mmio);
        map_range(
            &mut pt,
            PLIC_BASE + 0x0020_0000,
            PLIC_BASE + 0x0020_0000 + 4 * PAGE_SIZE,
            rw_mmio,
        );
        map_range(&mut pt, UART_BASE, UART_BASE + PAGE_SIZE, rw_mmio);
        map_range(&mut pt, BLOCK_BASE, BLOCK_BASE + PAGE_SIZE, rw_mmio);
        map_range(&mut pt, KEYS_BASE, KEYS_BASE + PAGE_SIZE, rw_mmio);
        map_range(&mut pt, DISPLAY_BASE, DISPLAY_BASE + PAGE_SIZE, rw_mmio);
        map_range(&mut pt, FB_BASE, FB_BASE + FB_MAP_SIZE, rw_mmio);

        let rx_k = Pte::V | Pte::R | Pte::X | Pte::G | Pte::A | Pte::D;
        let ro_k = Pte::V | Pte::R | Pte::G | Pte::A | Pte::D;
        let rw_k = Pte::V | Pte::R | Pte::W | Pte::G | Pte::A | Pte::D;
        map_range(&mut pt, stext, etext, rx_k);
        map_range(&mut pt, srodata, erodata, ro_k);
        map_range(&mut pt, sdata, ram_end, rw_k);

        let satp_val = pt.satp();
        // SAFETY: `pt` identity-maps all active kernel code, stack, data, heap, frames, and MMIO devices.
        unsafe {
            asm!(
                "csrw satp, {satp}",
                "sfence.vma zero, zero",
                satp = in(reg) satp_val,
                options(nostack),
            );
        }
        let _ = writeln!(Uart, "sv39 on");

        // SAFETY: `heap_start..heap_end` is a dedicated 4 MiB RAM region unused by any other subsystem.
        if !unsafe { HEAP.init(heap_start as *mut u8, HEAP_SIZE as usize) } {
            fail("heap init");
        }

        {
            let mut vec_test: Vec<u64> = Vec::with_capacity(10_000);
            for i in 0..10_000u64 {
                vec_test.push(i.wrapping_mul(3).wrapping_add(1));
            }
            if vec_test.len() != 10_000 {
                fail("vec len");
            }
            for (i, &val) in vec_test.iter().enumerate() {
                if val != (i as u64).wrapping_mul(3).wrapping_add(1) {
                    fail("vec check");
                }
            }

            let mut btree_test: BTreeMap<u64, u64> = BTreeMap::new();
            for i in 0..1_000u64 {
                btree_test.insert(i, i.wrapping_mul(7).wrapping_add(5));
            }
            if btree_test.len() != 1_000 {
                fail("btree len");
            }
            for i in 0..1_000u64 {
                if btree_test.get(&i).copied() != Some(i.wrapping_mul(7).wrapping_add(5)) {
                    fail("btree check");
                }
            }
        }

        let free_before = pt.allocator().inner.free_count();
        let scratch_va_base: u64 = 0x0000_0010_0000_0000;
        let mut allocated_pas = [0u64; 64];
        for (i, slot) in allocated_pas.iter_mut().enumerate() {
            let Some(pa) = pt.allocator_mut().alloc() else {
                fail("roundtrip alloc");
            };
            *slot = pa;
            let va = scratch_va_base + (i as u64) * PAGE_SIZE;
            if pt.map(va, pa, rw_k).is_err() {
                fail("roundtrip map");
            }
        }
        sfence_vma();

        for i in 0..64u64 {
            let va = scratch_va_base + i * PAGE_SIZE;
            let expected = 0xCAFE_0000_0000_0000u64 | i;
            // SAFETY: `va` was just mapped to a valid physical frame and flushed with `sfence.vma`.
            unsafe {
                write_volatile(va as *mut u64, expected);
                if read_volatile(va as *const u64) != expected {
                    fail("roundtrip touch");
                }
            }
        }

        for (i, &pa) in allocated_pas.iter().enumerate() {
            let va = scratch_va_base + (i as u64) * PAGE_SIZE;
            let Ok((unmapped_pa, _)) = pt.unmap(va) else {
                fail("roundtrip unmap");
            };
            if unmapped_pa != pa || pt.allocator_mut().free(pa).is_err() {
                fail("roundtrip free");
            }
        }
        sfence_vma();

        if pt.allocator().inner.free_count() != free_before {
            fail("frames free count changed");
        }
        let _ = writeln!(Uart, "heap ok");

        // 1. Load fault from an unmapped address (`ram_end`).
        let load_fault_va = ram_end;
        FAULT_EXPECTED.store(true, Ordering::SeqCst);
        let mut dummy: u64;
        // SAFETY: Deliberately reads from unmapped `load_fault_va`; the S-mode trap handler records the fault and advances `sepc`.
        unsafe {
            asm!(
                ".option push; .option norvc; ld {out}, 0({addr}); .option pop",
                addr = in(reg) load_fault_va,
                out = out(reg) dummy,
                options(nostack),
            );
        }
        let _ = dummy;
        let (scause_load, stval_load) = (
            LAST_SCAUSE.load(Ordering::SeqCst),
            LAST_STVAL.load(Ordering::SeqCst),
        );
        if scause_load != 13 || stval_load != load_fault_va {
            fail("fault load");
        }
        let _ = writeln!(Uart, "fault load 0x{stval_load:x} scause {scause_load}");

        // 2. Store fault to a read-only page (`_srodata`).
        let store_fault_va = srodata;
        FAULT_EXPECTED.store(true, Ordering::SeqCst);
        // SAFETY: Deliberately writes to read-only `store_fault_va`; the S-mode trap handler records the fault and advances `sepc`.
        unsafe {
            asm!(
                ".option push; .option norvc; sd zero, 0({addr}); .option pop",
                addr = in(reg) store_fault_va,
                options(nostack),
            );
        }
        let (scause_store, stval_store) = (
            LAST_SCAUSE.load(Ordering::SeqCst),
            LAST_STVAL.load(Ordering::SeqCst),
        );
        if scause_store != 15 || stval_store != store_fault_va {
            fail("fault store");
        }
        let _ = writeln!(Uart, "fault store 0x{stval_store:x} scause {scause_store}");

        // 3. Fetch fault by jumping to a mapped page without `X` (`_sdata`).
        let fetch_fault_va = sdata;
        FAULT_EXPECTED.store(true, Ordering::SeqCst);
        // SAFETY: Deliberately jumps to non-executable `fetch_fault_va`; `jalr` saves the recovery address in `ra` (`x1`), which the trap handler restores into `sepc`.
        unsafe {
            asm!(
                "jalr ra, {target}, 0",
                target = in(reg) fetch_fault_va,
                out("ra") _,
                options(nostack),
            );
        }
        let (scause_fetch, stval_fetch) = (
            LAST_SCAUSE.load(Ordering::SeqCst),
            LAST_STVAL.load(Ordering::SeqCst),
        );
        if scause_fetch != 12 || stval_fetch != fetch_fault_va {
            fail("fault fetch");
        }
        let _ = writeln!(Uart, "fault fetch 0x{stval_fetch:x} scause {scause_fetch}");

        // Enable S-mode software interrupts (SSIE = 1 << 1) and global S-mode interrupts (SIE = 1 << 1).
        // SAFETY: `stvec` is installed and ready to handle S-mode software interrupts from the M-mode timer shim.
        unsafe {
            asm!(
                "csrs sie, {ssie}; csrs sstatus, {sie}",
                ssie = in(reg) 2u64,
                sie = in(reg) 2u64,
                options(nomem, nostack),
            );
        }
        while TICKS.load(Ordering::Relaxed) < 3 {
            core::hint::spin_loop();
        }
        let _ = writeln!(Uart, "ticks ok");
        let _ = writeln!(Uart, "boot ok");

        finisher_exit(FINISHER_PASS)
    }

    #[panic_handler]
    fn panic(_info: &PanicInfo) -> ! {
        fail("panic")
    }
}
