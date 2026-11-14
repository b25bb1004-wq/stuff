//! Package manifest (`[package]` and `[dependencies]`) parser and formatter.

use std::fmt;

use crate::version::{ParseError, Req, Version};

/// A parsed package manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Package name (`[a-z][a-z0-9_-]*`).
    pub name: String,
    /// Package version.
    pub version: Version,
    /// Ordered list of `(dependency_name, requirement)` pairs.
    pub dependencies: Vec<(String, Req)>,
}

pub(crate) fn is_valid_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub(crate) fn parse_kv(trimmed: &str, line_no: usize) -> Result<(&str, &str), ParseError> {
    let Some((left, right)) = trimmed.split_once('=') else {
        return Err(ParseError::new(line_no, "expected key = \"value\""));
    };
    let key = left.trim();
    let val_raw = right.trim();
    if val_raw.len() < 2 || !val_raw.starts_with('"') || !val_raw.ends_with('"') {
        return Err(ParseError::new(
            line_no,
            "value must be enclosed in double quotes",
        ));
    }
    let val = &val_raw[1..val_raw.len() - 1];
    if val.contains('"') {
        return Err(ParseError::new(line_no, "unexpected quote inside value"));
    }
    Ok((key, val))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Package,
    Dependencies,
}

impl Manifest {
    /// Parse a manifest from text.
    pub fn parse(text: &str) -> Result<Manifest, ParseError> {
        let mut section: Option<Section> = None;
        let mut seen_package = false;
        let mut seen_deps = false;
        let mut name: Option<String> = None;
        let mut version: Option<Version> = None;
        let mut dependencies: Vec<(String, Req)> = Vec::new();
        let mut line_count: usize = 0;

        for (idx, raw_line) in text.lines().enumerate() {
            let line_no = idx + 1;
            line_count = line_no;
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') {
                match trimmed {
                    "[package]" => {
                        if seen_package || seen_deps {
                            return Err(ParseError::new(
                                line_no,
                                "[package] must appear once and first",
                            ));
                        }
                        seen_package = true;
                        section = Some(Section::Package);
                    }
                    "[dependencies]" => {
                        if !seen_package {
                            return Err(ParseError::new(
                                line_no,
                                "[package] must appear before [dependencies]",
                            ));
                        }
                        if seen_deps {
                            return Err(ParseError::new(
                                line_no,
                                "duplicate [dependencies] section",
                            ));
                        }
                        seen_deps = true;
                        section = Some(Section::Dependencies);
                    }
                    _ => {
                        return Err(ParseError::new(
                            line_no,
                            format!("unknown section '{trimmed}'"),
                        ));
                    }
                }
                continue;
            }

            let Some(cur_sec) = section else {
                return Err(ParseError::new(line_no, "key outside of any section"));
            };
            let (key, val) = parse_kv(trimmed, line_no)?;
            match cur_sec {
                Section::Package => match key {
                    "name" => {
                        if name.is_some() {
                            return Err(ParseError::new(line_no, "duplicate key 'name'"));
                        }
                        if !is_valid_name(val) {
                            return Err(ParseError::new(
                                line_no,
                                format!("invalid package name '{val}'"),
                            ));
                        }
                        name = Some(val.to_string());
                    }
                    "version" => {
                        if version.is_some() {
                            return Err(ParseError::new(line_no, "duplicate key 'version'"));
                        }
                        let v = Version::parse(val).map_err(|e| ParseError::new(line_no, e.msg))?;
                        version = Some(v);
                    }
                    _ => {
                        return Err(ParseError::new(
                            line_no,
                            format!("unknown key '{key}' in [package]"),
                        ));
                    }
                },
                Section::Dependencies => {
                    if !is_valid_name(key) {
                        return Err(ParseError::new(
                            line_no,
                            format!("invalid dependency name '{key}'"),
                        ));
                    }
                    if dependencies.iter().any(|(d, _)| d == key) {
                        return Err(ParseError::new(
                            line_no,
                            format!("duplicate dependency '{key}'"),
                        ));
                    }
                    let req = Req::parse(val).map_err(|e| ParseError::new(line_no, e.msg))?;
                    dependencies.push((key.to_string(), req));
                }
            }
        }

        let last_line = line_count.max(1);
        if !seen_package {
            return Err(ParseError::new(last_line, "missing [package] section"));
        }
        let name = name.ok_or_else(|| ParseError::new(last_line, "missing 'name' in [package]"))?;
        let version =
            version.ok_or_else(|| ParseError::new(last_line, "missing 'version' in [package]"))?;

        Ok(Manifest {
            name,
            version,
            dependencies,
        })
    }
}

impl fmt::Display for Manifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "[package]")?;
        writeln!(f, "name = \"{}\"", self.name)?;
        writeln!(f, "version = \"{}\"", self.version)?;
        if !self.dependencies.is_empty() {
            writeln!(f)?;
            writeln!(f, "[dependencies]")?;
            for (name, req) in &self.dependencies {
                writeln!(f, "{name} = \"{req}\"")?;
            }
        }
        Ok(())
    }
}
