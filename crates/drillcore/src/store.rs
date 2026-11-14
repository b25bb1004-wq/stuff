//! Flat key-value storage backends and path/branch validation.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use crate::id::Error;

/// Map of repository-relative file path to `(bytes, executable)`.
pub type Files = BTreeMap<String, (Vec<u8>, bool)>;

/// Flat key-value namespace of `/`-separated paths.
pub trait Store {
    /// Read the bytes at `path`, or `None` if absent.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// Write `data` to `path`, creating or overwriting it.
    fn write(&mut self, path: &str, data: &[u8]);
    /// Remove `path` if present.
    fn remove(&mut self, path: &str);
    /// Return all stored paths starting with `prefix`, sorted lexicographically.
    fn list(&self, prefix: &str) -> Vec<String>;
}

/// In-memory `Store` backed by a `BTreeMap`.
#[derive(Default, Debug, Clone)]
pub struct MemStore {
    entries: BTreeMap<String, Vec<u8>>,
}

impl MemStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for MemStore {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.entries.get(path).cloned()
    }

    fn write(&mut self, path: &str, data: &[u8]) {
        self.entries.insert(path.to_string(), data.to_vec());
    }

    fn remove(&mut self, path: &str) {
        self.entries.remove(path);
    }

    fn list(&self, prefix: &str) -> Vec<String> {
        self.entries
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }
}

/// Filesystem-backed `Store` rooted at a directory.
#[cfg(feature = "std")]
#[derive(Debug, Clone)]
pub struct DirStore {
    root: std::path::PathBuf,
}

#[cfg(feature = "std")]
impl DirStore {
    /// Create a `DirStore` rooted at `root`.
    pub fn new(root: impl AsRef<std::path::Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    fn disk_path(&self, rel: &str) -> std::path::PathBuf {
        let mut p = self.root.clone();
        for comp in rel.split('/') {
            if !comp.is_empty() {
                p.push(comp);
            }
        }
        p
    }
}

#[cfg(feature = "std")]
impl Store for DirStore {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.disk_path(path)).ok()
    }

    fn write(&mut self, path: &str, data: &[u8]) {
        let p = self.disk_path(path);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, data);
    }

    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(self.disk_path(path));
    }

    fn list(&self, prefix: &str) -> Vec<String> {
        let mut out = Vec::new();
        collect_dir(&self.root, "", prefix, &mut out);
        out.sort();
        out
    }
}

#[cfg(feature = "std")]
fn collect_dir(dir: &std::path::Path, rel_prefix: &str, filter: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let rel = if rel_prefix.is_empty() {
            name_str.to_string()
        } else {
            alloc::format!("{rel_prefix}/{name_str}")
        };
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_dir(&entry.path(), &rel, filter, out);
        } else if ft.is_file() && rel.starts_with(filter) {
            out.push(rel);
        }
    }
}

pub(crate) fn validate_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Err(Error::BadName);
    }
    for comp in path.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(Error::BadName);
        }
    }
    Ok(())
}

pub(crate) fn validate_branch_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with('.')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
    {
        return Err(Error::BadName);
    }
    for b in name.bytes() {
        let ok = matches!(
            b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'/' | b'-'
        );
        if !ok {
            return Err(Error::BadName);
        }
    }
    Ok(())
}
