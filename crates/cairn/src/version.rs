//! Semantic versions and Cargo-style version requirements.

use std::fmt;

/// Error returned when parsing a version, requirement, manifest, or lock file fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number, or 0 when not from a file.
    pub line: usize,
    /// Human-readable error description.
    pub msg: String,
}

impl ParseError {
    pub(crate) fn new(line: usize, msg: impl Into<String>) -> Self {
        Self {
            line,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.msg)
        } else {
            write!(f, "line {}: {}", self.line, self.msg)
        }
    }
}

impl std::error::Error for ParseError {}

/// A semantic version (`MAJOR.MINOR.PATCH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major version number.
    pub major: u64,
    /// Minor version number.
    pub minor: u64,
    /// Patch version number.
    pub patch: u64,
}

impl Version {
    /// Parse a strict `MAJOR.MINOR.PATCH` version string.
    pub fn parse(s: &str) -> Result<Version, ParseError> {
        let mut parts = s.split('.');
        let major_s = parts
            .next()
            .ok_or_else(|| ParseError::new(0, "empty version"))?;
        let minor_s = parts
            .next()
            .ok_or_else(|| ParseError::new(0, "version must have three components"))?;
        let patch_s = parts
            .next()
            .ok_or_else(|| ParseError::new(0, "version must have three components"))?;
        if parts.next().is_some() {
            return Err(ParseError::new(0, "version has too many components"));
        }
        Ok(Version {
            major: parse_component(major_s)?,
            minor: parse_component(minor_s)?,
            patch: parse_component(patch_s)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn parse_component(s: &str) -> Result<u64, ParseError> {
    if s.is_empty() {
        return Err(ParseError::new(0, "empty version component"));
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::new(
            0,
            format!("invalid version component '{s}'"),
        ));
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(ParseError::new(
            0,
            format!("leading zero in version component '{s}'"),
        ));
    }
    s.parse::<u64>()
        .map_err(|_| ParseError::new(0, format!("version component '{s}' overflows u64")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Bare,
    Caret,
    Tilde,
    Exact,
    Gt,
    Ge,
    Lt,
    Le,
}

impl Op {
    fn prefix(self) -> &'static str {
        match self {
            Op::Bare => "",
            Op::Caret => "^",
            Op::Tilde => "~",
            Op::Exact => "=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartialVer {
    Major(u64),
    MajorMinor(u64, u64),
    Full(u64, u64, u64),
}

impl fmt::Display for PartialVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            PartialVer::Major(m) => write!(f, "{m}"),
            PartialVer::MajorMinor(m, n) => write!(f, "{m}.{n}"),
            PartialVer::Full(m, n, p) => write!(f, "{m}.{n}.{p}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparator {
    Op(Op, PartialVer),
    WildAny,
    WildMajor(u64),
    WildMinor(u64, u64),
}

impl Comparator {
    fn parse(raw: &str) -> Result<Self, ParseError> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(ParseError::new(0, "empty comparator"));
        }
        let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
            (Some(Op::Ge), r)
        } else if let Some(r) = s.strip_prefix("<=") {
            (Some(Op::Le), r)
        } else if let Some(r) = s.strip_prefix('^') {
            (Some(Op::Caret), r)
        } else if let Some(r) = s.strip_prefix('~') {
            (Some(Op::Tilde), r)
        } else if let Some(r) = s.strip_prefix('=') {
            (Some(Op::Exact), r)
        } else if let Some(r) = s.strip_prefix('>') {
            (Some(Op::Gt), r)
        } else if let Some(r) = s.strip_prefix('<') {
            (Some(Op::Lt), r)
        } else {
            (None, s)
        };

        let ver_str = rest.trim_start();
        if ver_str.is_empty() {
            return Err(ParseError::new(0, "missing version after operator"));
        }

        if op.is_none() && ver_str.contains('*') {
            return parse_wildcard(ver_str);
        }

        let partial = parse_partial(ver_str)?;
        Ok(Comparator::Op(op.unwrap_or(Op::Bare), partial))
    }

    fn matches(&self, v: &Version) -> bool {
        match *self {
            Comparator::WildAny => true,
            Comparator::WildMajor(m) => v.major == m,
            Comparator::WildMinor(m, n) => v.major == m && v.minor == n,
            Comparator::Op(op, pv) => match op {
                Op::Bare | Op::Caret => match pv {
                    PartialVer::Major(m) => v.major == m,
                    PartialVer::MajorMinor(m, n) => {
                        if m > 0 {
                            v.major == m && v.minor >= n
                        } else {
                            v.major == 0 && v.minor == n
                        }
                    }
                    PartialVer::Full(m, n, p) => {
                        if m > 0 {
                            v.major == m && (v.minor, v.patch) >= (n, p)
                        } else if n > 0 {
                            v.major == 0 && v.minor == n && v.patch >= p
                        } else {
                            v.major == 0 && v.minor == 0 && v.patch == p
                        }
                    }
                },
                Op::Tilde => match pv {
                    PartialVer::Major(m) => v.major == m,
                    PartialVer::MajorMinor(m, n) => v.major == m && v.minor == n,
                    PartialVer::Full(m, n, p) => v.major == m && v.minor == n && v.patch >= p,
                },
                Op::Exact => match pv {
                    PartialVer::Major(m) => v.major == m,
                    PartialVer::MajorMinor(m, n) => v.major == m && v.minor == n,
                    PartialVer::Full(m, n, p) => (v.major, v.minor, v.patch) == (m, n, p),
                },
                Op::Gt => match pv {
                    PartialVer::Major(m) => v.major > m,
                    PartialVer::MajorMinor(m, n) => v.major > m || (v.major == m && v.minor > n),
                    PartialVer::Full(m, n, p) => (v.major, v.minor, v.patch) > (m, n, p),
                },
                Op::Ge => match pv {
                    PartialVer::Major(m) => v.major >= m,
                    PartialVer::MajorMinor(m, n) => (v.major, v.minor) >= (m, n),
                    PartialVer::Full(m, n, p) => (v.major, v.minor, v.patch) >= (m, n, p),
                },
                Op::Lt => match pv {
                    PartialVer::Major(m) => v.major < m,
                    PartialVer::MajorMinor(m, n) => (v.major, v.minor) < (m, n),
                    PartialVer::Full(m, n, p) => (v.major, v.minor, v.patch) < (m, n, p),
                },
                Op::Le => match pv {
                    PartialVer::Major(m) => v.major <= m,
                    PartialVer::MajorMinor(m, n) => v.major < m || (v.major == m && v.minor <= n),
                    PartialVer::Full(m, n, p) => (v.major, v.minor, v.patch) <= (m, n, p),
                },
            },
        }
    }
}

impl fmt::Display for Comparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Comparator::WildAny => write!(f, "*"),
            Comparator::WildMajor(m) => write!(f, "{m}.*"),
            Comparator::WildMinor(m, n) => write!(f, "{m}.{n}.*"),
            Comparator::Op(op, pv) => write!(f, "{}{}", op.prefix(), pv),
        }
    }
}

fn parse_wildcard(s: &str) -> Result<Comparator, ParseError> {
    if s == "*" {
        return Ok(Comparator::WildAny);
    }
    let parts: Vec<&str> = s.split('.').collect();
    match parts.as_slice() {
        [m, "*"] => Ok(Comparator::WildMajor(parse_component(m)?)),
        [m, n, "*"] => Ok(Comparator::WildMinor(
            parse_component(m)?,
            parse_component(n)?,
        )),
        _ => Err(ParseError::new(0, format!("invalid wildcard '{s}'"))),
    }
}

fn parse_partial(s: &str) -> Result<PartialVer, ParseError> {
    let mut parts = s.split('.');
    let m = parts
        .next()
        .ok_or_else(|| ParseError::new(0, "empty version"))?;
    let major = parse_component(m)?;
    let Some(n) = parts.next() else {
        return Ok(PartialVer::Major(major));
    };
    let minor = parse_component(n)?;
    let Some(p) = parts.next() else {
        return Ok(PartialVer::MajorMinor(major, minor));
    };
    let patch = parse_component(p)?;
    if parts.next().is_some() {
        return Err(ParseError::new(0, "version has too many components"));
    }
    Ok(PartialVer::Full(major, minor, patch))
}

/// A Cargo-style version requirement consisting of one or more comparators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Req {
    comparators: Vec<Comparator>,
}

impl Req {
    /// Parse a version requirement string.
    pub fn parse(s: &str) -> Result<Req, ParseError> {
        let mut comparators = Vec::new();
        for part in s.split(',') {
            comparators.push(Comparator::parse(part)?);
        }
        if comparators.is_empty() {
            return Err(ParseError::new(0, "empty requirement"));
        }
        Ok(Req { comparators })
    }

    /// Return true if `v` satisfies every comparator in this requirement.
    pub fn matches(&self, v: &Version) -> bool {
        self.comparators.iter().all(|c| c.matches(v))
    }
}

impl fmt::Display for Req {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, c) in self.comparators.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}
