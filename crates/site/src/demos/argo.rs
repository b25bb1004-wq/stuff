//! Argo package dependency solver and lockfile generator demo.

use argo::lock;
use argo::manifest::Manifest;
use argo::solve::{solve, Index};
use argo::version::{Req, Version};

use super::{check_input, Demo, Output};

const MAX_INDEX_LINES: usize = 256;

const DEFAULT_ARGO: &str = "\
[index]
alpha 1.0.0 : beta ^1.0
alpha 1.1.0 : beta ^1.1
beta 1.0.0
beta 1.1.0 : gamma ^2.0
gamma 2.0.0
gamma 2.1.0

[manifest]
[package]
name = \"demo-app\"
version = \"0.1.0\"

[dependencies]
alpha = \"^1.0\"
gamma = \"2.0.*\"";

/// Registered Argo package dependency solver demo.
pub const DEMO: Demo = Demo {
    id: "argo",
    title: "Argo Package Solver",
    default_input: DEFAULT_ARGO,
    run,
};

/// Parse `[index]` and `[manifest]` sections, solve dependencies, and emit an Argo lock file.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let (index_text, manifest_text) = split_sections(input)?;
    let index = parse_index(&index_text)?;
    let manifest = Manifest::parse(&manifest_text).map_err(|e| format!("manifest error: {e}"))?;

    let solution =
        solve(&index, &manifest.dependencies, None).map_err(|e| format!("solver conflict: {e}"))?;

    let lock_text = lock::write(&solution);
    let out = format!(
        "package: {} {}\n{}",
        manifest.name, manifest.version, lock_text
    );
    Ok(Output::Text(out))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TopSection {
    Index,
    Manifest,
}

fn split_sections(input: &str) -> Result<(String, String), String> {
    let mut cur: Option<TopSection> = None;
    let mut seen_index = false;
    let mut seen_manifest = false;
    let mut index_lines = Vec::new();
    let mut manifest_lines = Vec::new();

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw.trim();
        if trimmed == "[index]" {
            if seen_index {
                return Err(format!("line {line_no}: duplicate [index] section"));
            }
            seen_index = true;
            cur = Some(TopSection::Index);
            continue;
        }
        if trimmed == "[manifest]" {
            if seen_manifest {
                return Err(format!("line {line_no}: duplicate [manifest] section"));
            }
            seen_manifest = true;
            cur = Some(TopSection::Manifest);
            continue;
        }
        match cur {
            Some(TopSection::Index) => index_lines.push((line_no, raw)),
            Some(TopSection::Manifest) => manifest_lines.push(raw),
            None => {
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    return Err(format!(
                        "line {line_no}: content outside [index] or [manifest]"
                    ));
                }
            }
        }
    }

    if !seen_index {
        return Err(String::from("missing [index] section"));
    }
    if !seen_manifest {
        return Err(String::from("missing [manifest] section"));
    }

    let mut index_buf = String::new();
    for (line_no, line) in index_lines {
        use std::fmt::Write as _;
        let _ = writeln!(index_buf, "{line_no}\t{line}");
    }
    let manifest_buf = manifest_lines.join("\n");
    Ok((index_buf, manifest_buf))
}

fn parse_index(numbered_lines: &str) -> Result<Index, String> {
    let mut index = Index::new();
    let mut entries = 0usize;

    for raw_entry in numbered_lines.lines() {
        let Some((line_no_str, raw_line)) = raw_entry.split_once('\t') else {
            continue;
        };
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        entries += 1;
        if entries > MAX_INDEX_LINES {
            return Err(format!(
                "line {line_no_str}: index exceeds {MAX_INDEX_LINES} entries"
            ));
        }

        let (head, deps_part) = line
            .split_once(':')
            .map_or((line, ""), |(l, r)| (l.trim(), r.trim()));
        let mut head_parts = head.split_whitespace();
        let Some(pkg) = head_parts.next() else {
            return Err(format!("line {line_no_str}: missing package name"));
        };
        let Some(ver_str) = head_parts.next() else {
            return Err(format!("line {line_no_str}: missing version for `{pkg}`"));
        };
        if head_parts.next().is_some() {
            return Err(format!(
                "line {line_no_str}: unexpected extra token in `{head}`"
            ));
        }
        let ver = Version::parse(ver_str)
            .map_err(|e| format!("line {line_no_str}: invalid version `{ver_str}`: {e}"))?;

        let mut deps = Vec::new();
        if !deps_part.is_empty() {
            for dep_raw in deps_part.split(';') {
                let dep = dep_raw.trim();
                if dep.is_empty() {
                    continue;
                }
                let Some((dep_name, req_raw)) = split_dep_spec(dep) else {
                    return Err(format!("line {line_no_str}: expected `dep req` in `{dep}`"));
                };
                let req_clean = req_raw.trim().trim_matches('"');
                let req = Req::parse(req_clean)
                    .map_err(|e| format!("line {line_no_str}: bad req `{req_clean}`: {e}"))?;
                deps.push((dep_name.to_owned(), req));
            }
        }
        index.add(pkg, ver, deps);
    }

    Ok(index)
}

fn split_dep_spec(spec: &str) -> Option<(&str, &str)> {
    if let Some((l, r)) = spec.split_once('=') {
        let name = l.trim();
        let req = r.trim();
        if !name.is_empty() && !req.is_empty() {
            return Some((name, req));
        }
    }
    let idx = spec.find(|c: char| c.is_ascii_whitespace())?;
    let name = spec[..idx].trim();
    let req = spec[idx..].trim();
    if name.is_empty() || req.is_empty() {
        None
    } else {
        Some((name, req))
    }
}
