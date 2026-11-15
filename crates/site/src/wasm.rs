//! WebAssembly C-ABI exports for the Monolith site demos (plus `neon` exports).

use std::fmt::Write as _;
use std::sync::Mutex;

pub use neon::wasm::*;

use crate::demos::{self, Output, MAX_INPUT_BYTES};

static LAST_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static PIXEL_DIMS: Mutex<(u32, u32)> = Mutex::new((0, 0));

fn set_last(bytes: Vec<u8>, dims: (u32, u32)) -> usize {
    let len = bytes.len();
    let mut buf = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    *buf = bytes;
    let mut px = PIXEL_DIMS.lock().unwrap_or_else(|p| p.into_inner());
    *px = dims;
    len
}

fn copy_slice_from_guest<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Caller allocated `(ptr, len)` via `neon_alloc` in wasm linear memory and filled it
    // before invoking this single-threaded C-ABI entrypoint; borrow ends before returning.
    Some(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// Return a pointer to the last output or error buffer produced by `site`.
#[no_mangle]
pub extern "C" fn site_last_ptr() -> *const u8 {
    let guard = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_ptr()
}

/// Return the byte length of the last output or error buffer produced by `site`.
#[no_mangle]
pub extern "C" fn site_last_len() -> usize {
    let guard = LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    guard.len()
}

/// Return the width in pixels of the last pixel output (0 when last output was text).
#[no_mangle]
pub extern "C" fn site_pixels_w() -> u32 {
    let guard = PIXEL_DIMS.lock().unwrap_or_else(|p| p.into_inner());
    guard.0
}

/// Return the height in pixels of the last pixel output (0 when last output was text).
#[no_mangle]
pub extern "C" fn site_pixels_h() -> u32 {
    let guard = PIXEL_DIMS.lock().unwrap_or_else(|p| p.into_inner());
    guard.1
}

/// Write `id\ttitle` lines for every registered demo into the last buffer and return its byte length.
#[no_mangle]
pub extern "C" fn site_list() -> usize {
    let mut out = String::new();
    for demo in demos::all() {
        let _ = writeln!(out, "{}\t{}", demo.id, demo.title);
    }
    set_last(out.into_bytes(), (0, 0))
}

/// Run the demo identified by `(id_ptr, id_len)` on `(in_ptr, in_len)`.
/// Returns `0` when text is ready, `1` when RGBA pixels are ready, `2` when error text is ready.
#[no_mangle]
pub extern "C" fn site_demo(
    id_ptr: *const u8,
    id_len: usize,
    in_ptr: *const u8,
    in_len: usize,
) -> i32 {
    if id_len > 128 {
        set_last(b"demo id too long".to_vec(), (0, 0));
        return 2;
    }
    if in_len > MAX_INPUT_BYTES {
        set_last(
            format!("input exceeds {MAX_INPUT_BYTES} byte limit").into_bytes(),
            (0, 0),
        );
        return 2;
    }
    let Some(id_bytes) = copy_slice_from_guest(id_ptr, id_len) else {
        set_last(b"invalid demo id pointer".to_vec(), (0, 0));
        return 2;
    };
    let Ok(id) = core::str::from_utf8(id_bytes) else {
        set_last(b"demo id must be valid UTF-8".to_vec(), (0, 0));
        return 2;
    };
    let Some(demo) = demos::find(id) else {
        set_last(format!("unknown demo `{id}`").into_bytes(), (0, 0));
        return 2;
    };
    let Some(in_bytes) = copy_slice_from_guest(in_ptr, in_len) else {
        set_last(b"invalid input pointer".to_vec(), (0, 0));
        return 2;
    };
    let Ok(input) = core::str::from_utf8(in_bytes) else {
        set_last(b"input must be valid UTF-8".to_vec(), (0, 0));
        return 2;
    };

    match (demo.run)(input) {
        Ok(Output::Text(text)) => {
            set_last(text.into_bytes(), (0, 0));
            0
        }
        Ok(Output::Pixels { w, h, rgba }) => {
            set_last(rgba, (w, h));
            1
        }
        Err(err) => {
            set_last(err.into_bytes(), (0, 0));
            2
        }
    }
}
