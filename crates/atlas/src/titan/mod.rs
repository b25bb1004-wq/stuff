//! Titan backend: system calls over `ecall`. Compiled only when the target is `none`.
mod calls;
mod ecall;
mod heap;

pub mod args;
pub mod fs;
pub mod io;
pub mod proc;
pub mod rt;
pub mod time;

use crate::Error;
use alloc::format;

/// Maps a negative system call result to the crate's `Error`.
fn error(code: isize) -> Error {
    match code {
        -1 => Error::BadArg,
        -2 => Error::NotFound,
        -3 => Error::NoMem,
        -4 => Error::BadFd,
        -5 => Error::Exists,
        -6 => Error::NotDir,
        -7 => Error::Busy,
        other => Error::Io(format!("system call error {other}")),
    }
}
