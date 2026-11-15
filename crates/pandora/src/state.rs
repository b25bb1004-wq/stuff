//! Container store state: `store/containers/<id>/state` and `rootfs/`.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Lifecycle status of a container in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Directory and rootfs created, process not yet started.
    Created,
    /// Container process is running.
    Running,
    /// Container process has terminated.
    Exited,
}

impl Status {
    /// Return the canonical lowercase status string stored in `state`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Exited => "exited",
        }
    }

    fn parse(s: &str) -> io::Result<Self> {
        match s {
            "created" => Ok(Self::Created),
            "running" => Ok(Self::Running),
            "exited" => Ok(Self::Exited),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown container status: {s:?}"),
            )),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Persisted state of a single container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerState {
    /// Container identifier (directory name under `store/containers/`).
    pub id: String,
    /// Image name (or `-` when started directly from `--rootfs`).
    pub image: String,
    /// Current lifecycle status.
    pub status: Status,
    /// Host PID of the container's PID 1, once forked.
    pub pid: Option<i32>,
    /// Exit code (128 + signal when killed, or -1 if reaped externally).
    pub exit: Option<i32>,
}

impl ContainerState {
    /// Format the status column for `pandora ps`.
    pub fn ps_status(&self) -> String {
        match self.status {
            Status::Created => "created".to_string(),
            Status::Running => "running".to_string(),
            Status::Exited => format!("exited({})", self.exit.unwrap_or(-1)),
        }
    }
}

/// Validate a container id (hostname rules, at most 32 ASCII bytes, no leading `.` or `-`).
pub fn validate_id(id: &str) -> io::Result<()> {
    let b = id.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 32
        && b[0] != b'-'
        && b[0] != b'.'
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'.');
    if !ok {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid container id {id:?}: must be 1-32 hostname chars"),
        ));
    }
    Ok(())
}

static LAST_ID_NANOS: Mutex<u128> = Mutex::new(0);

/// Generate a unique container id of the form `c<unix-nanos-hex>`.
pub fn generate_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut guard = LAST_ID_NANOS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = now.max(guard.saturating_add(1));
    *guard = next;
    format!("c{next:x}")
}

/// Path to `store/containers/<id>`.
pub fn container_dir(store: &Path, id: &str) -> PathBuf {
    store.join("containers").join(id)
}

/// Path to `store/containers/<id>/rootfs`.
pub fn rootfs_dir(store: &Path, id: &str) -> PathBuf {
    container_dir(store, id).join("rootfs")
}

fn format_state(st: &ContainerState) -> io::Result<String> {
    if st.image.contains(['\n', '\r', '\0']) || st.image.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid image name for state",
        ));
    }
    let mut out = format!("image={}\nstatus={}\n", st.image, st.status);
    if let Some(pid) = st.pid {
        out.push_str(&format!("pid={pid}\n"));
    }
    if let Some(code) = st.exit {
        out.push_str(&format!("exit={code}\n"));
    }
    Ok(out)
}

fn parse_state(id: &str, text: &str) -> io::Result<ContainerState> {
    let mut image: Option<String> = None;
    let mut status: Option<Status> = None;
    let mut pid: Option<i32> = None;
    let mut exit: Option<i32> = None;

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("malformed state line: {line:?}"),
            ));
        };
        match k {
            "image" => image = Some(v.to_string()),
            "status" => status = Some(Status::parse(v)?),
            "pid" if !v.is_empty() => {
                let p = v.parse::<i32>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("bad pid: {v:?}"))
                })?;
                pid = Some(p);
            }
            "exit" if !v.is_empty() => {
                let c = v.parse::<i32>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("bad exit: {v:?}"))
                })?;
                exit = Some(c);
            }
            _ => {}
        }
    }

    let image = image
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing image in state"))?;
    let status = status
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing status in state"))?;
    let mut st = ContainerState {
        id: id.to_string(),
        image,
        status,
        pid,
        exit,
    };
    reconcile_liveness(&mut st);
    Ok(st)
}

#[cfg(target_os = "linux")]
fn reconcile_liveness(st: &mut ContainerState) {
    if st.status == Status::Running {
        let alive = matches!(st.pid, Some(p) if p > 0 && Path::new(&format!("/proc/{p}")).exists());
        if !alive {
            st.status = Status::Exited;
            st.exit = Some(-1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn reconcile_liveness(_st: &mut ContainerState) {}

/// Create `store/containers/<id>/` with `rootfs/` and initial `created` state.
/// Fails with `AlreadyExists` if `id` already exists.
pub fn create(store: &Path, id: &str, image: &str) -> io::Result<ContainerState> {
    validate_id(id)?;
    let containers_dir = store.join("containers");
    fs::create_dir_all(&containers_dir)?;
    let dir = containers_dir.join(id);
    fs::create_dir(&dir)?;

    let init = (|| -> io::Result<ContainerState> {
        fs::create_dir(dir.join("rootfs"))?;
        let st = ContainerState {
            id: id.to_string(),
            image: image.to_string(),
            status: Status::Created,
            pid: None,
            exit: None,
        };
        write(store, &st)?;
        Ok(st)
    })();

    if init.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    init
}

/// Atomically write `state` to `store/containers/<id>/state` via `state.tmp` and rename.
pub fn write(store: &Path, state: &ContainerState) -> io::Result<()> {
    validate_id(&state.id)?;
    let dir = container_dir(store, &state.id);
    let body = format_state(state)?;
    let tmp = dir.join("state.tmp");
    let final_path = dir.join("state");

    let res = (|| -> io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &final_path)?;
        Ok(())
    })();

    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Read the container state for `id`, reconciling dead PIDs on Linux.
pub fn read(store: &Path, id: &str) -> io::Result<ContainerState> {
    validate_id(id)?;
    let path = container_dir(store, id).join("state");
    let text = fs::read_to_string(&path)?;
    parse_state(id, &text)
}

/// List all containers in `store`, sorted by `id`.
pub fn list(store: &Path) -> io::Result<Vec<ContainerState>> {
    let containers_dir = store.join("containers");
    let entries = match fs::read_dir(&containers_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry_res in entries {
        let entry = entry_res?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if validate_id(&id).is_err() {
            continue;
        }
        match read(store, &id) {
            Ok(st) => out.push(st),
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Remove a container directory from `store`, refusing if it is still running.
pub fn remove(store: &Path, id: &str) -> io::Result<()> {
    let st = read(store, id)?;
    if st.status == Status::Running {
        return Err(io::Error::other(format!(
            "container '{id}' is still running"
        )));
    }
    fs::remove_dir_all(container_dir(store, id))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn id_validation_and_generation() {
        assert!(validate_id("web-1.local").is_ok());
        assert!(validate_id(&"a".repeat(32)).is_ok());
        for bad in [
            "",
            ".",
            "..",
            ".hidden",
            "-dash",
            "a/b",
            "a\\b",
            "a b",
            &"a".repeat(33),
        ] {
            assert!(validate_id(bad).is_err(), "{bad:?}");
        }
        let id1 = generate_id();
        let id2 = generate_id();
        assert!(id1.starts_with('c'));
        assert!(id2.starts_with('c'));
        assert_ne!(id1, id2);
        assert!(validate_id(&id1).is_ok());
        assert!(validate_id(&id2).is_ok());
    }

    #[test]
    fn parser_never_panics() {
        assay::forall(1000, 0x71A2_0052, |r| {
            let mut b = vec![0u8; r.below(64)];
            r.fill(&mut b);
            let s = String::from_utf8_lossy(&b);
            let _ = validate_id(&s);
            let _ = parse_state("c1", &s);
        });
    }
}
