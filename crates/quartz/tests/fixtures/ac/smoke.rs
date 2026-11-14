// rustc-built RISC-V program (riscv64gc: compressed code and atomics from LLVM and core).
// Prints "rust ok <hash> <hash % 1000>" on the UART, then writes PASS to the finisher.
#![no_std]
#![no_main]
include!("smoke_core.rs");

core::arch::global_asm!(
    ".section .text.start\n.globl _start\n_start:\n li sp, 0x80800000\n call rmain\n1: j 1b\n"
);

struct Uart;
impl core::fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            unsafe { core::ptr::write_volatile(0x1000_0000 as *mut u8, b) }
        }
        Ok(())
    }
}

#[no_mangle]
extern "C" fn rmain() -> ! {
    use core::fmt::Write;
    let h = smoke();
    let _ = write!(Uart, "rust ok {:016x} {}\n", h, h % 1000);
    unsafe { core::ptr::write_volatile(0x10_0000 as *mut u32, 0x5555) };
    loop {}
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::ptr::write_volatile(0x10_0000 as *mut u32, (9 << 16) | 0x3333) };
    loop {}
}
