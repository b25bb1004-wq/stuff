//! CLI tool for midas: buddy/slab heap allocator simulation.
//!
//! Text mode prints one line per step. `--describe` prints the manifest, and `--json` prints one ACI
//! envelope with the same steps as records (docs/ai-computer/ACI.md). Both read the records that `simulate` makes.
#![deny(unsafe_op_in_unsafe_fn)]

use std::alloc::Layout;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use midas::Heap;

const VERSION_STR: &str = "midas 0.1.0";
const MAX_SIM_HEAP_BYTES: usize = 128 * 1024 * 1024;

const SIM_ARGS: &[Arg] = &[
    Arg {
        name: "FILE",
        kind: ArgKind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--limit",
        kind: ArgKind::Int,
        required: false,
        default: Some("100"),
        choices: &[],
    },
    Arg {
        name: "--offset",
        kind: ArgKind::Int,
        required: false,
        default: Some("0"),
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[Command {
    name: "sim",
    summary: "Run allocator trace FILE ('-' for stdin); --json pages the per-step records (--limit, --offset)",
    effect: Effect::Read,
    args: SIM_ARGS,
    returns: "items: [{step, op, id, size, used, total, free, check}], total, more, next_offset",
    example: "midas --json sim trace.txt",
}];

const MANIFEST: Manifest = Manifest {
    tool: "midas",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Buddy/slab heap allocator simulation",
    commands: COMMANDS,
};

fn usage() -> &'static str {
    "\
midas - buddy/slab heap allocator simulation
Usage:
  midas <subcommand> [args]
  midas --help
  midas --version

Subcommands:
  sim FILE
      Run allocator trace FILE ('-' for stdin) with 'heap BYTES', 'alloc SIZE',
      and 'free N' lines, printing memory usage and invariant check per step.
      The heap line comes first. Example file (runs as given):
        heap 65536
        alloc 100
        free 0
      Example: midas sim trace.txt"
}

/// A failed run: the ACI code, the message, and whether it is a usage error (exit 2) or an operation error (exit 1).
enum CliError {
    Usage(String),
    Op(Code, String),
}

impl CliError {
    fn code(&self) -> Code {
        match self {
            CliError::Usage(_) => Code::Usage,
            CliError::Op(code, _) => *code,
        }
    }

    fn message(&self) -> &str {
        match self {
            CliError::Usage(msg) | CliError::Op(_, msg) => msg,
        }
    }

    fn exit(&self) -> i32 {
        match self {
            CliError::Usage(_) => 2,
            CliError::Op(..) => 1,
        }
    }
}

fn io_code(err: &io::Error) -> Code {
    if err.kind() == io::ErrorKind::NotFound {
        Code::NotFound
    } else {
        Code::Io
    }
}

fn read_input(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path)
            .map_err(|e| CliError::Op(io_code(&e), format!("failed to read '{path}': {e}")))
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
            return Err(CliError::Op(
                Code::Invalid,
                format!("line {line_no}: expected '<op> <arg>'"),
            ));
        }
        let val = parts[1].parse::<usize>().map_err(|_| {
            CliError::Op(
                Code::Invalid,
                format!("line {line_no}: invalid integer '{}'", parts[1]),
            )
        })?;
        match parts[0] {
            "heap" => steps.push(Step::Heap(val)),
            "alloc" => steps.push(Step::Alloc(val)),
            "free" => steps.push(Step::Free(val)),
            other => {
                return Err(CliError::Op(
                    Code::Invalid,
                    format!("line {line_no}: unknown command '{other}'"),
                ));
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
        .map_err(|_| CliError::Op(Code::Invalid, format!("invalid allocation size {size}")))
}

/// One simulated step, with the heap numbers after it.
struct Record {
    step: usize,
    op: &'static str,
    id: Option<usize>,
    size: Option<usize>,
    used: usize,
    total: usize,
    free: usize,
    check: bool,
}

impl Record {
    /// The operation as the text output words it.
    fn describe(&self) -> String {
        match (self.op, self.size, self.id) {
            ("heap", Some(bytes), _) => format!("heap {bytes}"),
            ("alloc", Some(size), Some(id)) => format!("alloc #{id} ({size}B)"),
            (_, _, Some(id)) => format!("free #{id}"),
            _ => String::new(),
        }
    }

    fn json(&self) -> Json {
        let mut pairs = vec![
            kv("step", int(self.step)),
            kv("op", Json::Str(self.op.into())),
        ];
        if let Some(id) = self.id {
            pairs.push(kv("id", int(id)));
        }
        if let Some(size) = self.size {
            pairs.push(kv("size", int(size)));
        }
        pairs.push(kv("used", int(self.used)));
        pairs.push(kv("total", int(self.total)));
        pairs.push(kv("free", int(self.free)));
        pairs.push(kv("check", Json::Bool(self.check)));
        Json::Obj(pairs)
    }
}

fn kv(key: &str, value: Json) -> (String, Json) {
    (key.to_string(), value)
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// Run the trace, passing one record to `sink` per step. Stops at the first failing step.
fn simulate(
    steps: &[Step],
    sink: &mut dyn FnMut(Record) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let mut backing: Vec<u8> = Vec::new();
    let mut heap: Option<Heap> = None;
    let mut allocs: Vec<Option<(*mut u8, Layout)>> = Vec::new();

    let res: Result<(), CliError> = (|| {
        for (step_idx, step) in steps.iter().enumerate() {
            let step_no = step_idx + 1;
            let (op, id, size) = match *step {
                Step::Heap(bytes) => {
                    if heap.is_some() {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!("step {step_no}: duplicate 'heap' directive"),
                        ));
                    }
                    if bytes > MAX_SIM_HEAP_BYTES {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!(
                                "step {step_no}: heap size {bytes} exceeds limit {MAX_SIM_HEAP_BYTES}"
                            ),
                        ));
                    }
                    backing = vec![0u8; bytes];
                    // SAFETY: `backing` stays alive and unmodified by caller code until `heap` is dropped below.
                    let h = unsafe { Heap::new(backing.as_mut_ptr(), backing.len()) }.ok_or_else(
                        || {
                            CliError::Op(
                                Code::Invalid,
                                format!("step {step_no}: heap size {bytes} is too small"),
                            )
                        },
                    )?;
                    heap = Some(h);
                    ("heap", None, Some(bytes))
                }
                Step::Alloc(size) => {
                    let Some(h) = heap.as_mut() else {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!("step {step_no}: 'alloc' before 'heap'"),
                        ));
                    };
                    let layout = layout_for_size(size)?;
                    let ptr = h.alloc(layout);
                    if size > 0 && ptr.is_null() {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!("step {step_no}: out of memory for alloc {size}"),
                        ));
                    }
                    let id = allocs.len();
                    allocs.push(Some((ptr, layout)));
                    ("alloc", Some(id), Some(size))
                }
                Step::Free(id) => {
                    let Some(h) = heap.as_mut() else {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!("step {step_no}: 'free' before 'heap'"),
                        ));
                    };
                    let Some(slot) = allocs.get_mut(id) else {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!("step {step_no}: bad allocation index {id}"),
                        ));
                    };
                    let Some((ptr, layout)) = slot.take() else {
                        return Err(CliError::Op(
                            Code::Invalid,
                            format!("step {step_no}: double free of allocation {id}"),
                        ));
                    };
                    // SAFETY: `ptr` came from `h.alloc(layout)` on this heap and `slot.take()` ensures it is freed once.
                    unsafe { h.dealloc(ptr, layout) };
                    ("free", Some(id), None)
                }
            };

            if let Some(h) = heap.as_ref() {
                let used = h.used_bytes();
                let total = h.total_bytes();
                let free = total.saturating_sub(used);
                let check = h.check();
                sink(Record {
                    step: step_no,
                    op,
                    id,
                    size,
                    used,
                    total,
                    free,
                    check,
                })?;
            }
        }
        Ok(())
    })();

    // `Heap` has no Drop impl, so freeing `backing` after the last use of `heap` is enough.
    drop(backing);
    res
}

/// The steps of the trace named by `FILE` (the arguments after `sim`).
fn load_trace(args: &[String]) -> Result<Vec<Step>, CliError> {
    if args.len() != 1 {
        return Err(CliError::Usage("sim expects FILE".to_string()));
    }
    let text = read_input(&args[0])?;
    let steps = parse_trace(&text)?;
    if steps.is_empty() {
        return Err(CliError::Op(Code::Invalid, "empty trace file".to_string()));
    }
    Ok(steps)
}

fn cmd_sim(args: &[String]) -> Result<(), CliError> {
    let steps = load_trace(args)?;
    let mut out = io::stdout().lock();
    simulate(&steps, &mut |r: Record| {
        writeln!(
            out,
            "step {}: {}: used={} total={} free={} check={}",
            r.step,
            r.describe(),
            r.used,
            r.total,
            r.free,
            r.check
        )
        .map_err(|e| CliError::Op(Code::Io, format!("failed to write stdout: {e}")))
    })
}

/// `midas --json sim FILE [--limit N] [--offset N]`: the page of step records.
fn sim_json(args: &[String]) -> Result<Json, CliError> {
    let mut rest = Vec::with_capacity(args.len());
    let mut limit = DEFAULT_LIMIT;
    let mut offset = 0;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if flag != "--limit" && flag != "--offset" {
            rest.push(args[i].clone());
            i += 1;
            continue;
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| CliError::Usage(format!("{flag} requires a value")))?;
        let number = value
            .parse::<usize>()
            .map_err(|_| CliError::Usage(format!("invalid value '{value}' for {flag}")))?;
        if flag == "--offset" {
            offset = number;
        } else if (1..=MAX_LIMIT).contains(&number) {
            limit = number;
        } else {
            return Err(CliError::Usage(format!(
                "--limit must be between 1 and {MAX_LIMIT}"
            )));
        }
        i += 2;
    }
    let steps = load_trace(&rest)?;
    let mut records = Vec::new();
    simulate(&steps, &mut |r: Record| {
        records.push(r);
        Ok(())
    })?;
    let total = records.len();
    let items = records
        .iter()
        .skip(offset)
        .take(limit)
        .map(Record::json)
        .collect();
    Ok(envelope::page(items, total, offset, limit))
}

fn run(args: &[String]) -> Result<(), CliError> {
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

/// `midas --json ...`: one envelope on stdout. Returns the command name and the data or the failure.
fn run_json(args: &[String]) -> (&'static str, Result<Json, CliError>) {
    match args.first().map(String::as_str) {
        Some("sim") => ("sim", sim_json(&args[1..])),
        Some("--help" | "-h" | "help") => {
            ("", Ok(Json::obj([("usage", Json::Str(usage().into()))])))
        }
        Some("--version" | "-V") => (
            "",
            Ok(Json::obj([("version", Json::Str(VERSION_STR.into()))])),
        ),
        None => (
            "",
            Err(CliError::Usage(
                "missing subcommand (try --help)".to_string(),
            )),
        ),
        Some(other) => (
            "",
            Err(CliError::Usage(format!("unknown subcommand '{other}'"))),
        ),
    }
}

fn main() {
    let mut raw: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return;
    }
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if json {
        let (cmd, result) = run_json(&args);
        match result {
            Ok(data) => print!("{}", envelope::ok("midas", cmd, data)),
            Err(err) => {
                eprintln!("midas: {}", err.message());
                let hint = matches!(err, CliError::Usage(_)).then_some("run: midas --describe");
                print!(
                    "{}",
                    envelope::err("midas", cmd, err.code(), err.message(), hint)
                );
                process::exit(err.exit());
            }
        }
        return;
    }
    if let Err(err) = run(&args) {
        eprintln!("midas: {}", err.message());
        process::exit(err.exit());
    }
}
