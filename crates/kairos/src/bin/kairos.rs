//! CLI tool for kairos: deterministic multi-CPU fair scheduler simulation.
#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use kairos::{Class, Config, Scheduler, TaskId};

const VERSION_STR: &str = "kairos 0.1.0";

const CFS_WEIGHTS: [u64; 40] = [
    88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100, 4904,
    3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172, 137, 110, 87,
    70, 56, 45, 36, 29, 23, 18, 15,
];

fn usage() -> &'static str {
    "\
Usage:
  kairos <subcommand> [args]
  kairos --help
  kairos --version

Subcommands:
  sim FILE
      Simulate tasks described in FILE ('-' for stdin) and print a per-CPU
      timeline in ticks plus each task's CPU share against its weight.
      FILE lines: 'cpus N' and 'task NAME WEIGHT WORK'.
      Example: kairos sim workload.txt"
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
                    return Err(CliError::Op(format!("line {line_no}: expected 'cpus N'")));
                }
                if cpus.is_some() {
                    return Err(CliError::Op(format!("line {line_no}: duplicate 'cpus'")));
                }
                let n = parts[1].parse::<usize>().map_err(|_| {
                    CliError::Op(format!("line {line_no}: invalid cpu count '{}'", parts[1]))
                })?;
                if !(1..=64).contains(&n) {
                    return Err(CliError::Op(format!(
                        "line {line_no}: cpus must be in 1..=64"
                    )));
                }
                cpus = Some(n);
            }
            Some("task") => {
                if parts.len() != 4 {
                    return Err(CliError::Op(format!(
                        "line {line_no}: expected 'task NAME WEIGHT WORK'"
                    )));
                }
                let name = parts[1];
                if !names.insert(name.to_string()) {
                    return Err(CliError::Op(format!(
                        "line {line_no}: duplicate task '{name}'"
                    )));
                }
                let weight = parts[2].parse::<u64>().map_err(|_| {
                    CliError::Op(format!("line {line_no}: invalid weight '{}'", parts[2]))
                })?;
                let work = parts[3].parse::<u64>().map_err(|_| {
                    CliError::Op(format!("line {line_no}: invalid work '{}'", parts[3]))
                })?;
                if weight == 0 {
                    return Err(CliError::Op(format!("line {line_no}: weight must be >= 1")));
                }
                if work == 0 {
                    return Err(CliError::Op(format!("line {line_no}: work must be >= 1")));
                }
                tasks.push(TaskSpec {
                    name: name.to_string(),
                    weight,
                    work,
                });
            }
            Some(other) => {
                return Err(CliError::Op(format!(
                    "line {line_no}: unknown directive '{other}'"
                )));
            }
            None => {}
        }
    }

    let cpu_count = cpus.ok_or_else(|| CliError::Op("missing 'cpus N' line".to_string()))?;
    if tasks.is_empty() {
        return Err(CliError::Op("no 'task' lines found".to_string()));
    }
    Ok((cpu_count, tasks))
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

fn cmd_sim(args: &[String]) -> Result<(), CliError> {
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
            .map_err(|e| CliError::Op(format!("spawn failed: {e:?}")))?;
        ids.push(id);
        remaining.push(spec.work);
    }

    let mut timelines: Vec<Vec<String>> = vec![Vec::new(); cpus];
    let mut tick: u64 = 0;
    const MAX_TICKS: u64 = 1_000_000;

    while remaining.iter().any(|&r| r > 0) {
        if tick >= MAX_TICKS {
            return Err(CliError::Op("simulation tick limit exceeded".to_string()));
        }
        let now_us = tick.saturating_mul(1000);
        let mut finished_this_tick: Vec<usize> = Vec::new();

        for (cpu, tl) in timelines.iter_mut().enumerate() {
            let dec = sched
                .pick(cpu, now_us)
                .map_err(|e| CliError::Op(format!("scheduler pick failed: {e:?}")))?;
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

    let mut out = io::stdout().lock();
    for (cpu, tl) in timelines.iter().enumerate() {
        writeln!(out, "cpu {cpu}: {}", tl.join(" "))
            .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
    }
    for (idx, spec) in tasks.iter().enumerate() {
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
        writeln!(
            out,
            "task {}: ticks={} share={:.4} weight={} expected={:.4}",
            spec.name, ticks_ran, share, spec.weight, expected
        )
        .map_err(|e| CliError::Op(format!("failed to write stdout: {e}")))?;
    }
    Ok(())
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
                eprintln!("kairos: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) => {
                eprintln!("kairos: {msg}");
                process::exit(1);
            }
        }
    }
}
