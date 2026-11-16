//! Midas allocator demo: an alloc and free script run on the real `midas::Sim` heap.

use std::fmt::Write as _;

use midas::{Handle, Sim};

use super::{check_input, Demo, Output};

const DEFAULT_ARENA: usize = 1 << 20;
const MAX_ARENA: usize = 16 << 20;
const MAX_OPS: usize = 256;
const MAX_NAME: usize = 16;

const DEFAULT_SCRIPT: &str = "\
arena 1048576
alloc a 100
alloc b 200
alloc c 5000
free a
alloc d 64
free c
alloc e 4096";

/// Registered Midas allocator demo.
pub const DEMO: Demo = Demo {
    id: "midas",
    title: "Midas Allocator",
    default_input: DEFAULT_SCRIPT,
    run,
};

enum Op {
    Alloc { name: String, size: usize },
    Free { name: String },
}

struct Entry {
    name: String,
    handle: Handle,
    offset: usize,
    size: usize,
    freed: bool,
}

/// Run the script on a fresh arena and report every op, the block map and the heap stats.
///
/// Output lines (one fact each): `arena`, `op N alloc|free ...`, `block OFFSET SIZE live|freed NAME`,
/// `stats used BYTES live N freed N check ok|FAILED`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let (arena, ops) = parse(input)?;
    let mut sim =
        Sim::new(arena).map_err(|e| format!("cannot create a {arena} byte arena: {e:?}"))?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut out = String::new();
    let _ = writeln!(out, "arena {arena} usable {}", sim.total_bytes());

    for (n, op) in ops.iter().enumerate() {
        let step = n + 1;
        match op {
            Op::Alloc { name, size } => {
                if entries.iter().any(|e| e.name == *name && !e.freed) {
                    return Err(format!("op {step}: block `{name}` is already live"));
                }
                let handle = sim
                    .alloc(*size)
                    .map_err(|e| format!("op {step}: alloc {name} {size} failed: {e:?}"))?;
                let block = sim.block(handle).map_err(|e| format!("op {step}: {e:?}"))?;
                let _ = writeln!(
                    out,
                    "op {step} alloc {name} {size} offset {} size {}",
                    block.offset, block.size
                );
                entries.push(Entry {
                    name: name.clone(),
                    handle,
                    offset: block.offset,
                    size: block.size,
                    freed: false,
                });
            }
            Op::Free { name } => {
                let entry = entries
                    .iter_mut()
                    .rev()
                    .find(|e| e.name == *name && !e.freed)
                    .ok_or_else(|| format!("op {step}: no live block `{name}`"))?;
                sim.free(entry.handle)
                    .map_err(|e| format!("op {step}: free {name} failed: {e:?}"))?;
                entry.freed = true;
                let _ = writeln!(
                    out,
                    "op {step} free {name} offset {} size {}",
                    entry.offset, entry.size
                );
            }
        }
    }

    for e in &entries {
        let state = if e.freed { "freed" } else { "live" };
        let _ = writeln!(out, "block {} {} {state} {}", e.offset, e.size, e.name);
    }
    let live = entries.iter().filter(|e| !e.freed).count();
    let _ = writeln!(
        out,
        "stats used {} live {live} freed {} check {}",
        sim.used_bytes(),
        entries.len() - live,
        if sim.check() { "ok" } else { "FAILED" }
    );
    Ok(Output::Text(out))
}

fn parse(input: &str) -> Result<(usize, Vec<Op>), String> {
    let mut arena = DEFAULT_ARENA;
    let mut ops = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if ops.len() >= MAX_OPS {
            return Err(format!("line {line_no}: more than {MAX_OPS} operations"));
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["arena", n] if ops.is_empty() => arena = parse_size(n, line_no)?,
            ["alloc", name, size] => ops.push(Op::Alloc {
                name: parse_name(name, line_no)?,
                size: parse_size(size, line_no)?,
            }),
            ["free", name] => ops.push(Op::Free {
                name: parse_name(name, line_no)?,
            }),
            _ => {
                return Err(format!(
                    "line {line_no}: expected `arena N` (first), `alloc NAME SIZE` or `free NAME`, got `{line}`"
                ));
            }
        }
    }
    if ops.is_empty() {
        return Err(String::from("script has no alloc or free lines"));
    }
    Ok((arena, ops))
}

fn parse_size(text: &str, line_no: usize) -> Result<usize, String> {
    let n: usize = text
        .parse()
        .map_err(|_| format!("line {line_no}: invalid size `{text}`"))?;
    if n == 0 || n > MAX_ARENA {
        return Err(format!("line {line_no}: size must be in 1..={MAX_ARENA}"));
    }
    Ok(n)
}

fn parse_name(text: &str, line_no: usize) -> Result<String, String> {
    if text.len() > MAX_NAME {
        return Err(format!(
            "line {line_no}: block name longer than {MAX_NAME} bytes"
        ));
    }
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn stats_line(out: &Output) -> String {
        match out {
            Output::Text(t) => t.lines().last().unwrap().to_owned(),
            Output::Pixels { .. } => panic!("midas must output text"),
        }
    }

    #[test]
    fn alloc_free_script_reports_expected_used_bytes() {
        // 100 B takes a 128 B slab slot, 200 B a 256 B slot; freeing `a` leaves 256 B used.
        let out = run("arena 1048576\nalloc a 100\nalloc b 200\nfree a").unwrap();
        assert_eq!(stats_line(&out), "stats used 256 live 1 freed 1 check ok");
        // A 5000 B request takes two pages (8192 B) from the buddy allocator.
        let big = run("alloc a 100\nalloc b 200\nalloc c 5000\nfree a").unwrap();
        assert_eq!(stats_line(&big), "stats used 8448 live 2 freed 1 check ok");
    }

    #[test]
    fn bad_scripts_are_refused() {
        assert!(run("").is_err());
        assert!(run("alloc a 64\nfree a\nfree a").is_err());
        assert!(run("alloc a 0").is_err());
        assert!(run("free ghost").is_err());
        assert!(run("alloc a 64\nalloc a 64").is_err());
        assert!(run("alloc a 999999999").is_err());
        assert!(run("bogus line").is_err());
    }
}
