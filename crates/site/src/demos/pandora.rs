//! Pandora container spec parsing and cgroup v2 validation demo (no execution).

use std::fmt::Write as _;

use pandora::{parse_size, Spec};

use super::{check_input, Demo, Output};

const MAX_LINES: usize = 64;

const DEFAULT_SPEC: &str = "\
rootfs /var/lib/pandora/alpine
hostname monolith-box
memory 64M
pids 32
cpu 50000/100000
env PATH=/usr/bin:/bin
env TERM=xterm-256color
cmd /bin/sh -c id";

/// Registered Pandora container spec parser and validator demo.
pub const DEMO: Demo = Demo {
    id: "pandora",
    title: "Pandora Container",
    default_input: DEFAULT_SPEC,
    run,
};

/// Parse and validate a container spec and print its cgroup v2 limits (nothing runs).
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let spec = parse_spec(input)?;
    spec.validate().map_err(|e| e.to_string())?;

    let mut out = String::new();
    out.push_str("validated: ok (dry-run; nothing executed)\n");
    let _ = writeln!(out, "rootfs: {}", spec.rootfs.display());
    let _ = writeln!(out, "hostname: {}", spec.hostname);
    let _ = writeln!(out, "argv: {}", spec.argv.join(" "));
    if spec.env.is_empty() {
        out.push_str("env: (none)\n");
    } else {
        let _ = writeln!(out, "env ({}):", spec.env.len());
        for (k, v) in &spec.env {
            let _ = writeln!(out, "  {k}={v}");
        }
    }

    out.push_str("=== cgroup v2 limits ===\n");
    if spec.limits.is_empty() {
        out.push_str("limits: none (no cgroup created)\n");
    } else {
        let _ = writeln!(out, "controllers: {}", spec.limits.controllers());
        for (file, val) in spec.limits.files() {
            let _ = writeln!(out, "  {file} = {val}");
        }
    }
    Ok(Output::Text(out))
}

fn parse_spec(input: &str) -> Result<Spec, String> {
    let mut spec = Spec::new("", Vec::new());
    let mut lines_seen: usize = 0;

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines_seen += 1;
        if lines_seen > MAX_LINES {
            return Err(format!("line {line_no}: exceeds {MAX_LINES} lines"));
        }

        let (key, val) = match line.find(char::is_whitespace) {
            Some(pos) => (&line[..pos], line[pos..].trim()),
            None => (line, ""),
        };

        match key {
            "rootfs" | "--rootfs" => {
                if val.is_empty() {
                    return Err(format!("line {line_no}: `rootfs` requires a path"));
                }
                spec.rootfs = val.into();
            }
            "hostname" | "--hostname" => {
                spec.hostname = val.to_owned();
            }
            "memory" | "--memory" => {
                spec.limits.memory_max = parse_size(val).map_err(|e| e.to_string())?;
            }
            "pids" | "--pids" => {
                spec.limits.pids_max = parse_size(val).map_err(|e| e.to_string())?;
            }
            "cpu" | "--cpu" => {
                let (q_s, p_s) = val
                    .split_once('/')
                    .ok_or_else(|| format!("line {line_no}: `cpu` needs QUOTA/PERIOD"))?;
                let q = q_s
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| format!("line {line_no}: invalid cpu quota `{}`", q_s.trim()))?;
                let p = p_s
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| format!("line {line_no}: invalid cpu period `{}`", p_s.trim()))?;
                spec.limits.cpu_max = Some((q, p));
            }
            "env" | "--env" => {
                let (k, v) = val
                    .split_once('=')
                    .ok_or_else(|| format!("line {line_no}: `env` needs KEY=VALUE"))?;
                spec.env.push((k.to_owned(), v.to_owned()));
            }
            "cmd" | "argv" | "--" => {
                let parts: Vec<String> = val.split_whitespace().map(str::to_owned).collect();
                if parts.is_empty() {
                    return Err(format!("line {line_no}: `{key}` requires at least one arg"));
                }
                spec.argv = parts;
            }
            other => {
                return Err(format!("line {line_no}: unknown directive `{other}`"));
            }
        }
    }

    if lines_seen == 0 {
        return Err(String::from("empty container spec"));
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn validates_limits_and_rejects_bad_cpu() {
        let Output::Text(txt) = run(DEFAULT_SPEC).unwrap() else {
            panic!("expected text");
        };
        assert!(txt.contains("memory.max = 67108864"));
        assert!(txt.contains("pids.max = 32"));
        assert!(txt.contains("cpu.max = 50000 100000"));
        assert!(run("rootfs /root\ncpu 0/100000\ncmd /bin/sh").is_err());
    }
}
