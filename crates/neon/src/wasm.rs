//! WebAssembly C-ABI exports for Titan and the Logos playground.

use std::sync::Mutex;

use crate::guest_alloc::{reserve, Live, LiveTable};
use crate::playground::run_logos;
use crate::session::{Pump, Titan};

static SESSION: Mutex<Option<Titan>> = Mutex::new(None);
static LAST_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());
/// Buffers from `neon_alloc` that have not been freed yet.
static LIVE: Mutex<LiveTable> = Mutex::new(LiveTable::new());

fn set_last(bytes: Vec<u8>) -> usize {
    let len = bytes.len();
    let mut guard = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    *guard = bytes;
    len
}

/// Borrow the guest bytes at `(ptr, len)`. Only pairs that `neon_alloc` handed out and that are
/// still live are read; any other pair gives an empty slice.
fn copy_slice_from_guest<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    let known = LIVE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(ptr as usize, len);
    if !known {
        return &[];
    }
    // SAFETY: `(ptr, len)` matches a live buffer from `neon_alloc(len)`, so it is valid for `len`
    // bytes and not freed. The guest is single-threaded and we borrow only for this one call.
    unsafe { core::slice::from_raw_parts(ptr, len) }
}

/// Allocate `len` bytes in wasm linear memory for guest-to-host transfers. Returns null on failure.
#[no_mangle]
pub extern "C" fn neon_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return core::ptr::NonNull::<u8>::dangling().as_ptr();
    }
    let Some((ptr, cap)) = reserve(len) else {
        return core::ptr::null_mut();
    };
    LIVE.lock()
        .unwrap_or_else(|p| p.into_inner())
        .record(Live { ptr, len, cap });
    ptr as *mut u8
}

/// Free a buffer previously allocated by [`neon_alloc`]. Pairs it did not hand out are ignored.
#[no_mangle]
pub extern "C" fn neon_free(ptr: *mut u8, len: usize) {
    if len == 0 || ptr.is_null() {
        return;
    }
    let taken = LIVE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take(ptr as usize, len);
    let Some(live) = taken else {
        return;
    };
    // SAFETY: `take` yields only a `(ptr, len)` that `neon_alloc` recorded, and removes it, so this
    // is the single reconstruction of that allocation, with the capacity it was made with.
    unsafe {
        let _ = Vec::from_raw_parts(ptr, 0, live.cap);
    }
}

/// Return a pointer to the last output or error buffer produced by `neon`.
#[no_mangle]
pub extern "C" fn neon_last_ptr() -> *const u8 {
    let guard = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_ptr()
}

/// Return the byte length of the last output or error buffer produced by `neon`.
#[no_mangle]
pub extern "C" fn neon_last_len() -> usize {
    let guard = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    guard.len()
}

/// Boot a Titan session from the ELF bytes at `(ptr, len)`. Returns `0` on success, else sets the error buffer.
#[no_mangle]
pub extern "C" fn titan_boot(ptr: *const u8, len: usize) -> i32 {
    match Titan::boot(copy_slice_from_guest(ptr, len)) {
        Ok(titan) => {
            let mut sess = SESSION.lock().unwrap_or_else(|p| p.into_inner());
            *sess = Some(titan);
            set_last(Vec::new());
            0
        }
        Err(err) => {
            set_last(err.into_bytes());
            -1
        }
    }
}

/// Queue a line of input (UTF-8, newline appended) on the active Titan session's UART.
#[no_mangle]
pub extern "C" fn titan_send(ptr: *const u8, len: usize) {
    let line = String::from_utf8_lossy(copy_slice_from_guest(ptr, len));
    let mut sess = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(titan) = sess.as_mut() {
        titan.send(&line);
    }
}

/// Pump the active Titan session up to `max_steps`. Returns `0` waiting, `1` busy, `2 + code` exited, or `-1` if unbooted.
#[no_mangle]
pub extern "C" fn titan_pump(max_steps: u64) -> i32 {
    let mut sess = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    let Some(titan) = sess.as_mut() else {
        return -1;
    };
    match titan.pump(max_steps) {
        Pump::WaitingForInput => 0,
        Pump::Busy => 1,
        Pump::Exited(code) => 2i32.saturating_add(code),
    }
}

/// Drain pending Titan UART output into the last buffer and return its byte length.
#[no_mangle]
pub extern "C" fn titan_output() -> usize {
    let mut sess = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    let bytes = match sess.as_mut() {
        Some(titan) => titan.take_output(),
        None => Vec::new(),
    };
    set_last(bytes)
}

/// Run the Logos source at `(ptr, len)` with `fuel`, storing the output/error in the last buffer and returning its length.
#[no_mangle]
pub extern "C" fn logos_run(ptr: *const u8, len: usize, fuel: u64) -> usize {
    let src = String::from_utf8_lossy(copy_slice_from_guest(ptr, len));
    let out = run_logos(&src, fuel);
    set_last(out.into_bytes())
}
