//! Host backend: every call on std. Compiled when the target is not `none`.
pub mod fs;
pub mod proc;

use crate::Error;
use alloc::string::ToString;

/// Maps a std I/O error to the crate's `Error`.
pub(crate) fn map(e: std::io::Error) -> Error {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => Error::NotFound,
        K::AlreadyExists => Error::Exists,
        K::NotADirectory => Error::NotDir,
        K::ResourceBusy => Error::Busy,
        K::OutOfMemory => Error::NoMem,
        K::InvalidInput => Error::BadArg,
        _ => Error::Io(e.to_string()),
    }
}

/// Program arguments.
pub mod args {
    use alloc::{string::String, vec::Vec};

    /// Argument vector including argv[0]; non-UTF-8 bytes become U+FFFD.
    pub fn args() -> Vec<String> {
        std::env::args_os()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }
}

/// Standard streams.
pub mod io {
    use crate::Result;
    use alloc::vec::Vec;
    use std::io::{Read, Write};

    /// Writes bytes to stdout; errors are ignored, as with `print!`.
    pub fn out(bytes: &[u8]) {
        let _ = std::io::stdout().write_all(bytes);
    }

    /// Writes bytes to stderr; errors are ignored.
    pub fn err(bytes: &[u8]) {
        let _ = std::io::stderr().write_all(bytes);
    }

    /// Reads stdin until end of file.
    pub fn read_stdin() -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).map_err(super::map)?;
        Ok(buf)
    }
}

/// Monotonic time.
pub mod time {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();

    /// Milliseconds since the first call to this function.
    pub fn uptime_ms() -> u64 {
        let start = START.get_or_init(Instant::now);
        u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use alloc::vec::Vec;

    #[test]
    fn uptime_is_non_decreasing() {
        let mut last = super::time::uptime_ms();
        for _ in 0..200 {
            let now = super::time::uptime_ms();
            assert!(now >= last);
            last = now;
        }
    }

    #[test]
    fn args_include_program_name() {
        let args: Vec<alloc::string::String> = crate::args::args();
        assert!(!args.is_empty());
    }

    #[test]
    fn outln_compiles_and_runs() {
        crate::outln!("atlas outln test {} {}", 1, "two");
    }
}
