//! Exams: run suites with the model or with the reference solutions, and report.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use aci::envelope::Code;
use aci::json::Json;

use crate::actions::{self, Ctx};
use crate::agent;
use crate::computer::Computer;
use crate::memory::{Kind, Store};
use crate::suite::{self, CheckResult, Task};
use crate::types::{Action, CmdOut, Error, Level, Outcome, Result, RunConfig, Stop};

/// What one exam run should do.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Exam files, run in order.
    pub suites: Vec<PathBuf>,
    /// Run folder: computers, results, report, default mind.
    pub out: PathBuf,
    /// Run the reference solutions instead of the model.
    pub selftest: bool,
    /// When not empty, only these task ids.
    pub only: Vec<String>,
    /// At most this many tasks (after `only`).
    pub limit: Option<usize>,
    /// Level, model, freeze, mind folder and the other run settings.
    pub cfg: RunConfig,
    /// Tool folder for the computers (tests point them at another one).
    pub tools: Option<PathBuf>,
}

/// Totals of a run (all tasks in the results file, including ones finished by an earlier run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Tasks in the results file.
    pub tasks: usize,
    /// Tasks that passed on the first try.
    pub passed: usize,
    /// Tasks that passed on the first try or on the verified retry.
    pub passed_after_retry: usize,
    /// Tasks whose setup failed (not run).
    pub setup_errors: usize,
    /// The markdown report, also written to `report.md`.
    pub report: String,
}

/// Runs the exam described by `opts` and writes `results.jsonl` and `report.md` in `opts.out`.
pub fn run(opts: &Options) -> Result<Summary> {
    fs::create_dir_all(&opts.out).map_err(|e| io_error("cannot create the run folder", &e))?;
    let mind_dir = opts
        .cfg
        .mind_dir
        .clone()
        .unwrap_or_else(|| opts.out.join("mind"));
    let mind = Store::at(&mind_dir);
    let mut selected = load_tasks(&opts.suites)?;
    if !opts.only.is_empty() {
        selected.retain(|(_, t)| opts.only.contains(&t.id));
    }
    if let Some(n) = opts.limit {
        selected.truncate(n);
    }
    let path = opts.out.join("results.jsonl");
    let mut records = read_records(&path);
    let done: HashSet<String> = records
        .iter()
        .filter_map(|r| sval(r, "id").map(str::to_string))
        .collect();
    let mut file = open_results(&path)?;
    let total = selected.len();
    for (k, (suite, task)) in selected.iter().enumerate() {
        if done.contains(&task.id) {
            continue;
        }
        let rec = run_task_record(opts, &mind, suite, task)?;
        file.write_all(format!("{}\n", rec.encode()).as_bytes())
            .map_err(|e| io_error("cannot write the results", &e))?;
        eprintln!("[{}/{}] {}", k + 1, total, progress(&rec));
        records.push(rec.json);
    }
    let report = make_report(opts, &records);
    fs::write(opts.out.join("report.md"), &report)
        .map_err(|e| io_error("cannot write the report", &e))?;
    Ok(summary(&records, report))
}

/// CLI: `eval --suite FILE [--suite FILE]... --out DIR [--selftest] [--only ID,ID] [--limit N]`
/// plus the run flags of `run`.
pub fn cmd(args: &[String]) -> Result<CmdOut> {
    let mut cfg = RunConfig::default();
    let rest = agent::parse_run_flags(args, &mut cfg)?;
    let mut suites = Vec::new();
    let mut out = None;
    let mut selftest = false;
    let mut only = Vec::new();
    let mut limit = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--suite" => suites.push(PathBuf::from(flag_value(&rest, &mut i)?)),
            "--out" => out = Some(PathBuf::from(flag_value(&rest, &mut i)?)),
            "--selftest" => selftest = true,
            "--only" => {
                only = flag_value(&rest, &mut i)?
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
            }
            "--limit" => {
                let v = flag_value(&rest, &mut i)?;
                limit = Some(
                    v.parse::<usize>()
                        .map_err(|_| usage(format!("--limit needs a whole number, not `{v}`")))?,
                );
            }
            other => {
                return Err(usage(format!("unknown argument `{other}`")).hint(USAGE));
            }
        }
        i += 1;
    }
    if suites.is_empty() {
        return Err(usage("no --suite given").hint(USAGE));
    }
    let out = out.ok_or_else(|| usage("no --out given").hint(USAGE))?;
    let opts = Options {
        suites,
        out,
        selftest,
        only,
        limit,
        cfg,
        tools: None,
    };
    let s = run(&opts)?;
    Ok(CmdOut {
        data: Json::obj([
            ("tasks", count(s.tasks)),
            ("passed", count(s.passed)),
            ("passed_after_retry", count(s.passed_after_retry)),
            ("setup_errors", count(s.setup_errors)),
            ("out", Json::path(&opts.out)),
        ]),
        text: s.report,
        exit: 0,
    })
}

const USAGE: &str = "eval --suite FILE [--suite FILE]... --out DIR [--selftest] [--only ID,ID] \
     [--limit N] [run flags]";

/// A usage error.
fn usage(msg: impl Into<String>) -> Error {
    Error::new(Code::Usage, msg)
}

/// The value after the flag at `*i`, moving `*i` onto it.
fn flag_value<'a>(args: &'a [String], i: &mut usize) -> Result<&'a str> {
    let idx = *i;
    match args.get(idx + 1) {
        Some(v) => {
            *i = idx + 1;
            Ok(v.as_str())
        }
        None => Err(usage(format!("{} needs a value", args[idx]))),
    }
}

/// An I/O error with what was being done.
fn io_error(context: &str, e: &std::io::Error) -> Error {
    Error::new(Code::Io, format!("{context}: {e}"))
}

/// A count as a JSON integer.
fn count(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// The first `max_bytes` bytes of `s`, cut on a character boundary.
fn cut(s: &str, max_bytes: usize) -> String {
    let mut end = max_bytes.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// The first `n` characters of `s`.
fn cut_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Every task of every suite, with its suite name (the file stem). Duplicate ids are an error.
fn load_tasks(suites: &[PathBuf]) -> Result<Vec<(String, Task)>> {
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    for path in suites {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("suite")
            .to_string();
        for task in suite::load(path)? {
            if !ids.insert(task.id.clone()) {
                return Err(Error::new(
                    Code::Invalid,
                    format!("task id `{}` is in more than one place", task.id),
                )
                .hint("give every task a unique id across all suite files"));
            }
            out.push((name.clone(), task));
        }
    }
    Ok(out)
}

/// The records of an earlier run. Lines that do not parse are ignored.
fn read_records(path: &Path) -> Vec<Json> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| Json::parse(l).ok())
        .filter(|j| matches!(j, Json::Obj(_)))
        .collect()
}

/// Opens the results file for appending. A crashed run may leave a partial last line; a newline
/// first keeps the next record on its own line.
fn open_results(path: &Path) -> Result<fs::File> {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| io_error("cannot open the results file", &e))?;
    let torn = fs::read(path).is_ok_and(|b| b.last().is_some_and(|&c| c != b'\n'));
    if torn {
        f.write_all(b"\n")
            .map_err(|e| io_error("cannot write the results file", &e))?;
    }
    Ok(f)
}

/// The computer for a task: the named one (created if missing), or a fresh one whose name is the
/// task id (cut to 34 characters, plus `suffix`) with `-2`, `-3`, ... if the name is taken.
fn computer_for(opts: &Options, name: Option<&str>, id: &str, suffix: &str) -> Result<Computer> {
    let home = opts.out.as_path();
    let mut c = match name {
        Some(n) => match Computer::open(home, n) {
            Ok(c) => c,
            Err(e) if e.code == Code::NotFound => Computer::create(home, n)?,
            Err(e) => return Err(e),
        },
        None => {
            let base = format!("{}{suffix}", cut_chars(id, 34));
            let mut k = 1;
            loop {
                let candidate = if k == 1 {
                    base.clone()
                } else {
                    format!("{base}-{k}")
                };
                match Computer::create(home, &candidate) {
                    Ok(c) => break c,
                    Err(e) if e.code == Code::Exists => k += 1,
                    Err(e) => return Err(e),
                }
            }
        }
    };
    if let Some(t) = &opts.tools {
        c.tools = t.clone();
    }
    Ok(c)
}

/// Runs the setup actions. Returns the text of the first action that failed, if any.
fn setup(c: &Computer, mind: &Store, cfg: &RunConfig, task: &Task) -> Option<String> {
    let ctx = Ctx {
        computer: c,
        mind,
        cfg,
        session: "setup",
    };
    task.setup
        .iter()
        .map(|a| actions::execute(&ctx, a))
        .find(|o| !o.ok)
        .map(|o| o.text)
}

/// Runs the reference solution. Returns its answer (the first `done`), its step count and how
/// many of its steps failed.
fn solution(
    c: &Computer,
    mind: &Store,
    cfg: &RunConfig,
    task: &Task,
) -> (Option<String>, u64, u64) {
    let ctx = Ctx {
        computer: c,
        mind,
        cfg,
        session: "solution",
    };
    let mut answer = None;
    let mut failed = 0;
    for a in &task.solution {
        if let Action::Done { answer: text } = a {
            if answer.is_none() {
                answer = Some(text.clone());
            }
        }
        if !actions::execute(&ctx, a).ok {
            failed += 1;
        }
    }
    (answer, task.solution.len() as u64, failed)
}

/// Runs the task's checks. `read PATH` looks at a file of the computer; other lines are commands.
fn check(c: &Computer, task: &Task, answer: Option<&str>, cfg: &RunConfig) -> Vec<CheckResult> {
    let mut run = |line: &str| -> Result<(i32, String)> {
        match line.strip_prefix("read ") {
            Some(rel) => {
                let path = c.resolve(rel.trim())?;
                Ok(match fs::read(&path) {
                    Ok(bytes) => (0, String::from_utf8_lossy(&bytes).into_owned()),
                    Err(_) => (1, String::new()),
                })
            }
            None => actions::run_line(c, line, cfg.tool_timeout_secs),
        }
    };
    suite::run_checks(task, answer, &mut run)
}

/// The class of a run that did not pass, from how it stopped.
fn stop_class(stop: &Stop) -> &'static str {
    match stop {
        Stop::Done => "wrong_result",
        Stop::StepLimit => "step_limit",
        Stop::ModelError(_) => "model_error",
    }
}

/// The stop label written to the results.
fn stop_label(stop: &Stop) -> &'static str {
    match stop {
        Stop::Done => "done",
        Stop::StepLimit => "step_limit",
        Stop::ModelError(_) => "model_error",
    }
}

/// One result record, filled in as the task runs.
struct Rec {
    json: Json,
}

impl Rec {
    /// Starts a record for a task; the status and class are set by the caller.
    fn new(suite: &str, id: &str, computer: &str) -> Self {
        Self {
            json: Json::Obj(vec![
                ("id".into(), s(id)),
                ("suite".into(), s(suite)),
                ("status".into(), s("fail")),
                ("retried".into(), Json::Bool(false)),
                ("retry_pass".into(), Json::Bool(false)),
                ("lesson".into(), Json::Null),
                ("class".into(), s("ok")),
                ("stop".into(), Json::Null),
                ("steps".into(), count(0)),
                ("invalid_actions".into(), count(0)),
                ("failed_actions".into(), count(0)),
                ("prompt_tokens".into(), count(0)),
                ("completion_tokens".into(), count(0)),
                ("model_ms".into(), count(0)),
                ("wall_ms".into(), count(0)),
                ("computer".into(), s(computer)),
                ("answer".into(), Json::Null),
                ("failed_checks".into(), Json::Arr(Vec::new())),
            ]),
        }
    }

    /// Sets one field.
    fn set(&mut self, key: &str, value: Json) {
        if let Json::Obj(fields) = &mut self.json {
            if let Some(slot) = fields.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            }
        }
    }

    /// Records the model run's counters, stop and answer.
    fn absorb(&mut self, out: &Outcome) {
        self.set("steps", count(out.steps as usize));
        self.set("invalid_actions", count(out.invalid_actions as usize));
        self.set("failed_actions", count(out.failed_actions as usize));
        self.set("prompt_tokens", count(out.prompt_tokens as usize));
        self.set("completion_tokens", count(out.completion_tokens as usize));
        self.set("model_ms", count(out.model_ms as usize));
        self.set("stop", s(stop_label(&out.stop)));
        self.set("answer", out.answer.as_deref().map_or(Json::Null, s));
    }

    /// Sets the status and the failed checks from the check results. Returns true when all pass.
    fn judge(&mut self, results: &[CheckResult], fail_class: &str) -> bool {
        let ok = all_pass(results);
        self.set("status", s(if ok { "pass" } else { "fail" }));
        self.set("class", s(if ok { "ok" } else { fail_class }));
        self.set("failed_checks", Json::Arr(failed_details(results)));
        ok
    }

    /// The failed check details as stored (cut to 300 bytes each).
    fn failed(&self) -> Vec<String> {
        match self.json.get("failed_checks").and_then(Json::as_array) {
            Some(a) => a
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Stores the wall time.
    fn finish(mut self, t0: Instant) -> Rec {
        let ms = t0.elapsed().as_millis();
        self.set("wall_ms", count(ms as usize));
        self
    }

    /// The record as one JSON line (without the newline).
    fn encode(&self) -> String {
        self.json.write()
    }
}

/// A JSON string.
fn s(text: &str) -> Json {
    Json::Str(text.to_string())
}

/// The failed checks' details, cut to 300 bytes each.
fn failed_details(results: &[CheckResult]) -> Vec<Json> {
    results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| s(&cut(&r.detail, 300)))
        .collect()
}

/// True when every check passed.
fn all_pass(results: &[CheckResult]) -> bool {
    results.iter().all(|r| r.passed)
}

/// Runs one task (first try, then learning) and returns its record.
fn run_task_record(opts: &Options, mind: &Store, suite: &str, task: &Task) -> Result<Rec> {
    let t0 = Instant::now();
    let cfg = &opts.cfg;
    let c = computer_for(opts, task.computer.as_deref(), &task.id, "")?;
    let mut rec = Rec::new(suite, &task.id, &c.name);
    let setup_cfg = RunConfig {
        level: Level::Memory,
        ..cfg.clone()
    };
    if let Some(text) = setup(&c, mind, &setup_cfg, task) {
        rec.set("status", s("setup_error"));
        rec.set("class", s("setup_error"));
        rec.set("failed_checks", Json::Arr(vec![s(&cut(&text, 300))]));
        return Ok(rec.finish(t0));
    }
    if opts.selftest {
        selftest(&c, mind, &setup_cfg, task, cfg, &mut rec);
        return Ok(rec.finish(t0));
    }
    let ctx = Ctx {
        computer: &c,
        mind,
        cfg,
        session: &task.id,
    };
    let out = agent::run_task(&ctx, &task.prompt);
    rec.absorb(&out);
    rec.judge(
        &check(&c, task, out.answer.as_deref(), cfg),
        stop_class(&out.stop),
    );
    if cfg.level == Level::Memory && !cfg.freeze_memory {
        learn(opts, mind, task, &ctx, &out, &mut rec)?;
    }
    Ok(rec.finish(t0))
}

/// Checks the task before and after its reference solution.
fn selftest(
    c: &Computer,
    mind: &Store,
    setup_cfg: &RunConfig,
    task: &Task,
    cfg: &RunConfig,
    rec: &mut Rec,
) {
    rec.set("stop", s("selftest"));
    if all_pass(&check(c, task, None, cfg)) {
        rec.set("status", s("fail"));
        rec.set("class", s("selftest"));
        rec.set(
            "failed_checks",
            Json::Arr(vec![s("checks already pass before the solution")]),
        );
        return;
    }
    let (answer, steps, failed) = solution(c, mind, setup_cfg, task);
    rec.set("steps", count(steps as usize));
    rec.set("failed_actions", count(failed as usize));
    rec.set("answer", answer.as_deref().map_or(Json::Null, s));
    rec.judge(&check(c, task, answer.as_deref(), cfg), "selftest");
}

/// Stores what a run taught: a skill after a clean pass, a lesson after a failure. A lesson from a
/// task without a named computer is tried once more on a fresh computer and verified if it helps.
fn learn(
    opts: &Options,
    mind: &Store,
    task: &Task,
    ctx: &Ctx<'_>,
    out: &Outcome,
    rec: &mut Rec,
) -> Result<()> {
    if rec.json.get("status").and_then(Json::as_str) == Some("pass") {
        if !out.good_commands.is_empty() {
            mind.add(
                Kind::Skill,
                &skill_text(&task.prompt, &out.good_commands),
                true,
            )?;
        }
        return Ok(());
    }
    let failed = rec.failed();
    let Some(lesson) = agent::reflect(ctx, &task.prompt, &failed) else {
        return Ok(());
    };
    let mut retry_pass = false;
    if task.computer.is_none() {
        let r = computer_for(opts, None, &task.id, "-r")?;
        let setup_cfg = RunConfig {
            level: Level::Memory,
            ..opts.cfg.clone()
        };
        if setup(&r, mind, &setup_cfg, task).is_none() {
            let rcfg = RunConfig {
                extra_lessons: vec![lesson.clone()],
                ..opts.cfg.clone()
            };
            let rctx = Ctx {
                computer: &r,
                mind,
                cfg: &rcfg,
                session: &task.id,
            };
            let rout = agent::run_task(&rctx, &task.prompt);
            retry_pass = all_pass(&check(&r, task, rout.answer.as_deref(), &opts.cfg));
            rec.set("retried", Json::Bool(true));
            rec.set("retry_pass", Json::Bool(retry_pass));
        }
    }
    mind.add(Kind::Lesson, &lesson, retry_pass)?;
    rec.set("lesson", s(&lesson));
    Ok(())
}

/// The skill text: the prompt's first line, then the commands that worked.
fn skill_text(prompt: &str, commands: &[String]) -> String {
    let first = prompt.lines().next().unwrap_or_default();
    cut(
        &format!("{} => {}", cut_chars(first, 120), commands.join(" ; ")),
        600,
    )
}

/// The one-line progress text for a finished task.
fn progress(rec: &Rec) -> String {
    let j = &rec.json;
    let label = match sval(j, "status") {
        Some("pass") => "PASS",
        Some("setup_error") => "SETUP-ERROR",
        _ if bval(j, "retry_pass") => "RETRY-PASS",
        _ => "FAIL",
    };
    format!(
        "{} {label} {:.0} steps {:.0}s",
        sval(j, "id").unwrap_or("?"),
        nval(j, "steps"),
        nval(j, "wall_ms") / 1000.0
    )
}

/// A string field of a record.
fn sval<'a>(r: &'a Json, key: &str) -> Option<&'a str> {
    r.get(key).and_then(Json::as_str)
}

/// A number field of a record (0 when missing).
fn nval(r: &Json, key: &str) -> f64 {
    r.get(key).and_then(Json::as_i64).unwrap_or(0) as f64
}

/// A true/false field of a record (false when missing).
fn bval(r: &Json, key: &str) -> bool {
    r.get(key).and_then(Json::as_bool).unwrap_or(false)
}

/// True when the record passed on the first try.
fn is_pass(r: &Json) -> bool {
    sval(r, "status") == Some("pass")
}

/// The totals of all records.
fn summary(records: &[Json], report: String) -> Summary {
    Summary {
        tasks: records.len(),
        passed: records.iter().filter(|r| is_pass(r)).count(),
        passed_after_retry: records
            .iter()
            .filter(|r| is_pass(r) || bval(r, "retry_pass"))
            .count(),
        setup_errors: records
            .iter()
            .filter(|r| sval(r, "status") == Some("setup_error"))
            .count(),
        report,
    }
}

/// `part / whole`, or 0 when there is nothing to divide by.
fn ratio(part: f64, whole: f64) -> f64 {
    if whole > 0.0 {
        part / whole
    } else {
        0.0
    }
}

/// One table row of the report for `rows`.
fn stats_row(label: &str, rows: &[&Json]) -> String {
    let n = rows.len() as f64;
    let sum = |key: &str| rows.iter().map(|r| nval(r, key)).sum::<f64>();
    let pass = rows.iter().filter(|r| is_pass(r)).count();
    let after = rows
        .iter()
        .filter(|r| is_pass(r) || bval(r, "retry_pass"))
        .count();
    let steps = sum("steps");
    format!(
        "| {label} | {} | {pass} | {:.0} | {after} | {:.1} | {:.1} | {:.0} | {:.0} | {:.1} |",
        rows.len(),
        100.0 * ratio(pass as f64, n),
        ratio(steps, n),
        100.0 * ratio(sum("invalid_actions"), steps),
        sum("prompt_tokens"),
        sum("completion_tokens"),
        ratio(sum("model_ms"), 1000.0),
    )
}

/// The markdown report over all records.
fn make_report(opts: &Options, records: &[Json]) -> String {
    let mode = if opts.selftest {
        "selftest".to_string()
    } else {
        format!("level {}", opts.cfg.level.number())
    };
    let frozen = if opts.cfg.freeze_memory {
        "frozen"
    } else {
        "not frozen"
    };
    let mut out = format!("# Exam report: {mode}, memory {frozen}\n\n");
    out.push_str("| suite | tasks | pass | pass % | pass after retry | mean steps | invalid % | prompt tok | completion tok | model s |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
    let mut suites: Vec<&str> = Vec::new();
    for r in records {
        if let Some(name) = sval(r, "suite") {
            if !suites.contains(&name) {
                suites.push(name);
            }
        }
    }
    for name in &suites {
        let rows: Vec<&Json> = records
            .iter()
            .filter(|r| sval(r, "suite") == Some(*name))
            .collect();
        out.push_str(&stats_row(name, &rows));
        out.push('\n');
    }
    let all: Vec<&Json> = records.iter().collect();
    out.push_str(&stats_row("total", &all));
    out.push_str("\n\nFailures by class:\n");
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for r in records.iter().filter(|r| !is_pass(r)) {
        *classes
            .entry(sval(r, "class").unwrap_or("unknown"))
            .or_insert(0) += 1;
    }
    for (class, n) in &classes {
        out.push_str(&format!("{class}: {n}\n"));
    }
    out.push_str("\nFailed tasks:\n");
    for r in records.iter().filter(|r| !is_pass(r)) {
        let first = r
            .get("failed_checks")
            .and_then(Json::as_array)
            .and_then(|a| a.first())
            .and_then(Json::as_str)
            .unwrap_or("");
        out.push_str(&format!(
            "{} ({}): {}\n",
            sval(r, "id").unwrap_or("?"),
            sval(r, "class").unwrap_or("?"),
            cut_chars(first, 160)
        ));
    }
    out
}
