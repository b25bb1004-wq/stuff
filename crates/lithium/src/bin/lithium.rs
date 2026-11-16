//! Command-line tool for the lithium RV64 kernel.
//!
//! Text mode is unchanged. `--describe` prints the manifest and `--json` prints one ACI envelope
//! (docs/ai-computer/ACI.md). Both modes share `run_machine` and the `Run` result.
#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use silicon::machine::{Config, Exit};

const VERSION: &str = "lithium 0.1.0";
const DEFAULT_STEPS: u64 = 1_000_000;

const RUN_ARGS: &[Arg] = &[Arg {
    name: "--steps",
    kind: ArgKind::Int,
    required: false,
    default: Some("1000000"),
    choices: &[],
}];
const IMAGE_ARGS: &[Arg] = &[Arg {
    name: "-o",
    kind: ArgKind::Path,
    required: true,
    default: None,
    choices: &[],
}];

const COMMANDS: &[Command] = &[
    Command {
        name: "run",
        summary: "Boot lithium on Silicon and run up to --steps N (default 1000000)",
        effect: Effect::Read,
        args: RUN_ARGS,
        returns: "exit (only when stop is exit), stdout (capped, with stdout_truncated, stdout_bytes, stdout_lossy when they apply), steps, duration_ms, stop: exit, step_limit",
        example: "lithium --json run --steps 100000",
    },
    Command {
        name: "image",
        summary: "Write the assembled kernel image to -o OUT.bin (--json needs a file, not -)",
        effect: Effect::Write,
        args: IMAGE_ARGS,
        returns: "path, bytes",
        example: "lithium --json image -o kernel.bin",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "lithium",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Lithium RV64 kernel boot tool",
    commands: COMMANDS,
};

const USAGE: &str = "\
lithium - boot the lithium RV64 OS image or export its binary

Usage:
  lithium run [--steps N]
  lithium image -o OUT.bin
  lithium --help
  lithium --version

Subcommands:
  run [--steps N]    Boot lithium on Silicon and print console output, exit, and steps (default: 1000000)
  image -o OUT.bin   Write the assembled kernel image to OUT.bin (or '-' for stdout)

Examples:
  lithium run --steps 100000
  lithium image -o kernel.bin";

enum Action {
    Help,
    Version,
    Run { steps: u64 },
    Image { out: String },
}

struct CliErr {
    code: u8,
    msg: String,
    /// The error code under `--json`.
    kind: Code,
}

impl CliErr {
    fn fail(msg: impl Into<String>) -> Self {
        Self {
            code: 1,
            msg: msg.into(),
            kind: Code::Invalid,
        }
    }

    fn io(msg: impl Into<String>) -> Self {
        Self {
            code: 1,
            msg: msg.into(),
            kind: Code::Io,
        }
    }

    fn usage(msg: impl Into<String>) -> Self {
        Self {
            code: 2,
            msg: msg.into(),
            kind: Code::Usage,
        }
    }
}

fn parse_steps(raw: &str) -> Result<u64, CliErr> {
    let n: u64 = raw
        .parse()
        .map_err(|_| CliErr::fail(format!("invalid step count '{raw}'")))?;
    if n == 0 {
        return Err(CliErr::fail("step count must be at least 1"));
    }
    Ok(n)
}

fn parse_run(args: &[String]) -> Result<Action, CliErr> {
    let mut steps = DEFAULT_STEPS;
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            return Ok(Action::Help);
        } else if arg == "--steps" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliErr::usage("missing value for --steps"))?;
            steps = parse_steps(val)?;
        } else if let Some(val) = arg.strip_prefix("--steps=") {
            steps = parse_steps(val)?;
        } else {
            return Err(CliErr::usage(format!("unknown argument for run: '{arg}'")));
        }
        i += 1;
    }
    Ok(Action::Run { steps })
}

fn parse_image(args: &[String]) -> Result<Action, CliErr> {
    let mut out: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            return Ok(Action::Help);
        } else if arg == "-o" || arg == "--out" {
            i += 1;
            let val = args
                .get(i)
                .ok_or_else(|| CliErr::usage(format!("missing path after {arg}")))?;
            out = Some(val.clone());
        } else if let Some(val) = arg.strip_prefix("--out=") {
            out = Some(val.to_string());
        } else {
            return Err(CliErr::usage(format!(
                "unknown argument for image: '{arg}'"
            )));
        }
        i += 1;
    }
    let out = out.ok_or_else(|| CliErr::usage("missing required option -o OUT.bin"))?;
    if out.is_empty() {
        return Err(CliErr::fail("output path must not be empty"));
    }
    Ok(Action::Image { out })
}

fn parse_args(args: &[String]) -> Result<Action, CliErr> {
    let Some(first) = args.first() else {
        return Err(CliErr::usage("missing subcommand (try --help)"));
    };
    match first.as_str() {
        "--help" | "-h" => Ok(Action::Help),
        "--version" | "-V" => Ok(Action::Version),
        "run" => parse_run(&args[1..]),
        "image" => parse_image(&args[1..]),
        other if other.starts_with('-') => Err(CliErr::usage(format!("unknown option '{other}'"))),
        other => Err(CliErr::usage(format!("unknown subcommand '{other}'"))),
    }
}

fn format_exit(exit: Exit) -> String {
    match exit {
        Exit::Pass => "pass".to_string(),
        Exit::Fail(code) => format!("fail({code})"),
        Exit::OutOfSteps => "out_of_steps".to_string(),
    }
}

/// One boot and run: how it stopped, what the guest printed, and the steps and time it used.
struct Run {
    exit: Exit,
    uart: Vec<u8>,
    steps: u64,
    duration_ms: i64,
}

fn run_machine(steps: u64) -> Result<Run, CliErr> {
    let mut machine = lithium::boot(Config::default()).map_err(|e| CliErr {
        code: 1,
        msg: format!("boot failed: {e}"),
        kind: Code::Internal,
    })?;
    let start_mtime = machine.board.mtime();
    let started = Instant::now();
    let exit = machine.run(steps);
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let executed = machine.board.mtime().saturating_sub(start_mtime);
    let uart = machine.board.take_uart_output();
    Ok(Run {
        exit,
        uart,
        steps: executed,
        duration_ms,
    })
}

fn cmd_run(steps: u64) -> Result<(), CliErr> {
    let run = run_machine(steps)?;
    let uart = &run.uart;

    let mut stdout = io::stdout().lock();
    stdout
        .write_all(uart)
        .map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;
    if !uart.is_empty() && !uart.ends_with(b"\n") {
        writeln!(stdout).map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;
    }
    writeln!(stdout, "exit: {}", format_exit(run.exit))
        .map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;
    writeln!(stdout, "steps: {}", run.steps)
        .map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;

    match run.exit {
        Exit::Pass | Exit::OutOfSteps => Ok(()),
        Exit::Fail(code) => Err(CliErr::fail(format!("kernel exited with fail({code})"))),
    }
}

fn cmd_image(out: &str) -> Result<(), CliErr> {
    let bytes = lithium::image().map_err(|e| CliErr::fail(format!("assemble failed: {e}")))?;
    if out == "-" {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(&bytes)
            .map_err(|e| CliErr::fail(format!("failed to write stdout: {e}")))?;
        return Ok(());
    }
    fs::write(out, &bytes).map_err(|e| CliErr::fail(format!("failed to write '{out}': {e}")))?;
    println!("out: {out}");
    println!("bytes: {}", bytes.len());
    Ok(())
}

fn dispatch(args: &[String]) -> Result<(), CliErr> {
    match parse_args(args)? {
        Action::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Action::Version => {
            println!("{VERSION}");
            Ok(())
        }
        Action::Run { steps } => cmd_run(steps),
        Action::Image { out } => cmd_image(&out),
    }
}

fn main() -> ExitCode {
    let mut raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if json {
        return run_json(&args);
    }
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lithium: {}", err.msg);
            ExitCode::from(err.code)
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(first: Option<&String>) -> &'static str {
    const NAMES: [&str; 4] = ["run", "image", "--help", "--version"];
    first
        .and_then(|s| NAMES.iter().copied().find(|n| *n == s.as_str()))
        .unwrap_or("")
}

/// `lithium --json ...`: one envelope on stdout; stderr keeps the `lithium: message` line.
fn run_json(args: &[String]) -> ExitCode {
    let cmd = known_command(args.first());
    match json_dispatch(args) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("lithium", cmd, data));
            ExitCode::from(exit)
        }
        Err(err) => {
            eprintln!("lithium: {}", err.msg);
            let hint = match err.kind {
                Code::Usage => Some("run: lithium --describe"),
                _ => None,
            };
            print!(
                "{}",
                envelope::err("lithium", cmd, err.kind, &err.msg, hint)
            );
            ExitCode::from(err.code)
        }
    }
}

/// Run one command under `--json`. Returns the data and the exit code the text path gives.
fn json_dispatch(args: &[String]) -> Result<(Json, u8), CliErr> {
    match parse_args(args)? {
        Action::Help => Ok((Json::obj([("usage", Json::Str(USAGE.into()))]), 0)),
        Action::Version => Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0)),
        Action::Run { steps } => {
            let run = run_machine(steps)?;
            // A guest that failed its own test is ok: true with the text path's exit code 1.
            let (stop, guest_exit, exit) = match run.exit {
                Exit::Pass => ("exit", Some(0), 0),
                Exit::Fail(code) => ("exit", Some(i64::from(code)), 1),
                Exit::OutOfSteps => ("step_limit", None, 0),
            };
            let mut pairs = Vec::new();
            if let Some(code) = guest_exit {
                pairs.push(("exit".to_string(), Json::Int(code)));
            }
            pairs.extend(stream("stdout", &run.uart));
            pairs.push(("steps".to_string(), int(run.steps)));
            pairs.push(("duration_ms".to_string(), Json::Int(run.duration_ms)));
            pairs.push(("stop".to_string(), Json::Str(stop.into())));
            Ok((Json::Obj(pairs), exit))
        }
        Action::Image { out } => {
            if out == "-" {
                return Err(CliErr::usage("--json needs a file: image -o FILE, not -"));
            }
            let bytes = lithium::image().map_err(|e| CliErr {
                code: 1,
                msg: format!("assemble failed: {e}"),
                kind: Code::Internal,
            })?;
            fs::write(&out, &bytes)
                .map_err(|e| CliErr::io(format!("failed to write '{out}': {e}")))?;
            let data = Json::obj([
                ("path", Json::path(std::path::Path::new(&out))),
                ("bytes", int(bytes.len())),
            ]);
            Ok((data, 0))
        }
    }
}

fn int<T: TryInto<i64>>(n: T) -> Json {
    Json::Int(n.try_into().unwrap_or(i64::MAX))
}

/// A captured stream: `name` for the text, `name_<field>` for `truncated`, `bytes`, `lossy`.
fn stream(name: &str, bytes: &[u8]) -> Vec<(String, Json)> {
    envelope::capped(bytes)
        .into_iter()
        .map(|(key, value)| {
            if key == "text" {
                (name.to_string(), value)
            } else {
                (format!("{name}_{key}"), value)
            }
        })
        .collect()
}
