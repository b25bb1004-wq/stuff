//! Kairos multi-CPU fair scheduler timeline demo.

use std::fmt::Write as _;

use kairos::{Class, Config, Scheduler, TaskId};

use super::{check_input, Demo, Output};

const MAX_LINES: usize = 64;
const MAX_CPUS: usize = 8;
const MAX_TICKS: usize = 400;
const TICK_US: u64 = 1_000;

const NICE_WEIGHTS: [(i8, u64); 40] = [
    (-20, 88761),
    (-19, 71755),
    (-18, 56483),
    (-17, 46273),
    (-16, 36291),
    (-15, 29154),
    (-14, 23254),
    (-13, 18705),
    (-12, 14949),
    (-11, 11916),
    (-10, 9548),
    (-9, 7620),
    (-8, 6100),
    (-7, 4904),
    (-6, 3906),
    (-5, 3121),
    (-4, 2501),
    (-3, 1991),
    (-2, 1586),
    (-1, 1277),
    (0, 1024),
    (1, 820),
    (2, 655),
    (3, 526),
    (4, 423),
    (5, 335),
    (6, 272),
    (7, 215),
    (8, 172),
    (9, 137),
    (10, 110),
    (11, 87),
    (12, 70),
    (13, 56),
    (14, 45),
    (15, 36),
    (16, 29),
    (17, 23),
    (18, 18),
    (19, 15),
];

const DEFAULT_SPEC: &str = "\
cpus 2
task alpha 4 12
task beta 2 12
task gamma 1 12";

/// Registered Kairos fair scheduler demo.
pub const DEMO: Demo = Demo {
    id: "kairos",
    title: "Kairos Scheduler",
    default_input: DEFAULT_SPEC,
    run,
};

struct TaskSpec {
    name: String,
    weight: u64,
    work: usize,
    nice: i8,
    cfs_weight: u64,
    id: TaskId,
    remaining: usize,
    ticks_run: usize,
}

/// Simulate weighted fair scheduling across `cpus` and output per-CPU tick timelines and shares.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let (cpus, mut specs) = parse_spec(input)?;
    assign_nice_values(&mut specs);

    let cfg = Config {
        latency_us: 6_000,
        min_slice_us: TICK_US,
    };
    let mut sched = Scheduler::new(cpus, cfg);
    for spec in &mut specs {
        let id = sched
            .spawn(Class::Normal { nice: spec.nice }, 0, 0)
            .map_err(|e| format!("spawn `{}` failed: {e:?}", spec.name))?;
        spec.id = id;
    }

    let mut timelines: Vec<Vec<Option<usize>>> = vec![Vec::new(); cpus];
    let mut total_ticks = 0usize;

    for tick in 0..MAX_TICKS {
        if specs.iter().all(|s| s.remaining == 0) {
            break;
        }
        let now_us = (tick as u64) * TICK_US;
        let mut ran_on_cpu: Vec<Option<usize>> = Vec::with_capacity(cpus);

        for (cpu, tl) in timelines.iter_mut().enumerate() {
            let decision = sched
                .pick(cpu, now_us)
                .map_err(|e| format!("pick cpu {cpu} at tick {tick}: {e:?}"))?;
            let task_idx = decision
                .task
                .and_then(|tid| specs.iter().position(|s| s.id == tid));
            tl.push(task_idx);
            ran_on_cpu.push(task_idx);
        }

        let next_us = ((tick + 1) as u64) * TICK_US;
        for task_idx in ran_on_cpu.into_iter().flatten() {
            if let Some(spec) = specs.get_mut(task_idx) {
                spec.ticks_run = spec.ticks_run.saturating_add(1);
                if spec.remaining > 0 {
                    spec.remaining -= 1;
                    if spec.remaining == 0 {
                        sched
                            .exit(spec.id, next_us)
                            .map_err(|e| format!("exit `{}` failed: {e:?}", spec.name))?;
                    }
                }
            }
        }
        total_ticks = tick + 1;
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "cpus: {cpus}, tasks: {}, simulated ticks: {total_ticks} ({} us/tick)",
        specs.len(),
        TICK_US
    );
    out.push_str("\n=== per-CPU timeline ===\n");
    for (cpu, tl) in timelines.iter().enumerate() {
        let _ = write!(out, "cpu {cpu}:");
        for slot in tl {
            match slot {
                Some(idx) => {
                    let tag = specs
                        .get(*idx)
                        .and_then(|s| s.name.chars().next())
                        .unwrap_or('?');
                    let _ = write!(out, " {tag}");
                }
                None => out.push_str(" -"),
            }
        }
        out.push('\n');
    }

    out.push_str("\n=== task shares vs weight ===\n");
    let sum_ticks: usize = specs.iter().map(|s| s.ticks_run).sum();
    let sum_weights: u64 = specs.iter().map(|s| s.weight).sum();
    for spec in &specs {
        let actual_pct = if sum_ticks > 0 {
            (spec.ticks_run as f64) * 100.0 / (sum_ticks as f64)
        } else {
            0.0
        };
        let weight_pct = if sum_weights > 0 {
            (spec.weight as f64) * 100.0 / (sum_weights as f64)
        } else {
            0.0
        };
        let runtime_us = sched.stats(spec.id).map_or(0, |st| st.runtime_us);
        let _ = writeln!(
            out,
            "task {} (tag '{}'): weight={} (nice={}, cfs_w={}) work={} ticks={} share={actual_pct:.1}% weight_share={weight_pct:.1}% runtime={}us",
            spec.name,
            spec.name.chars().next().unwrap_or('?'),
            spec.weight,
            spec.nice,
            spec.cfs_weight,
            spec.work,
            spec.ticks_run,
            runtime_us
        );
    }

    Ok(Output::Text(out))
}

fn parse_spec(input: &str) -> Result<(usize, Vec<TaskSpec>), String> {
    let mut cpus: Option<usize> = None;
    let mut specs = Vec::new();
    let mut lines_seen = 0usize;

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

        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["cpus", n_str] => {
                let n = n_str
                    .parse::<usize>()
                    .map_err(|_| format!("line {line_no}: invalid cpu count `{n_str}`"))?;
                if !(1..=MAX_CPUS).contains(&n) {
                    return Err(format!(
                        "line {line_no}: cpus must be in 1..={MAX_CPUS}, got {n}"
                    ));
                }
                cpus = Some(n);
            }
            ["task", name, weight_str, work_str] => {
                if name.is_empty() {
                    return Err(format!("line {line_no}: empty task name"));
                }
                let weight = weight_str
                    .parse::<u64>()
                    .map_err(|_| format!("line {line_no}: invalid weight `{weight_str}`"))?;
                if !(1..=100_000).contains(&weight) {
                    return Err(format!("line {line_no}: weight must be in 1..=100000"));
                }
                let work = work_str
                    .parse::<usize>()
                    .map_err(|_| format!("line {line_no}: invalid work `{work_str}`"))?;
                if !(1..=MAX_TICKS).contains(&work) {
                    return Err(format!("line {line_no}: work must be in 1..={MAX_TICKS}"));
                }
                specs.push(TaskSpec {
                    name: (*name).to_owned(),
                    weight,
                    work,
                    nice: 0,
                    cfs_weight: 1024,
                    id: 0,
                    remaining: work,
                    ticks_run: 0,
                });
            }
            _ => {
                return Err(format!(
                    "line {line_no}: expected `cpus N` or `task NAME WEIGHT WORK`, got `{line}`"
                ));
            }
        }
    }

    if specs.is_empty() {
        return Err(String::from(
            "expected at least one `task NAME WEIGHT WORK`",
        ));
    }
    Ok((cpus.unwrap_or(2), specs))
}

fn assign_nice_values(specs: &mut [TaskSpec]) {
    if specs.is_empty() {
        return;
    }
    let avg_log: f64 =
        specs.iter().map(|s| (s.weight as f64).ln()).sum::<f64>() / (specs.len() as f64);
    let ref_w = avg_log.exp().max(1.0);

    for spec in specs {
        let target = ((spec.weight as f64) * 1024.0 / ref_w).clamp(15.0, 88_761.0);
        let mut best_nice = 0i8;
        let mut best_cw = 1024u64;
        let mut best_err = f64::INFINITY;
        for &(nice, cw) in &NICE_WEIGHTS {
            let err = ((cw as f64).ln() - target.ln()).abs();
            if err < best_err {
                best_err = err;
                best_nice = nice;
                best_cw = cw;
            }
        }
        spec.nice = best_nice;
        spec.cfs_weight = best_cw;
    }
}
