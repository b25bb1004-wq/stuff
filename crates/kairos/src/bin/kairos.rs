//! CLI tool for kairos: deterministic multi-CPU fair scheduler simulation.
//!
//! Text mode prints the per-CPU timeline and the task shares. `--describe` prints the manifest and
//! `--json` prints one ACI envelope (docs/ai-computer/ACI.md). Both read the same `Report`.
#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use kairos::{Class, Config, Scheduler, TaskId};

const VERSION_STR: &str = "kairos 0.1.0";

const CFS_WEIGHTS: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];

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
    summary: "Simulate the tasks in FILE ('-' for stdin); --json pages the per-CPU timelines (--limit, --offset)",
    effect: Effect::Read,
    args: SIM_ARGS,
    returns: "cpus, ticks, items: [{cpu, slots: [task name or idle per tick]}], total, more, next_offset, tasks: [{name, ticks, share, weight, expected}]",
    example: "kairos --json sim workload.txt",
}];

const MANIFEST: Manifest = Manifest {
    tool: "kairos",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Deterministic multi-CPU fair scheduler simulation",
    commands: COMMANDS,
};

fn usage() -> &'static str {
    "\
kairos - deterministic multi-CPU fair scheduler simulation
Usage:
  kairos <subcommand> [args]
  kairos --help
  kairos --version

Subcommands:
  sim FILE
      Simulate tasks described in FILE ('-' for stdin) and print a per-CPU
      timeline in ticks plus each task's CPU share against its weight.
      FILE lines: 'cpus N' and 'task NAME WEIGHT WORK'.
      Example file (runs as given):
        cpus 2
        task a 1024 10
        task b 512 10
      Example: kairos sim workload.txt"
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

fn read_input(path: &str) -> Result<String, CliError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Op(Code::Io, format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| {
            let code = if e.kind() == io::ErrorKind::NotFound {
                Code::NotFound
            } else {
                Code::Io
            };
            CliError::Op(code, format!("failed to read '{path}': {e}"))
        })
    }
}

struct TaskSpec {
    name: String,
    weight: u64,
    work: u64,
}

fn parse_sim(text: &str) -> Result<(usize, Vec<TaskSpec>), CliError> {
    let mut cpus: Option<usize> = None;
    let mut tasks: Vec<TaskSpec> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.first().copied() {
            Some("cpus") => {
                if parts.len() != 2 {
                    return Err(invalid(format!("line {line_no}: expected 'cpus N'")));
                }
                if cpus.is_some() {
                    return Err(invalid(format!("line {line_no}: duplicate 'cpus'")));
                }
                let n = parts[1].parse::<usize>().map_err(|_| {
                    invalid(format!("line {line_no}: invalid cpu count '{}'", parts[1]))
                })?;
                if !(1..=64).contains(&n) {
                    return Err(invalid(format!("line {line_no}: cpus must be in 1..=64")));
                }
                cpus = Some(n);
            }
            Some("task") => {
                if parts.len() != 4 {
                    return Err(invalid(format!(
                        "line {line_no}: expected 'task NAME WEIGHT WORK'"
                    )));
                }
                let name = parts[1];
                if !names.insert(name.to_string()) {
                    return Err(invalid(format!("line {line_no}: duplicate task '{name}'")));
                }
                let weight = parts[2].parse::<u64>().map_err(|_| {
                    invalid(format!("line {line_no}: invalid weight '{}'", parts[2]))
                })?;
                let work = parts[3]
                    .parse::<u64>()
                    .map_err(|_| invalid(format!("line {line_no}: invalid work '{}'", parts[3])))?;
                if weight == 0 {
                    return Err(invalid(format!("line {line_no}: weight must be >= 1")));
                }
                if work == 0 {
                    return Err(invalid(format!("line {line_no}: work must be >= 1")));
                }
                tasks.push(TaskSpec {
                    name: name.to_string(),
                    weight,
                    work,
                });
            }
            Some(other) => {
                return Err(invalid(format!(
                    "line {line_no}: unknown directive '{other}'"
                )));
            }
            None => {}
        }
    }

    let cpu_count = cpus.ok_or_else(|| invalid("missing 'cpus N' line".to_string()))?;
    if tasks.is_empty() {
        return Err(invalid("no 'task' lines found".to_string()));
    }
    Ok((cpu_count, tasks))
}

fn invalid(message: String) -> CliError {
    CliError::Op(Code::Invalid, message)
}

fn weight_to_nice(weight: u64, geom_mean: f64) -> i8 {
    let target = 1024.0 * (weight as f64) / geom_mean.max(1e-9);
    let target_ln = target.max(1.0).ln();
    let mut best_idx = 20usize;
    let mut best_diff = f64::INFINITY;
    for (idx, &w) in CFS_WEIGHTS.iter().enumerate() {
        let diff = ((w as f64).ln() - target_ln).abs();
        if diff < best_diff {
            best_diff = diff;
            best_idx = idx;
        }
    }
    (best_idx as i32 - 20) as i8
}

/// One task's result: ticks it ran, its share of all ticks, and its weight's expected share.
struct TaskRow {
    name: String,
    ticks: u64,
    share: f64,
    weight: u64,
    expected: f64,
}

/// What a simulation produced: the timeline of each CPU (one name per tick) and the task rows.
struct Report {
    ticks: u64,
    timelines: Vec<Vec<String>>,
    tasks: Vec<TaskRow>,
}

/// Parse and run the workload named by `FILE` (the arguments after `sim`).
fn simulate(args: &[String]) -> Result<Report, CliError> {
    if args.len() != 1 {
        return Err(CliError::Usage("sim expects FILE".to_string()));
    }
    let text = read_input(&args[0])?;
    let (cpus, tasks) = parse_sim(&text)?;

    let mean_ln = tasks.iter().map(|t| (t.weight as f64).ln()).sum::<f64>() / (tasks.len() as f64);
    let geom_mean = mean_ln.exp();

    let mut sched = Scheduler::new(cpus, Config::default());
    let mut ids: Vec<TaskId> = Vec::with_capacity(tasks.len());
    let mut remaining: Vec<u64> = Vec::with_capacity(tasks.len());
    let mut ran: Vec<u64> = vec![0; tasks.len()];

    for spec in &tasks {
        let nice = weight_to_nice(spec.weight, geom_mean);
        let id = sched
            .spawn(Class::Normal { nice }, 0, 0)
            .map_err(|e| CliError::Op(Code::Internal, format!("spawn failed: {e:?}")))?;
        ids.push(id);
        remaining.push(spec.work);
    }

    let mut timelines: Vec<Vec<String>> = vec![Vec::new(); cpus];
    let mut tick: u64 = 0;
    const MAX_TICKS: u64 = 1_000_000;

    while remaining.iter().any(|&r| r > 0) {
        if tick >= MAX_TICKS {
            return Err(invalid("simulation tick limit exceeded".to_string()));
        }
        let now_us = tick.saturating_mul(1000);
        let mut finished_this_tick: Vec<usize> = Vec::new();

        for (cpu, tl) in timelines.iter_mut().enumerate() {
            let dec = sched.pick(cpu, now_us).map_err(|e| {
                CliError::Op(Code::Internal, format!("scheduler pick failed: {e:?}"))
            })?;
            if let Some(tid) = dec.task {
                let t_idx = tid as usize;
                let name = tasks
                    .get(t_idx)
                    .map_or("?", |t| t.name.as_str())
                    .to_string();
                tl.push(name);
                if let Some(cnt) = ran.get_mut(t_idx) {
                    *cnt = cnt.saturating_add(1);
                }
                if let Some(rem) = remaining.get_mut(t_idx) {
                    *rem = rem.saturating_sub(1);
                    if *rem == 0 {
                        finished_this_tick.push(t_idx);
                    }
                }
            } else {
                tl.push("idle".to_string());
            }
        }

        let end_us = tick.saturating_add(1).saturating_mul(1000);
        for t_idx in finished_this_tick {
            if let Some(&tid) = ids.get(t_idx) {
                let _ = sched.exit(tid, end_us);
            }
        }
        tick += 1;
    }

    let total_ran: u64 = ran.iter().sum();
    let total_weight: u64 = tasks.iter().map(|t| t.weight).sum();

    let rows = tasks
        .iter()
        .enumerate()
        .map(|(idx, spec)| {
            let ticks_ran = ran.get(idx).copied().unwrap_or(0);
            let share = if total_ran > 0 {
                ticks_ran as f64 / total_ran as f64
            } else {
                0.0
            };
            let expected = if total_weight > 0 {
                spec.weight as f64 / total_weight as f64
            } else {
                0.0
            };
            TaskRow {
                name: spec.name.clone(),
                ticks: ticks_ran,
                share,
                weight: spec.weight,
                expected,
            }
        })
        .collect();

    Ok(Report {
        ticks: tick,
        timelines,
        tasks: rows,
    })
}

fn cmd_sim(args: &[String]) -> Result<(), CliError> {
    let report = simulate(args)?;
    let mut out = io::stdout().lock();
    for (cpu, tl) in report.timelines.iter().enumerate() {
        writeln!(out, "cpu {cpu}: {}", tl.join(" "))
            .map_err(|e| CliError::Op(Code::Io, format!("failed to write stdout: {e}")))?;
    }
    for t in &report.tasks {
        writeln!(
            out,
            "task {}: ticks={} share={:.4} weight={} expected={:.4}",
            t.name, t.ticks, t.share, t.weight, t.expected
        )
        .map_err(|e| CliError::Op(Code::Io, format!("failed to write stdout: {e}")))?;
    }
    Ok(())
}

fn int<T: TryInto<i64>>(n: T) -> Json {
    Json::Int(n.try_into().unwrap_or(i64::MAX))
}

fn kv(key: &str, value: Json) -> (String, Json) {
    (key.to_string(), value)
}

/// `kairos --json sim FILE [--limit N] [--offset N]`: the page of CPU timelines, plus the task rows.
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
    let report = simulate(&rest)?;
    let total = report.timelines.len();
    let items = report
        .timelines
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(cpu, tl)| {
            let slots = tl.iter().map(|name| Json::Str(name.clone())).collect();
            Json::obj([("cpu", int(cpu)), ("slots", Json::Arr(slots))])
        })
        .collect();
    let mut page = envelope::page(items, total, offset, limit);
    let tasks = report
        .tasks
        .iter()
        .map(|t| {
            Json::obj([
                ("name", Json::Str(t.name.clone())),
                ("ticks", int(t.ticks)),
                ("share", Json::Num(t.share)),
                ("weight", int(t.weight)),
                ("expected", Json::Num(t.expected)),
            ])
        })
        .collect();
    if let Json::Obj(pairs) = &mut page {
        pairs.push(kv("cpus", int(total)));
        pairs.push(kv("ticks", int(report.ticks)));
        pairs.push(kv("tasks", Json::Arr(tasks)));
    }
    Ok(page)
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

/// `kairos --json ...`: the command name and the data or the failure.
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
            Ok(data) => print!("{}", envelope::ok("kairos", cmd, data)),
            Err(err) => {
                eprintln!("kairos: {}", err.message());
                let hint = matches!(err, CliError::Usage(_)).then_some("run: kairos --describe");
                print!(
                    "{}",
                    envelope::err("kairos", cmd, err.code(), err.message(), hint)
                );
                process::exit(err.exit());
            }
        }
        return;
    }
    if let Err(err) = run(&args) {
        eprintln!("kairos: {}", err.message());
        process::exit(err.exit());
    }
}
