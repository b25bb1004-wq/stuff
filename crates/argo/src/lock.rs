//! Lock file reader and writer (`# argo lock v1`).

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::manifest::{is_valid_name, parse_kv};
use crate::solve::Solution;
use crate::version::{ParseError, Version};

const LOCK_HEADER: &str = "# argo lock v1";

/// Format a solution as a deterministic lock file.
pub fn write(s: &Solution) -> String {
    let mut out = String::from(LOCK_HEADER);
    out.push('\n');
    for (name, version) in s {
        let _ = writeln!(out, "{name} = \"{version}\"");
    }
    out
}

/// Parse a lock file into a `Solution`.
pub fn read(text: &str) -> Result<Solution, ParseError> {
    let mut lines = text.lines().enumerate();
    let Some((_, first)) = lines.next() else {
        return Err(ParseError::new(1, "empty lock file; expected header"));
    };
    if first != LOCK_HEADER {
        return Err(ParseError::new(
            1,
            format!("invalid lock header; expected '{LOCK_HEADER}'"),
        ));
    }

    let mut solution = BTreeMap::new();
    for (idx, raw_line) in lines {
        let line_no = idx + 1;
        if raw_line.trim().is_empty() {
            continue;
        }
        let (key, val) = parse_kv(raw_line.trim(), line_no)?;
        if !is_valid_name(key) {
            return Err(ParseError::new(
                line_no,
                format!("invalid package name '{key}'"),
            ));
        }
        if solution.contains_key(key) {
            return Err(ParseError::new(
                line_no,
                format!("duplicate package '{key}' in lock file"),
            ));
        }
        let ver = Version::parse(val).map_err(|e| ParseError::new(line_no, e.msg))?;
        solution.insert(key.to_string(), ver);
    }
    Ok(solution)
}
