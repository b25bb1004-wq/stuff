//! CLI tool for midas: buddy/slab heap allocator simulation.
#![deny(unsafe_op_in_unsafe_fn)]

use std::alloc::Layout;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use midas::Heap;

const VERSION_STR: &str = "midas 0.1.0";
const MAX_SIM_HEAP_BYTES: usize = 128 * 1024 * 1024;

fn usage() -> &'static str {
    "\
Usage:
  midas <subcommand> [args]
  midas --help
  midas --version

Subcommands:
  sim FILE
      Run allocator trace FILE ('-' for stdin) with 'heap BYTES', 'alloc SIZE',
      and 'free N' lines, printing memory usage and invariant check per step.
      Example: midas sim trace.txt"
}

enum CliError {
    Usage(String),
    Op(String),
}

fn read_input(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Op(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| CliError::Op(format!("failed to read '{path}': {e}")))
    }
}

enum Step {
    Heap(usize),
    Alloc(usize),
    Free(usize),
}

fn parse_trace(text: &str) -> Result<Vec<Step>, CliError> {
    let mut steps = Vec::new();
    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(CliError::Op(format!(
                "line {line_no}: expected '<op> <arg>'"
            )));
        }
        let val = parts[1]
            .parse::<usize>()
            .map_err(|_| CliError::Op(format!("line {line_no}: invalid integer '{}'", parts[1])))?;
        match parts[0] {
            "heap" => steps.push(Step::Heap(val)),
            "alloc" => steps.push(Step::Alloc(val)),
            "free" => steps.push(Step::Free(val)),
            other => {
                return Err(CliError::Op(format!(
                    "line {line_no}: unknown command '{other}'"
                )));
            }
        }
    }
    Ok(steps)
}

fn layout_for_size(size: usize) -> Result<Layout, CliError> {
    let align = if size == 0 {
        1
    } else {
        size.checked_next_power_of_two().unwrap_or(8).min(8)
    };
    Layout::from_size_align(size, align)
        .map_err(|_| CliError::Op(format!("invalid allocation size {size}")))
}

fn cmd_sim(args: &[String]) -> Result<(), CliError> {
    if args.len() != 1 {
        return Err(CliError::Usage("sim expects FILE".to_string()));
    }
    let text = read_input(&args[0])?;
    let steps = parse_trace(&text)?;
    if steps.is_empty() {
        return Err(CliError::Op("empty trace file".to_string()));
    }

    let mut backing: Vec<u8> = Vec::new();
    let mut heap: Option<Heap> = None;
    let mut allocs: Vec<Option<(*mut u8, Layout)>> = Vec::new();
    let mut out = io::stdout().lock();

    let res: Result<(), CliError> = (|| {
        for (step_idx, step) in steps.iter().enumerate() {
            let step_no = step_idx + 1;
            let op_desc = match *step {
                Step::Heap(bytes) => {
                    if heap.is_some() {
                        return Err(CliError::Op(format!(
                            "step {step_no}: duplicate 'heap' directive"
                        )));
                    }
                    if bytes > MAX_SIM_HEAP_BYTES {
                        return Err(CliError::Op(format!(
                            "step {step_no}: heap size {bytes} exceeds limit {MAX_SIM_HEAP_BYTES}"
                        )));
                    }
                    backing = vec![0u8; bytes];
                    // SAFETY: `backing` stays alive and unmodified by caller code until `heap` is dropped below.
                    let h = unsafe { Heap::new(backing.as_mut_ptr(), backing.len()) }.ok_or_else(
                        || CliError::Op(format!("step {step_no}: heap size {bytes} is too small")),
                    )?;
                    heap = Some(h);
                    format!("heap {bytes}")
                }
                Step::Alloc(size) => {
                    let Some(h) = heap.as_mut() else {
                        return Err(CliError::Op(format!(
                            "step {step_no}: 'alloc' before 'heap'"
                        )));
                    };
                    let layout = layout_for_size(size)?;
                    let ptr = h.alloc(layout);
                    if size > 0 && ptr.is_null() {
                        return Err(CliError::Op(format!(
                            "step {step_no}: out of memory for alloc {size}"
                        )));
                    }
                    let id = allocs.len();
                    allocs.push(Some((ptr, layout)));
                    format!("alloc #{id} ({size}B)")
                }
                Step::Free(id) => {
                    let Some(h) = heap.as_mut() else {
                        return Err(CliError::Op(format!(
                            "step {step_no}: 'free' before 'heap'"
                        )));
                    };
                    let Some(slot) = allocs.get_mut(id) else {
                        return Err(CliError::Op(format!(
                            "step {step_no}: bad allocation index {id}"
                        )));
                    };
                    let Some((ptr, layout)) = slot.take() else {
                        return Err(CliError::Op(format!(
                            "step {step_no}: double free of allocation {id}"
                        )));
                    };
                    // SAFETY: `ptr` came from `h.alloc(layout)` on this heap and `slot.take()` ensures it is freed once.
                    unsafe { h.dealloc(ptr, layout) };
                    format!("free #{id}")
                }
            };

            if let Some(h) = heap.as_ref() {
                let used = h.used_bytes();
                let total = h.total_bytes();
                let free = total.saturating_sub(used);
                let ok = h.check();
                writeln!(
                    out,
                    "step {step_no}: {op_desc}: used={used} total={total} free={free} check={ok}"
                )
                .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
            }
        }
        Ok(())
    })();

    // `Heap` has no Drop impl, so freeing `backing` after the last use of `heap` is enough.
    drop(backing);
    res
}

fn run() -> Result<(), CliError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(first) = args.first() else {
        return Err(CliError::Usage(
            "missing subcommand (try --help)".to_string(),
        ));
    };
    match first.as_str() {
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        "--version" | "-V" => {
            println!("{VERSION_STR}");
            Ok(())
        }
        sub => {
            let rest = &args[1..];
            if rest.iter().any(|a| a == "--help" || a == "-h") {
                println!("{}", usage());
                return Ok(());
            }
            match sub {
                "sim" => cmd_sim(rest),
                other => Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
            }
        }
    }
}

fn main() {
    if let Err(err) = run() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("midas: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) => {
                eprintln!("midas: {msg}");
                process::exit(1);
            }
        }
    }
}
