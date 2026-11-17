//! Process calls: spawn, wait, exit, result and child_result.
use super::{calls, error};
use crate::{Error, Result};
use alloc::{format, string::String, vec, vec::Vec};

/// Most argv strings `spawn` copies (argv[0] included).
const MAX_ARGC: usize = 16;
/// Longest single argv string, NUL excluded.
const MAX_ARG: usize = 255;
/// Most bytes `spawn` copies for all argv strings together.
const MAX_TOTAL: usize = 4096;
/// Largest result, in bytes.
const RESULT_MAX: usize = 64 * 1024;

/// Exits the process with `code`.
pub fn exit(code: i32) -> ! {
    calls::exit(code)
}

/// Starts `path` with `args` (argv[0] is `path`); returns the child pid.
pub fn spawn(
    path: &str,
    args: &[&str],
    stdin_fd: Option<i32>,
    stdout_fd: Option<i32>,
) -> Result<u64> {
    if args.len() + 1 > MAX_ARGC {
        return Err(Error::TooBig);
    }
    let mut words: Vec<Vec<u8>> = Vec::with_capacity(args.len() + 1);
    let mut total = 0;
    for w in core::iter::once(path).chain(args.iter().copied()) {
        if w.len() > MAX_ARG {
            return Err(Error::TooBig);
        }
        total += w.len() + 1;
        if total > MAX_TOTAL {
            return Err(Error::TooBig);
        }
        let mut bytes = Vec::with_capacity(w.len() + 1);
        bytes.extend_from_slice(w.as_bytes());
        bytes.push(0);
        words.push(bytes);
    }
    let ptrs: Vec<*const u8> = words.iter().map(|v| v.as_ptr()).collect();
    let stdin = stdin_fd.map_or(-1, i64::from);
    let stdout = stdout_fd.map_or(-1, i64::from);
    let pid = calls::spawn(path, &ptrs, stdin, stdout);
    if pid < 0 {
        return Err(error(pid));
    }
    Ok(pid as u64)
}

/// Waits for child `pid` and returns its exit code.
/// shortcut: a negative exit code reads as an error, because the call shares its return value with errors.
pub fn wait(pid: u64) -> Result<i32> {
    let code = calls::wait(pid);
    if code < 0 {
        return Err(error(code));
    }
    i32::try_from(code).map_err(|_| Error::Io(String::from("exit code out of range")))
}

/// Stores `envelope` as this process's result (last call wins).
pub fn result(envelope: &str) -> Result<()> {
    if envelope.len() > RESULT_MAX {
        return Err(Error::TooBig);
    }
    let r = calls::result(envelope);
    if r < 0 {
        return Err(error(r));
    }
    Ok(())
}

/// Result of child `pid`, or `None` when it has none.
pub fn child_result(pid: u64) -> Result<Option<String>> {
    let mut buf = vec![0u8; RESULT_MAX];
    let n = calls::child_result(pid, &mut buf);
    if n == -2 {
        return Ok(None);
    }
    if n < 0 {
        return Err(error(n));
    }
    let bytes = buf.get(..(n as usize).min(buf.len())).unwrap_or(&[]);
    String::from_utf8(bytes.to_vec())
        .map(Some)
        .map_err(|_| Error::Io(format!("child {pid} result is not UTF-8")))
}
