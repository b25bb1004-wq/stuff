//! Host file system calls on std.
use super::map;
use crate::fs::DirEntry;
use crate::Result;
use alloc::{string::String, vec::Vec};

/// Reads a whole file.
pub fn read(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(map)
}

/// Reads a whole file as UTF-8.
pub fn read_to_string(path: &str) -> Result<String> {
    std::fs::read_to_string(path).map_err(map)
}

/// Creates or truncates a file and writes `data`.
pub fn write(path: &str, data: &[u8]) -> Result<()> {
    std::fs::write(path, data).map_err(map)
}

/// Appends `data` to a file, creating it if needed.
pub fn append(path: &str, data: &[u8]) -> Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut f| f.write_all(data))
        .map_err(map)
}

/// Lists a directory's entries (no `.` or `..`).
pub fn read_dir(path: &str) -> Result<Vec<DirEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path).map_err(map)? {
        let entry = entry.map_err(map)?;
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: entry.file_type().is_ok_and(|t| t.is_dir()),
        });
    }
    Ok(out)
}

/// Creates one directory; fails if it exists.
pub fn create_dir(path: &str) -> Result<()> {
    std::fs::create_dir(path).map_err(map)
}

/// Creates a directory and any missing parents.
pub fn create_dir_all(path: &str) -> Result<()> {
    std::fs::create_dir_all(path).map_err(map)
}

/// Removes one file.
pub fn remove_file(path: &str) -> Result<()> {
    std::fs::remove_file(path).map_err(map)
}

/// True when `path` is a directory.
pub fn is_dir(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_dir())
}

/// True when `path` exists.
pub fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::Error;
    use alloc::format;

    /// Fresh empty folder under the OS temp dir, unique per test name.
    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("atlas-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn write_read_roundtrip_and_truncate() {
        let d = scratch("write_read");
        let p = format!("{d}/a.txt");
        write(&p, b"hello world").unwrap();
        assert_eq!(read(&p).unwrap(), b"hello world");
        write(&p, b"hi").unwrap();
        assert_eq!(read_to_string(&p).unwrap(), "hi");
        assert_eq!(read(&format!("{d}/missing")), Err(Error::NotFound));
    }

    #[test]
    fn append_adds_to_end() {
        let d = scratch("append");
        let p = format!("{d}/log");
        append(&p, b"one ").unwrap();
        append(&p, b"two").unwrap();
        assert_eq!(read(&p).unwrap(), b"one two");
    }

    #[test]
    fn dirs_exists_and_errors() {
        let d = scratch("dirs");
        let sub = format!("{d}/x/y");
        create_dir_all(&sub).unwrap();
        create_dir_all(&sub).unwrap();
        assert!(is_dir(&sub));
        assert!(exists(&sub));
        assert_eq!(create_dir(&sub), Err(Error::Exists));
        let file = format!("{d}/f");
        write(&file, b"x").unwrap();
        assert!(!is_dir(&file));
        assert!(exists(&file));
        assert!(!exists(&format!("{d}/nope")));
        assert_eq!(read_dir(&file).err(), Some(Error::NotDir));
    }

    #[test]
    fn read_dir_lists_names_and_kinds() {
        let d = scratch("list");
        create_dir(&format!("{d}/sub")).unwrap();
        write(&format!("{d}/file"), b"").unwrap();
        let mut entries = read_dir(&d).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(entries.len(), 2);
        assert_eq!(
            (entries[0].name.as_str(), entries[0].is_dir),
            ("file", false)
        );
        assert_eq!((entries[1].name.as_str(), entries[1].is_dir), ("sub", true));
    }

    #[test]
    fn remove_file_and_not_found() {
        let d = scratch("remove");
        let p = format!("{d}/gone");
        write(&p, b"x").unwrap();
        remove_file(&p).unwrap();
        assert!(!exists(&p));
        assert_eq!(remove_file(&p), Err(Error::NotFound));
    }
}
