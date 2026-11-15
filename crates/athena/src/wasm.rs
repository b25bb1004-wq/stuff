//! WebAssembly C-ABI exports for Athena IDE services, re-exporting `neon` wasm exports.

use std::sync::Mutex;

use neon::guest_alloc::{Live, LiveTable};
pub use neon::wasm::{
    logos_run, neon_alloc, neon_free, neon_last_len, neon_last_ptr, titan_boot, titan_output,
    titan_pump, titan_send,
};

use crate::debug;
use crate::lang::{definition, format as format_source, hover};
use crate::service::{check, highlight, run_tests, Style};

static ATHENA_LAST_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());
/// Buffers from `athena_alloc` that are not freed yet. Athena reads only pairs recorded here.
static ATHENA_LIVE: Mutex<LiveTable> = Mutex::new(LiveTable::new());

fn set_last(bytes: Vec<u8>) -> usize {
    let len = bytes.len();
    let mut guard = ATHENA_LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    *guard = bytes;
    len
}

/// Allocate `len` bytes in wasm linear memory for the page to fill. Returns null on failure.
/// Neon's allocator does the work and keeps its own table (used by `titan_*` and `logos_run`);
/// this copy records the same pair so athena's reads can check it.
#[no_mangle]
pub extern "C" fn athena_alloc(len: usize) -> *mut u8 {
    let ptr = neon::wasm::neon_alloc(len);
    if len > 0 && !ptr.is_null() {
        ATHENA_LIVE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .record(Live {
                ptr: ptr as usize,
                len,
                cap: len,
            });
    }
    ptr
}

/// Free a buffer from [`athena_alloc`]. Pairs that were not handed out, or were already freed, are ignored.
#[no_mangle]
pub extern "C" fn athena_free(ptr: *mut u8, len: usize) {
    if len == 0 || ptr.is_null() {
        return;
    }
    let taken = ATHENA_LIVE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take(ptr as usize, len);
    if taken.is_some() {
        neon::wasm::neon_free(ptr, len);
    }
}

/// Borrow the guest bytes at `(ptr, len)`. Only a pair that `athena_alloc` handed out and that is still
/// live is read; any other pair (a stale, forged or oversized one) gives `None`.
fn copy_slice_from_guest<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    let known = ATHENA_LIVE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(ptr as usize, len);
    if !known {
        return None;
    }
    // SAFETY: `(ptr, len)` is a live buffer from `athena_alloc(len)` that `athena_free` has not released,
    // so it is valid for `len` bytes. The guest is single-threaded and we borrow it only for this call.
    Some(unsafe { core::slice::from_raw_parts(ptr, len) })
}

fn sanitize_line(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

fn style_class(style: Style) -> &'static str {
    match style {
        Style::Plain => "plain",
        Style::Keyword => "keyword",
        Style::Type => "type",
        Style::Number => "number",
        Style::String => "string",
        Style::Comment => "comment",
        Style::Error => "error",
        Style::Status => "status",
        Style::Match => "match",
    }
}

/// Return a pointer to the last output buffer produced by `athena_*` calls.
#[no_mangle]
pub extern "C" fn athena_last_ptr() -> *const u8 {
    let guard = ATHENA_LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_ptr()
}

/// Return the byte length of the last output buffer produced by `athena_*` calls.
#[no_mangle]
pub extern "C" fn athena_last_len() -> usize {
    let guard = ATHENA_LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
    guard.len()
}

/// Parse and type-check Logos source at `(ptr, len)`, writing `line:col:end_line:end_col:message` lines.
#[no_mangle]
pub extern "C" fn athena_check(ptr: *const u8, len: usize) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(b"1:1:1:1:invalid source pointer".to_vec());
    };
    let src = String::from_utf8_lossy(bytes);
    let diags = check(&src);
    let lines: Vec<String> = diags
        .into_iter()
        .map(|d| {
            format!(
                "{}:{}:{}:{}:{}",
                d.line,
                d.col,
                d.end_line,
                d.end_col,
                sanitize_line(&d.message)
            )
        })
        .collect();
    set_last(lines.join("\n").into_bytes())
}

/// Highlight Logos source at `(ptr, len)`, writing `start len class` lines.
#[no_mangle]
pub extern "C" fn athena_highlight(ptr: *const u8, len: usize) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(Vec::new());
    };
    let src = String::from_utf8_lossy(bytes);
    let runs = highlight(&src);
    let lines: Vec<String> = runs
        .into_iter()
        .map(|(start, end, style)| {
            let run_len = end.saturating_sub(start);
            format!("{start} {run_len} {}", style_class(style))
        })
        .collect();
    set_last(lines.join("\n").into_bytes())
}

/// Run all unit and property tests in Logos source at `(ptr, len)` with `fuel`.
#[no_mangle]
pub extern "C" fn athena_test(ptr: *const u8, len: usize, fuel: u64) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(b"error: 1:1: invalid source pointer".to_vec());
    };
    let src = String::from_utf8_lossy(bytes);
    let lines: Vec<String> = match run_tests(&src, fuel) {
        Ok(results) => results
            .into_iter()
            .map(|r| {
                if r.passed {
                    format!("PASS {}", sanitize_line(&r.name))
                } else {
                    format!(
                        "FAIL {}: {}",
                        sanitize_line(&r.name),
                        sanitize_line(&r.detail)
                    )
                }
            })
            .collect(),
        Err(diags) => diags
            .into_iter()
            .map(|d| format!("error: {}:{}: {}", d.line, d.col, sanitize_line(&d.message)))
            .collect(),
    };
    set_last(lines.join("\n").into_bytes())
}

/// Resolve the declaration byte range `start end` for the identifier at `offset`.
#[no_mangle]
pub extern "C" fn athena_definition(ptr: *const u8, len: usize, offset: usize) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(Vec::new());
    };
    let src = String::from_utf8_lossy(bytes);
    match definition(&src, offset) {
        Some((start, end)) => set_last(format!("{start} {end}").into_bytes()),
        None => set_last(Vec::new()),
    }
}

/// Return hover text for the identifier or literal at `offset` in Logos source at `(ptr, len)`.
#[no_mangle]
pub extern "C" fn athena_hover(ptr: *const u8, len: usize, offset: usize) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(Vec::new());
    };
    let src = String::from_utf8_lossy(bytes);
    match hover(&src, offset) {
        Some(text) => set_last(text.into_bytes()),
        None => set_last(Vec::new()),
    }
}

/// Start the debug session for Logos source at `(ptr, len)`, paused before `main`; writes the debug state text.
#[no_mangle]
pub extern "C" fn athena_debug_start(ptr: *const u8, len: usize) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(debug::error_text("invalid source pointer").into_bytes());
    };
    let src = String::from_utf8_lossy(bytes);
    set_last(debug::start_text(&src).into_bytes())
}

/// Step the debug session: mode 0 continue, 1 over, 2 into, 3 out; breakpoint lines are the digit runs at `(bp_ptr, bp_len)`.
#[no_mangle]
pub extern "C" fn athena_debug_step(mode: u32, bp_ptr: *const u8, bp_len: usize) -> usize {
    let text = copy_slice_from_guest(bp_ptr, bp_len)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    let lines = debug::parse_breakpoints(&text);
    set_last(debug::step_text(mode, &lines).into_bytes())
}

/// Select call frame `index` (0 is innermost) of the debug session; writes its state with that frame's variables.
#[no_mangle]
pub extern "C" fn athena_debug_frame(index: usize) -> usize {
    set_last(debug::frame_text(index).into_bytes())
}

/// End the debug session.
#[no_mangle]
pub extern "C" fn athena_debug_stop() {
    debug::stop();
}

/// Format Logos source at `(ptr, len)`, writing `ok\n<text>` or `error: <message>`.
#[no_mangle]
pub extern "C" fn athena_format(ptr: *const u8, len: usize) -> usize {
    let Some(bytes) = copy_slice_from_guest(ptr, len) else {
        return set_last(b"error: invalid source pointer".to_vec());
    };
    let src = String::from_utf8_lossy(bytes);
    match format_source(&src) {
        Ok(formatted) => set_last(format!("ok\n{formatted}").into_bytes()),
        Err(msg) => set_last(format!("error: {}", sanitize_line(&msg)).into_bytes()),
    }
}
