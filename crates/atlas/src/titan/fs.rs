//! File system calls over open, read, write, close, readdir, mkdir and unlink.
use super::{calls, error, io::write_all};
use crate::fs::DirEntry;
use crate::{Error, Result};
use alloc::{format, string::String, vec::Vec};

const READ: u64 = 1;
const WRITE: u64 = 2;
const CREATE: u64 = 4;
const TRUNC: u64 = 8;
const APPEND: u64 = 16;
const NOT_DIR: isize = -6;

/// An open descriptor, closed when dropped.
struct Fd(u64);

impl Drop for Fd {
    fn drop(&mut self) {
        let _ = calls::close(self.0);
    }
}

fn open(path: &str, flags: u64) -> Result<Fd> {
    let fd = calls::open(path, flags);
    if fd < 0 {
        return Err(error(fd));
    }
    Ok(Fd(fd as u64))
}

/// The part of `buf` a call that returned `n` bytes filled.
pub(super) fn take(buf: &[u8], n: isize) -> &[u8] {
    buf.get(..(n.max(0) as usize).min(buf.len())).unwrap_or(&[])
}

/// Reads a whole file.
pub fn read(path: &str) -> Result<Vec<u8>> {
    let fd = open(path, READ)?;
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = calls::read(fd.0, &mut buf);
        if n < 0 {
            return Err(error(n));
        }
        if n == 0 {
            return Ok(data);
        }
        data.extend_from_slice(take(&buf, n));
    }
}

/// Reads a whole file as UTF-8.
pub fn read_to_string(path: &str) -> Result<String> {
    String::from_utf8(read(path)?).map_err(|_| Error::Io(String::from("file is not UTF-8")))
}

/// Creates or truncates a file and writes `data`.
pub fn write(path: &str, data: &[u8]) -> Result<()> {
    let fd = open(path, WRITE | CREATE | TRUNC)?;
    write_all(fd.0, data)
}

/// Appends `data` to a file, creating it if needed.
pub fn append(path: &str, data: &[u8]) -> Result<()> {
    let fd = open(path, WRITE | CREATE | APPEND)?;
    write_all(fd.0, data)
}

/// Lists a directory's entries (no `.` or `..`).
pub fn read_dir(path: &str) -> Result<Vec<DirEntry>> {
    let names = read_names(path)?;
    let mut out = Vec::new();
    for line in names.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let name = String::from_utf8_lossy(line).into_owned();
        let child = join(path, &name);
        out.push(DirEntry {
            is_dir: is_dir(&child),
            name,
        });
    }
    Ok(out)
}

/// Raw newline-terminated names from `readdir`.
fn read_names(path: &str) -> Result<Vec<u8>> {
    let fd = open(path, READ)?;
    let mut names = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = calls::readdir(fd.0, &mut buf);
        if n < 0 {
            return Err(error(n));
        }
        if n == 0 {
            return Ok(names);
        }
        names.extend_from_slice(take(&buf, n));
    }
}

fn join(path: &str, name: &str) -> String {
    if path.ends_with('/') {
        format!("{path}{name}")
    } else {
        format!("{path}/{name}")
    }
}

/// Creates one directory; fails with `Exists` if it is already there.
pub fn create_dir(path: &str) -> Result<()> {
    let r = calls::mkdir(path);
    if r < 0 {
        return Err(error(r));
    }
    Ok(())
}

/// Creates a directory and any missing parents.
pub fn create_dir_all(path: &str) -> Result<()> {
    let mut cur = if path.starts_with('/') {
        String::from("/")
    } else {
        String::new()
    };
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if !cur.is_empty() && !cur.ends_with('/') {
            cur.push('/');
        }
        cur.push_str(part);
        match create_dir(&cur) {
            Ok(()) => {}
            Err(Error::Exists) if is_dir(&cur) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Removes one file.
pub fn remove_file(path: &str) -> Result<()> {
    let r = calls::unlink(path);
    if r < 0 {
        return Err(error(r));
    }
    Ok(())
}

/// True when `path` is a directory: readdir on a regular file answers NotDir.
pub fn is_dir(path: &str) -> bool {
    let Ok(fd) = open(path, READ) else {
        return false;
    };
    let mut buf = [0u8; 1024];
    calls::readdir(fd.0, &mut buf) != NOT_DIR
}

/// True when `path` exists (it can be opened).
pub fn exists(path: &str) -> bool {
    open(path, READ).is_ok()
}
