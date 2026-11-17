//! Standard streams over the `write` and `read` calls.
use super::{calls, error};
use crate::Result;
use alloc::vec::Vec;

/// Writes bytes to stdout; errors are ignored, as with `print!`.
pub fn out(bytes: &[u8]) {
    let _ = write_all(1, bytes);
}

/// Writes bytes to stderr; errors are ignored.
pub fn err(bytes: &[u8]) {
    let _ = write_all(2, bytes);
}

/// Reads stdin until end of file.
pub fn read_stdin() -> Result<Vec<u8>> {
    let mut all = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = calls::read(0, &mut buf);
        if n < 0 {
            return Err(error(n));
        }
        if n == 0 {
            return Ok(all);
        }
        all.extend_from_slice(super::fs::take(&buf, n));
    }
}

/// Writes all of `data` to `fd`, looping over short writes.
pub(super) fn write_all(fd: u64, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        let n = calls::write(fd, data);
        if n <= 0 {
            return Err(error(n));
        }
        data = data.get(n as usize..).unwrap_or(&[]);
    }
    Ok(())
}
