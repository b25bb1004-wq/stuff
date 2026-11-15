//! cgroup v2 groups: create a directory, write limits, move a process in, remove it when dropped.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Resource limits. `None` leaves that controller at its default (`max`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// `memory.max` in bytes.
    pub memory_max: Option<u64>,
    /// `pids.max`: most processes and threads in the group.
    pub pids_max: Option<u64>,
    /// `cpu.max` as (quota, period) in microseconds.
    pub cpu_max: Option<(u64, u64)>,
}

impl Limits {
    /// True when no limit is set, so no cgroup is needed.
    pub fn is_empty(&self) -> bool {
        self.memory_max.is_none() && self.pids_max.is_none() && self.cpu_max.is_none()
    }

    /// The interface files to write and their contents, in a fixed order.
    pub fn files(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(m) = self.memory_max {
            out.push(("memory.max", m.to_string()));
        }
        if let Some(p) = self.pids_max {
            out.push(("pids.max", p.to_string()));
        }
        if let Some((q, p)) = self.cpu_max {
            out.push(("cpu.max", format!("{q} {p}")));
        }
        out
    }

    /// Controllers the parent must enable in `cgroup.subtree_control` (as `+name` words).
    pub fn controllers(&self) -> String {
        let mut words = Vec::new();
        if self.memory_max.is_some() {
            words.push("+memory");
        }
        if self.pids_max.is_some() {
            words.push("+pids");
        }
        if self.cpu_max.is_some() {
            words.push("+cpu");
        }
        words.join(" ")
    }
}

/// A cgroup directory owned by this value; dropping it removes the directory (best effort).
#[derive(Debug)]
pub struct Cgroup {
    path: PathBuf,
}

impl Cgroup {
    /// Create `base/name`, enable the needed controllers in `base`, and write `limits`.
    /// `name` must be one plain path component.
    pub fn create(base: &Path, name: &str, limits: &Limits) -> io::Result<Cgroup> {
        let plain =
            !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']);
        if !plain {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cgroup name must be one path component",
            ));
        }
        let controllers = limits.controllers();
        if !controllers.is_empty() {
            fs::write(base.join("cgroup.subtree_control"), controllers)?;
        }
        let path = base.join(name);
        fs::create_dir(&path)?;
        let cg = Cgroup { path };
        for (file, value) in limits.files() {
            fs::write(cg.path.join(file), value)?;
        }
        Ok(cg)
    }

    /// Move process `pid` (and its future children) into the group.
    pub fn add(&self, pid: u32) -> io::Result<()> {
        fs::write(self.path.join("cgroup.procs"), pid.to_string())
    }

    /// The group's directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        // cgroupfs removes a group with rmdir once it has no processes; on a plain directory this fails, ignored.
        let _ = fs::remove_dir(&self.path);
    }
}
