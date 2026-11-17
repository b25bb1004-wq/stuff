//! Process start, panic exit, and the entry used by `atlas::main!`.
use super::calls;
use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub use super::heap::Heap;

pub(super) static ARGC: AtomicUsize = AtomicUsize::new(0);
pub(super) static ARGV: AtomicPtr<*const u8> = AtomicPtr::new(core::ptr::null_mut());

/// Records argc and argv, runs `main`, and exits with its code.
pub fn start(argc: usize, argv: *const *const u8, main: fn() -> i32) -> ! {
    ARGC.store(argc, Ordering::Relaxed);
    ARGV.store(argv as *mut *const u8, Ordering::Relaxed);
    calls::exit(main())
}

struct Stderr;

impl Write for Stderr {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Write errors are ignored: the process is already dying.
        let _ = super::io::write_all(2, s.as_bytes());
        Ok(())
    }
}

/// Writes the panic message to stderr and exits with status 101.
pub fn panic(info: &PanicInfo) -> ! {
    let _ = Stderr.write_fmt(format_args!("panic: {info}\n"));
    calls::exit(101)
}
