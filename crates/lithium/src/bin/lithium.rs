//! Command-line tool for the lithium RV64 kernel.
#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

use silicon::machine::{Config, Exit};

const VERSION: &str = "lithium 0.1.0";
const DEFAULT_STEPS: u64 = 1_000_000;

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
}

impl CliErr {
    fn fail(msg: impl Into<String>) -> Self {
        Self {
            code: 1,
            msg: msg.into(),
        }
    }

    fn usage(msg: impl Into<String>) -> Self {
        Self {
            code: 2,
            msg: msg.into(),
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

fn cmd_run(steps: u64) -> Result<(), CliErr> {
    let mut machine =
        lithium::boot(Config::default()).map_err(|e| CliErr::fail(format!("boot failed: {e}")))?;
    let start_mtime = machine.board.mtime();
    let exit = machine.run(steps);
    let executed = machine.board.mtime().saturating_sub(start_mtime);
    let uart = machine.board.take_uart_output();

    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&uart)
        .map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;
    if !uart.is_empty() && !uart.ends_with(b"\n") {
        writeln!(stdout).map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;
    }
    writeln!(stdout, "exit: {}", format_exit(exit))
        .map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;
    writeln!(stdout, "steps: {executed}")
        .map_err(|e| CliErr::fail(format!("stdout error: {e}")))?;

    match exit {
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lithium: {}", err.msg);
            ExitCode::from(err.code)
        }
    }
}
