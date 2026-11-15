//! WebAssembly C-ABI exports for Titan and the Logos playground.

use std::sync::Mutex;

use crate::playground::run_logos;
use crate::session::{Pump, Titan};

static SESSION: Mutex<Option<Titan>> = Mutex::new(None);
static LAST_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn set_last(bytes: Vec<u8>) -> usize {
    let len = bytes.len();
    let mut guard = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    *guard = bytes;
    len
}

fn copy_slice_from_guest<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Caller passed `(ptr, len)` obtained from `neon_alloc(len)` and filled it in linear
    // memory for this single-threaded wasm call; we borrow it only for the duration of the call.
    Some(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// Allocate `len` bytes in wasm linear memory for guest-to-host transfers.
#[no_mangle]
pub extern "C" fn neon_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return core::ptr::NonNull::<u8>::dangling().as_ptr();
    }
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// Free a buffer previously allocated by [`neon_alloc`].
#[no_mangle]
pub extern "C" fn neon_free(ptr: *mut u8, len: usize) {
    if len == 0 || ptr.is_null() {
        return;
    }
    // SAFETY: `(ptr, len)` was returned by `neon_alloc(len)` and has not been freed yet.
    unsafe {
        let _ = Vec::from_raw_parts(ptr, 0, len);
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
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        set_last(b"invalid ELF pointer".to_vec());
        return -1;
    };
    match Titan::boot(bytes) {
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
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return;
    };
    let line = String::from_utf8_lossy(bytes);
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
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(b"invalid source pointer".to_vec());
    };
    let src = String::from_utf8_lossy(bytes);
    let out = run_logos(&src, fuel);
    set_last(out.into_bytes())
}
