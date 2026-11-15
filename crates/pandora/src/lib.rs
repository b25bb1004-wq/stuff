//! pandora: a small Linux container runtime.
//!
//! A container is a process tree in fresh user, mount, PID, UTS, IPC and network namespaces, with its root
//! switched to a host directory by `pivot_root` and `/proc` mounted inside, optionally under a cgroup v2
//! group with memory, pids and cpu limits. Running needs Linux; the spec and cgroup logic build everywhere.
//! This crate may use `unsafe` (docs/SPEC.md rule 3): only in `sys`, and every block has a `// SAFETY:` line.

pub mod cgroup;
pub mod spec;

#[cfg(target_os = "linux")]
mod sys;

pub use cgroup::{Cgroup, Limits};
pub use spec::{parse_size, Spec, SpecError};

/// Run the container described by `spec` and return its exit code (128 + signal number when killed).
/// `cgroup_base` is the cgroup v2 directory the container's group is created in (`/sys/fs/cgroup` normally);
/// it is only touched when the spec has limits.
#[cfg(target_os = "linux")]
pub fn run(spec: &Spec, cgroup_base: &std::path::Path) -> std::io::Result<i32> {
    sys::run(spec, cgroup_base)
}

/// Containers need Linux namespaces; on other systems this always returns `Unsupported`.
#[cfg(not(target_os = "linux"))]
pub fn run(_spec: &Spec, _cgroup_base: &std::path::Path) -> std::io::Result<i32> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "pandora needs Linux namespaces",
    ))
}
