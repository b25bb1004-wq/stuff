//! atlas: runtime library for Titan programs. One API, a host backend (std) and a titan backend (system calls).
#![no_std]
#![cfg_attr(not(target_os = "none"), forbid(unsafe_code))]

extern crate alloc;
#[cfg(not(target_os = "none"))]
extern crate std;

#[cfg(not(target_os = "none"))]
mod host;
#[cfg(not(target_os = "none"))]
use host as backend;

#[cfg(target_os = "none")]
mod titan;
#[cfg(target_os = "none")]
use titan as backend;

use alloc::string::String;

/// Every failure a runtime call can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Bad argument (titan -1).
    BadArg,
    /// Path or pid not found (titan -2).
    NotFound,
    /// Out of memory (titan -3).
    NoMem,
    /// Bad file descriptor (titan -4).
    BadFd,
    /// Already exists (titan -5).
    Exists,
    /// Not a directory (titan -6).
    NotDir,
    /// Resource busy (titan -7).
    Busy,
    /// Data exceeds a fixed limit (result 64 KiB, spawn 16 strings of 255 bytes, 4 KiB total).
    TooBig,
    /// Any other I/O failure, with a description.
    Io(String),
}

/// Result type of every fallible atlas call.
pub type Result<T> = core::result::Result<T, Error>;

/// Program arguments.
pub mod args {
    pub use crate::backend::args::*;
}

/// Standard streams.
pub mod io {
    pub use crate::backend::io::*;
}

/// File system access.
pub mod fs {
    /// One directory entry: its name and whether it is a directory.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DirEntry {
        /// File or directory name (no path).
        pub name: alloc::string::String,
        /// True when the entry is a directory.
        pub is_dir: bool,
    }

    pub use crate::backend::fs::*;
}

/// Processes: spawn, wait, exit, results.
pub mod proc {
    pub use crate::backend::proc::*;
}

/// Monotonic time.
pub mod time {
    pub use crate::backend::time::*;
}

#[cfg(target_os = "none")]
#[doc(hidden)]
pub use crate::titan::rt as __rt;

#[doc(hidden)]
pub extern crate alloc as __alloc;

/// Writes formatted text followed by a newline to stdout.
#[macro_export]
macro_rules! outln {
    ($($t:tt)*) => {{
        $crate::io::out($crate::__alloc::format!($($t)*).as_bytes());
        $crate::io::out(b"\n");
    }};
}

/// Declares the program entry point: `fn run() -> i32` then `atlas::main!(run);`.
#[macro_export]
macro_rules! main {
    ($f:path) => {
        #[cfg(not(target_os = "none"))]
        fn main() {
            $crate::proc::exit($f())
        }

        #[cfg(target_os = "none")]
        #[global_allocator]
        static __ATLAS_HEAP: $crate::__rt::Heap = $crate::__rt::Heap::new();

        #[cfg(target_os = "none")]
        #[panic_handler]
        fn __atlas_panic(info: &core::panic::PanicInfo) -> ! {
            $crate::__rt::panic(info)
        }

        #[cfg(target_os = "none")]
        extern "C" fn __atlas_entry(argc: usize, argv: *const *const u8) -> ! {
            $crate::__rt::start(argc, argv, $f)
        }

        #[cfg(target_os = "none")]
        core::arch::global_asm!(
            ".section .text.entry",
            ".globl _start",
            "_start:",
            "call {entry}",
            "li a7, 0",
            "ecall",
            "1: j 1b",
            entry = sym __atlas_entry,
        );
    };
}
