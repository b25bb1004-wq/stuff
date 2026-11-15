//! Image operations: pulling argo packages and assembling container root filesystems.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fs::{self, Metadata, OpenOptions};
use std::io;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};

use argo::client::{install, installed, InstallError};
use argo::manifest::Manifest;
use argo::solve::Solution;
use argo::version::{Req, Version};

/// Convert an [`InstallError`] into an [`io::Error`].
fn install_err_to_io(err: InstallError) -> io::Error {
    match err {
        InstallError::Io(e) => e,
        InstallError::Http(msg) => io::Error::other(msg),
        InstallError::Solve(e) => io::Error::new(io::ErrorKind::InvalidInput, e.to_string()),
        InstallError::Package(name, e) => {
            io::Error::new(io::ErrorKind::InvalidData, format!("{name}: {e}"))
        }
        InstallError::NotInstalled(name) => io::Error::new(
            io::ErrorKind::NotFound,
            format!("package '{name}' is not installed"),
        ),
        InstallError::InUse(deps) => io::Error::other(format!("in use by {}", deps.join(", "))),
    }
}

/// Pull `name` matching `req` from `registry` into `store`, verifying signatures against `trusted`.
pub fn pull(
    registry: SocketAddr,
    name: &str,
    req: &Req,
    store: &Path,
    trusted: &[[u8; 32]],
) -> io::Result<Solution> {
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "image name cannot be empty",
        ));
    }
    install(registry, &[(name.to_string(), req.clone())], store, trusted).map_err(install_err_to_io)
}

/// Return true if `rel` (slash-separated relative path) should be executable in a rootfs.
pub fn is_executable_path(rel: &str) -> bool {
    // Packages carry no file modes; shared objects need x too (the kernel exec-checks the ELF interpreter).
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.starts_with("bin/")
        || rel.starts_with("usr/bin/")
        || rel.ends_with(".sh")
        || name.contains(".so")
}

/// Validate a relative package file path, rejecting absolute paths, `..`, `.`, and control bytes.
pub(crate) fn validate_rel_path(rel: &str) -> io::Result<()> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.starts_with('\\')
        || rel.bytes().any(|b| b == b'\\' || b == b':' || b == 0)
        || rel.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid package path: {rel:?}"),
        ));
    }
    for comp in rel.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsafe path component in {rel:?}"),
            ));
        }
    }
    let p = Path::new(rel);
    for comp in p.components() {
        if !matches!(comp, Component::Normal(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("non-normal path component in {rel:?}"),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_mode_exec(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    (meta.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_mode_exec(_meta: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn apply_mode(path: &Path, exec: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if exec { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _exec: bool) -> io::Result<()> {
    Ok(())
}

struct PkgNode {
    ver: Version,
    deps: BTreeSet<String>,
}

struct PlannedFile {
    src: PathBuf,
    rel: String,
    exec: bool,
}

/// Collect `image` and its transitive dependencies from `installed`, validating manifests.
fn collect_subgraph(
    store: &Path,
    image: &str,
    installed_pkgs: &Solution,
) -> io::Result<BTreeMap<String, PkgNode>> {
    if !installed_pkgs.contains_key(image) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("image '{image}' is not installed"),
        ));
    }
    let mut graph: BTreeMap<String, PkgNode> = BTreeMap::new();
    let mut queue = VecDeque::new();
    queue.push_back(image.to_string());

    while let Some(name) = queue.pop_front() {
        if graph.contains_key(&name) {
            continue;
        }
        let ver = *installed_pkgs.get(&name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing dependency '{name}' in argo.lock"),
            )
        })?;
        let manifest_path = store.join("pkgs").join(format!("{name}-{ver}.manifest"));
        let text = fs::read_to_string(&manifest_path)?;
        let manifest = Manifest::parse(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if manifest.name != name || manifest.version != ver {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("manifest mismatch for {name} {ver}"),
            ));
        }
        let mut deps = BTreeSet::new();
        for (dep_name, dep_req) in &manifest.dependencies {
            let dep_ver = installed_pkgs.get(dep_name).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing dependency '{dep_name}' required by {name}"),
                )
            })?;
            if !dep_req.matches(dep_ver) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("installed {dep_name} {dep_ver} does not satisfy {dep_req}"),
                ));
            }
            if deps.insert(dep_name.clone()) && !graph.contains_key(dep_name) {
                queue.push_back(dep_name.clone());
            }
        }
        graph.insert(name, PkgNode { ver, deps });
    }
    Ok(graph)
}

/// Topologically sort packages: dependencies before dependents, ties broken by name, `image` last.
fn topo_sort(graph: &BTreeMap<String, PkgNode>, image: &str) -> io::Result<Vec<String>> {
    let mut in_degree: BTreeMap<String, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ready = BTreeSet::new();

    for (name, node) in graph {
        in_degree.insert(name.clone(), node.deps.len());
        if node.deps.is_empty() {
            ready.insert(name.clone());
        }
        for dep in &node.deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(name.clone());
        }
    }

    let mut order = Vec::with_capacity(graph.len());
    while let Some(name) = ready.pop_first() {
        order.push(name.clone());
        if let Some(children) = dependents.get(&name) {
            for child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        ready.insert(child.clone());
                    }
                }
            }
        }
    }

    if order.len() != graph.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "dependency cycle detected",
        ));
    }
    if order.last().map(String::as_str) != Some(image) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("image '{image}' has unexpected dependent"),
        ));
    }
    Ok(order)
}

/// Walk `pkg_dir` iteratively, validating every relative path and rejecting symlinks.
fn collect_pkg_files(pkg_dir: &Path, out: &mut Vec<PlannedFile>) -> io::Result<()> {
    let root_meta = fs::symlink_metadata(pkg_dir)?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid package directory: {}", pkg_dir.display()),
        ));
    }

    let mut stack = vec![(pkg_dir.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let mut entries: Vec<(String, PathBuf)> = Vec::new();
        for entry_res in fs::read_dir(&dir)? {
            let entry = entry_res?;
            let fname_os = entry.file_name();
            let fname = fname_os.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 file name in package")
            })?;
            entries.push((fname.to_string(), entry.path()));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (fname, path) in entries.into_iter().rev() {
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink not allowed in package: {fname}"),
                ));
            }
            let rel = if prefix.is_empty() {
                fname
            } else {
                format!("{prefix}/{fname}")
            };
            validate_rel_path(&rel)?;
            if meta.is_dir() {
                stack.push((path, rel));
            } else if meta.is_file() {
                let exec = is_executable_path(&rel) || is_mode_exec(&meta);
                out.push(PlannedFile {
                    src: path,
                    rel,
                    exec,
                });
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported file type in package: {rel}"),
                ));
            }
        }
    }
    Ok(())
}

/// Ensure all parent directories for `rel` exist under `dest` without traversing symlinks.
fn ensure_safe_parent(
    dest: &Path,
    canon_dest: &Path,
    rel: &str,
    verified_dirs: &mut HashSet<String>,
) -> io::Result<PathBuf> {
    let mut cur = dest.to_path_buf();
    let mut cur_rel = String::new();
    let parts: Vec<&str> = rel.split('/').collect();
    let parent_parts = &parts[..parts.len().saturating_sub(1)];

    for part in parent_parts {
        cur.push(part);
        if !cur_rel.is_empty() {
            cur_rel.push('/');
        }
        cur_rel.push_str(part);
        if verified_dirs.contains(&cur_rel) {
            continue;
        }
        match fs::symlink_metadata(&cur) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unsafe parent path in destination: {cur_rel}"),
                    ));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&cur)?;
            }
            Err(e) => return Err(e),
        }
        let canon = fs::canonicalize(&cur)?;
        if !canon.starts_with(canon_dest) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("destination escapes rootfs: {cur_rel}"),
            ));
        }
        verified_dirs.insert(cur_rel.clone());
    }

    if let Some(last) = parts.last() {
        cur.push(last);
    }
    Ok(cur)
}

/// Assemble `dest` from `image` and its transitive dependencies in `store`.
pub fn build_rootfs(store: &Path, image: &str, dest: &Path) -> io::Result<()> {
    let installed_pkgs = installed(store).map_err(install_err_to_io)?;
    let graph = collect_subgraph(store, image, &installed_pkgs)?;
    let order = topo_sort(&graph, image)?;

    let mut planned = Vec::new();
    for name in &order {
        let ver = graph
            .get(name)
            .ok_or_else(|| io::Error::other("missing package node"))?
            .ver;
        let pkg_dir = store.join("pkgs").join(format!("{name}-{ver}"));
        collect_pkg_files(&pkg_dir, &mut planned)?;
    }

    fs::create_dir_all(dest)?;
    let canon_dest = fs::canonicalize(dest)?;
    let mut verified_dirs = HashSet::new();

    for file in &planned {
        let dst = ensure_safe_parent(dest, &canon_dest, &file.rel, &mut verified_dirs)?;
        if let Ok(meta) = fs::symlink_metadata(&dst) {
            if meta.file_type().is_symlink() || meta.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("cannot overwrite non-regular destination: {}", file.rel),
                ));
            }
            fs::remove_file(&dst)?;
        }
        let mut src_file = fs::File::open(&file.src)?;
        let mut dst_file = OpenOptions::new().write(true).create_new(true).open(&dst)?;
        io::copy(&mut src_file, &mut dst_file)?;
        drop(dst_file);
        apply_mode(&dst, file.exec)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn executable_path_rules() {
        assert!(is_executable_path("bin/sh"));
        assert!(is_executable_path("usr/bin/env"));
        assert!(is_executable_path("app/run.sh"));
        assert!(!is_executable_path("etc/conf"));
        assert!(!is_executable_path("sbin/init"));
        assert!(is_executable_path("lib/libc.so.6"));
        assert!(is_executable_path("lib64/ld-linux-x86-64.so.2"));
    }

    #[test]
    fn rel_path_validation() {
        assert!(validate_rel_path("bin/tool").is_ok());
        assert!(validate_rel_path("app/a..b.sh").is_ok());
        assert!(validate_rel_path("app/foo -> bar").is_ok());
        for bad in [
            "", "/x", "../x", "a/../b", "./a", "a/./b", "a//b", "a/", "a\\b", "C:foo", "a\0b",
            "a\nb",
        ] {
            assert!(validate_rel_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rel_path_never_panics() {
        assay::forall(1000, 0x51A2_0052, |r| {
            let mut b = vec![0u8; r.below(48)];
            r.fill(&mut b);
            let s = String::from_utf8_lossy(&b);
            let _ = validate_rel_path(&s);
        });
    }
}
