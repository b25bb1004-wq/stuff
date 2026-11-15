//! Registry HTTP client and local package installation/removal.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use outcrop::{parse_response, write_request, Error as HttpError, Limits, Request};

use crate::lock;
use crate::manifest::Manifest;
use crate::pkg::{self, Package, PkgError};
use crate::solve::{self, Index, Solution, SolveError};
use crate::version::{Req, Version};

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Errors returned by [`install`], [`remove`], and [`installed`].
#[derive(Debug)]
pub enum InstallError {
    /// HTTP transport or non-2xx response status error.
    Http(String),
    /// Dependency resolution failed.
    Solve(SolveError),
    /// Package signature, path, or format verification failed.
    Package(String, PkgError),
    /// Local filesystem or socket I/O error.
    Io(io::Error),
    /// Requested package is not currently installed.
    NotInstalled(String),
    /// Package cannot be removed because other installed packages depend on it.
    InUse(Vec<String>),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::Http(msg) => write!(f, "http error: {msg}"),
            InstallError::Solve(e) => write!(f, "solve error: {e}"),
            InstallError::Package(name, e) => write!(f, "package '{name}' error: {e}"),
            InstallError::Io(e) => write!(f, "io error: {e}"),
            InstallError::NotInstalled(name) => write!(f, "package '{name}' is not installed"),
            InstallError::InUse(deps) => {
                write!(f, "package is in use by: {}", deps.join(", "))
            }
        }
    }
}

impl std::error::Error for InstallError {}

impl From<io::Error> for InstallError {
    fn from(err: io::Error) -> Self {
        InstallError::Io(err)
    }
}

fn http_get(addr: SocketAddr, target: &str) -> Result<Vec<u8>, InstallError> {
    let mut stream = TcpStream::connect(addr)?;
    let timeout = Duration::from_secs(30);
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let req = Request {
        method: "GET".to_string(),
        target: target.to_string(),
        minor: 1,
        headers: vec![
            ("Host".to_string(), addr.to_string().into_bytes()),
            ("Connection".to_string(), b"close".to_vec()),
        ],
        body: Vec::new(),
    };
    let wire = write_request(&req);
    stream.write_all(&wire)?;
    stream.flush()?;

    let limits = Limits {
        max_body: MAX_BODY_BYTES,
        ..Limits::default()
    };
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 8192];

    let resp = loop {
        match parse_response(&buf, &limits, false) {
            Ok((resp, _)) => break resp,
            Err(HttpError::Incomplete) => {
                let n = stream.read(&mut tmp)?;
                if n == 0 {
                    return Err(InstallError::Http(
                        "unexpected EOF reading HTTP response".to_string(),
                    ));
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            Err(e) => {
                return Err(InstallError::Http(format!(
                    "malformed HTTP response: {e:?}"
                )));
            }
        }
    };

    if !(200..300).contains(&resp.status) {
        return Err(InstallError::Http(format!(
            "HTTP {} {}",
            resp.status, resp.reason
        )));
    }
    Ok(resp.body)
}

fn parse_index(bytes: &[u8]) -> Result<Index, InstallError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| InstallError::Http("registry index is not valid UTF-8".to_string()))?;
    let mut index = Index::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let name = parts
            .next()
            .ok_or_else(|| InstallError::Http("missing name in index".to_string()))?;
        let ver_str = parts
            .next()
            .ok_or_else(|| InstallError::Http("missing version in index".to_string()))?;
        let ver = Version::parse(ver_str)
            .map_err(|e| InstallError::Http(format!("bad version in index: {e}")))?;

        let mut deps = Vec::new();
        while let Some(dep) = parts.next() {
            let req_str = parts
                .next()
                .ok_or_else(|| InstallError::Http("missing req in index".to_string()))?;
            let req = Req::parse(req_str)
                .map_err(|e| InstallError::Http(format!("bad req in index: {e}")))?;
            deps.push((dep.to_string(), req));
        }
        index.add(name, ver, deps);
    }
    Ok(index)
}

fn pkg_dir_path(root: &Path, name: &str, ver: Version) -> PathBuf {
    root.join("pkgs").join(format!("{name}-{ver}"))
}

fn pkg_manifest_path(root: &Path, name: &str, ver: Version) -> PathBuf {
    root.join("pkgs").join(format!("{name}-{ver}.manifest"))
}

fn write_package_to_disk(
    root: &Path,
    name: &str,
    ver: Version,
    package: &Package,
) -> io::Result<()> {
    let dir = pkg_dir_path(root, name, ver);
    fs::create_dir_all(&dir)?;
    for (rel_path, data) in &package.files {
        let mut dest = dir.clone();
        for comp in rel_path.split('/') {
            dest.push(comp);
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, data)?;
    }
    fs::write(
        pkg_manifest_path(root, name, ver),
        package.manifest.to_string(),
    )?;
    Ok(())
}

/// Resolve and install `deps` from `registry` into `root`, verifying all signatures against `trusted`.
pub fn install(
    registry: SocketAddr,
    deps: &[(String, Req)],
    root: &Path,
    trusted: &[[u8; 32]],
) -> Result<Solution, InstallError> {
    let index_bytes = http_get(registry, "/index")?;
    let index = parse_index(&index_bytes)?;

    let existing_lock = installed(root)?;
    let lock_opt = if existing_lock.is_empty() {
        None
    } else {
        Some(&existing_lock)
    };
    let solution = solve::solve(&index, deps, lock_opt).map_err(InstallError::Solve)?;

    let mut to_install: Vec<(String, Version, Package)> = Vec::new();
    for (name, &ver) in &solution {
        let already =
            existing_lock.get(name) == Some(&ver) && pkg_manifest_path(root, name, ver).exists();
        if already {
            continue;
        }
        let target = format!("/pkg/{name}/{ver}");
        let blob = http_get(registry, &target)?;
        let package =
            pkg::open(&blob, trusted).map_err(|e| InstallError::Package(name.clone(), e))?;
        if package.manifest.name != *name || package.manifest.version != ver {
            return Err(InstallError::Package(
                name.clone(),
                PkgError::Malformed(format!(
                    "manifest names {} {}, expected {} {}",
                    package.manifest.name, package.manifest.version, name, ver
                )),
            ));
        }
        to_install.push((name.clone(), ver, package));
    }

    // All downloads and verifications succeeded; now mutate `root`.
    fs::create_dir_all(root.join("pkgs"))?;
    for (name, ver, package) in &to_install {
        write_package_to_disk(root, name, *ver, package)?;
    }
    for (old_name, &old_ver) in &existing_lock {
        if solution.get(old_name) != Some(&old_ver) {
            let old_dir = pkg_dir_path(root, old_name, old_ver);
            if old_dir.exists() {
                fs::remove_dir_all(old_dir)?;
            }
            let old_manifest = pkg_manifest_path(root, old_name, old_ver);
            if old_manifest.exists() {
                fs::remove_file(old_manifest)?;
            }
        }
    }
    fs::write(root.join("cairn.lock"), lock::write(&solution))?;

    Ok(solution)
}

/// Remove an installed package `name` from `root` if no other installed package depends on it.
pub fn remove(root: &Path, name: &str) -> Result<(), InstallError> {
    let mut sol = installed(root)?;
    let Some(&ver) = sol.get(name) else {
        return Err(InstallError::NotInstalled(name.to_string()));
    };

    let mut dependents = Vec::new();
    for (other_name, &other_ver) in &sol {
        if other_name == name {
            continue;
        }
        let text = fs::read_to_string(pkg_manifest_path(root, other_name, other_ver))?;
        let manifest = Manifest::parse(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if manifest.dependencies.iter().any(|(d, _)| d == name) {
            dependents.push(other_name.clone());
        }
    }
    dependents.sort();
    if !dependents.is_empty() {
        return Err(InstallError::InUse(dependents));
    }

    let dir = pkg_dir_path(root, name, ver);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    let manifest_path = pkg_manifest_path(root, name, ver);
    if manifest_path.exists() {
        fs::remove_file(manifest_path)?;
    }
    sol.remove(name);
    fs::write(root.join("cairn.lock"), lock::write(&sol))?;
    Ok(())
}

/// Read the currently installed package solution from `cairn.lock` (empty if absent).
pub fn installed(root: &Path) -> Result<Solution, InstallError> {
    let lock_path = root.join("cairn.lock");
    let text = match fs::read_to_string(&lock_path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Solution::new()),
        Err(e) => return Err(InstallError::Io(e)),
    };
    let sol =
        lock::read(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(sol)
}
