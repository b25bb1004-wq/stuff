//! WebAssembly C-ABI exports for Athena IDE services, re-exporting `neon` wasm exports.

use std::sync::Mutex;

pub use neon::wasm::{
    logos_run, neon_alloc, neon_free, neon_last_len, neon_last_ptr, titan_boot, titan_output,
    titan_pump, titan_send,
};

use crate::lang::{definition, format as format_source, hover};
use crate::service::{check, highlight, run_tests, Style};

static ATHENA_LAST_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn set_last(bytes: Vec<u8>) -> usize {
    let len = bytes.len();
    let mut guard = ATHENA_LAST_BUF.lock().unwrap_or_else(|p| p.into_inner());
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
