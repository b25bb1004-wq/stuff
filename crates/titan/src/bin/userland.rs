//! Titan multicall userland binary (`sh`, `ls`, `cat`, `echo`, `mkdir`, `rm`, `ps`).
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
extern crate alloc;

#[cfg(target_os = "none")]
#[path = "userland/sys.rs"]
mod sys;

#[cfg(target_os = "none")]
#[path = "userland/cmds.rs"]
mod cmds;

#[cfg(target_os = "none")]
#[path = "userland/sh.rs"]
mod sh;

#[cfg(target_os = "none")]
mod app {
    extern crate alloc;

    use alloc::vec::Vec;
    use core::arch::global_asm;
    use core::fmt::Write;
    use core::panic::PanicInfo;

    use crate::cmds;
    use crate::sh;
    use crate::sys::{self, BumpAllocator, FdWriter};

    #[global_allocator]
    static ALLOCATOR: BumpAllocator = BumpAllocator::new();

    global_asm!(
        ".section .text.entry",
        ".globl _start",
        "_start:",
        "    call user_main",
        "    li a7, 0",
        "    ecall",
        "1:  j 1b",
    );

    unsafe fn cstr_to_str<'a>(ptr: *const u8) -> Option<&'a str> {
        if ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        while len < 4096 {
            // SAFETY: `ptr` comes from the kernel-populated `argv` array and ends with a NUL byte.
            let b = unsafe { *ptr.add(len) };
            if b == 0 {
                break;
            }
            len += 1;
        }
        // SAFETY: `ptr..ptr+len` is a valid NUL-terminated string within user stack bounds.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        core::str::from_utf8(bytes).ok()
    }

    #[no_mangle]
    extern "C" fn user_main(argc: usize, argv: *const *const u8) -> i32 {
        let mut args: Vec<&str> = Vec::new();
        if !argv.is_null() {
            let count = argc.min(64);
            for i in 0..count {
                // SAFETY: Kernel placed `argc` valid pointers followed by NULL at `argv`.
                let ptr = unsafe { *argv.add(i) };
                let Some(s) = (unsafe { cstr_to_str(ptr) }) else {
                    break;
                };
                args.push(s);
            }
        }
        let Some(&first) = args.first() else {
            let _ = writeln!(FdWriter(2), "userland: missing argv[0]");
            return 127;
        };
        let prog = first.rsplit('/').next().unwrap_or(first);
        match prog {
            "sh" => sh::run_sh(),
            "ls" => cmds::run_ls(&args),
            "cat" => cmds::run_cat(&args),
            "echo" => cmds::run_echo(&args),
            "mkdir" => cmds::run_mkdir(&args),
            "rm" => cmds::run_rm(&args),
            "ps" => cmds::run_ps(),
            _ => {
                let _ = writeln!(FdWriter(2), "{prog}: unknown applet");
                127
            }
        }
    }

    #[panic_handler]
    fn panic(_info: &PanicInfo) -> ! {
        let _ = sys::write_all(2, b"panic\n");
        sys::exit(101)
    }
}
