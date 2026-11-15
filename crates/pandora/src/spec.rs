//! What to run: root directory, command, hostname, environment and resource limits.

use std::fmt;
use std::path::PathBuf;

use crate::cgroup::Limits;

/// A validated container description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// Host directory that becomes `/` inside the container. Must be absolute.
    pub rootfs: PathBuf,
    /// Program and arguments; `argv[0]` is a path inside the rootfs.
    pub argv: Vec<String>,
    /// Hostname inside the container's UTS namespace.
    pub hostname: String,
    /// `KEY=VALUE` pairs; the container gets exactly these and nothing from the host.
    pub env: Vec<(String, String)>,
    /// cgroup v2 limits (all `None` means no cgroup is created).
    pub limits: Limits,
}

/// Why a spec was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// The rootfs path is empty or relative.
    RootfsNotAbsolute,
    /// No command was given.
    NoCommand,
    /// An argument or environment entry contains a NUL byte.
    NulByte,
    /// Hostname is empty, longer than 64 bytes, or has a character outside `A-Z a-z 0-9 - .`.
    BadHostname,
    /// An environment key is empty or contains `=`.
    BadEnvKey,
    /// A size or count could not be parsed.
    BadNumber(String),
    /// cpu quota or period is zero, or the period is outside 1000..=1_000_000 microseconds.
    BadCpu,
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootfsNotAbsolute => write!(f, "rootfs must be an absolute path"),
            Self::NoCommand => write!(f, "no command given"),
            Self::NulByte => write!(f, "argument or environment contains a NUL byte"),
            Self::BadHostname => write!(f, "hostname must be 1-64 chars of A-Z a-z 0-9 - ."),
            Self::BadEnvKey => write!(f, "environment key is empty or contains '='"),
            Self::BadNumber(s) => write!(f, "bad number: {s:?}"),
            Self::BadCpu => write!(f, "cpu needs quota > 0 and period in 1000..=1000000"),
        }
    }
}

impl std::error::Error for SpecError {}

impl Spec {
    /// A spec with hostname `pandora`, no environment and no limits.
    pub fn new(rootfs: impl Into<PathBuf>, argv: Vec<String>) -> Spec {
        Spec {
            rootfs: rootfs.into(),
            argv,
            hostname: "pandora".into(),
            env: Vec::new(),
            limits: Limits::default(),
        }
    }

    /// Check every field; `run` refuses specs that fail this.
    pub fn validate(&self) -> Result<(), SpecError> {
        // shortcut: checks a leading '/' rather than Path::is_absolute so the rule is the Linux one on every host.
        if !self.rootfs.to_string_lossy().starts_with('/') {
            return Err(SpecError::RootfsNotAbsolute);
        }
        if self.argv.is_empty() || self.argv[0].is_empty() {
            return Err(SpecError::NoCommand);
        }
        let h = self.hostname.as_bytes();
        let host_ok = !h.is_empty()
            && h.len() <= 64
            && h[0] != b'-'
            && h.iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'.');
        if !host_ok {
            return Err(SpecError::BadHostname);
        }
        for (k, v) in &self.env {
            if k.is_empty() || k.contains('=') {
                return Err(SpecError::BadEnvKey);
            }
            if k.contains('\0') || v.contains('\0') {
                return Err(SpecError::NulByte);
            }
        }
        if self.argv.iter().any(|a| a.contains('\0'))
            || self.rootfs.to_string_lossy().contains('\0')
        {
            return Err(SpecError::NulByte);
        }
        if let Some((quota, period)) = self.limits.cpu_max {
            if quota == 0 || !(1000..=1_000_000).contains(&period) {
                return Err(SpecError::BadCpu);
            }
        }
        Ok(())
    }
}

/// Parse a byte size: decimal digits with an optional `K`, `M` or `G` suffix (powers of 1024).
/// `max` means no limit (`None`). Overflow, empty input and other text are errors.
pub fn parse_size(s: &str) -> Result<Option<u64>, SpecError> {
    if s == "max" {
        return Ok(None);
    }
    let bad = || SpecError::BadNumber(s.to_string());
    let (digits, mul) = match s.as_bytes().last() {
        Some(b'K' | b'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1 << 20),
        Some(b'G' | b'g') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u64 = digits.parse().map_err(|_| bad())?;
    n.checked_mul(mul).map(Some).ok_or_else(bad)
}
