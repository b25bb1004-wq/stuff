//! Program arguments, copied from the argv that `_start` received.
use super::rt::{ARGC, ARGV};
use alloc::{string::String, vec::Vec};
use core::sync::atomic::Ordering;

/// Longest argument read, guards against a missing NUL.
const MAX_ARG: usize = 4096;

/// Argument vector including argv[0].
pub fn args() -> Vec<String> {
    let argc = ARGC.load(Ordering::Relaxed);
    let argv = ARGV.load(Ordering::Relaxed);
    let mut out = Vec::new();
    if argv.is_null() {
        return out;
    }
    for i in 0..argc {
        // SAFETY: the kernel passes `argc` pointers, valid for `argc` entries, in `argv`.
        let p = unsafe { *argv.add(i) };
        if p.is_null() {
            break;
        }
        out.push(c_string(p));
    }
    out
}

/// Copies a NUL-terminated string at `p` (at most `MAX_ARG` bytes) into an owned string.
fn c_string(p: *const u8) -> String {
    let mut len = 0;
    while len < MAX_ARG {
        // SAFETY: the kernel places NUL-terminated argument strings in user memory.
        if unsafe { *p.add(len) } == 0 {
            break;
        }
        len += 1;
    }
    // SAFETY: `len` bytes starting at `p` were read above and contain no NUL.
    let bytes = unsafe { core::slice::from_raw_parts(p, len) };
    String::from_utf8_lossy(bytes).into_owned()
}
