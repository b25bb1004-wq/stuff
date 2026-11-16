//! Command-line RISC-V machine runner using `chisel::elf` and `silicon`.
//!
//! Text mode prints what it always printed. `--describe` prints the manifest and `--json` prints one ACI
//! envelope (docs/ai-computer/ACI.md). Both modes share `simulate`.
#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use silicon::machine::{Config, Exit, Machine};
use silicon::Bus;

const VERSION: &str = "silicon 0.1.0";
const DEFAULT_STEPS: u64 = 1_000_000;
const TRACE_LIMIT: u64 = 1000;
const MIB: usize = 1024 * 1024;

const USAGE: &str = "\
Usage: silicon <COMMAND>

Commands:
  run ELF [--steps N] [--ram MiB] [--trace]  Load an RV64 ELF and run it on Silicon

Options:
  -h, --help     Print help
  -V, --version  Print version

Examples:
  silicon run program.elf
  silicon run program.elf --steps 50000 --ram 32 --trace
  silicon run - < program.elf";

const RUN_USAGE: &str = "\
Usage: silicon run ELF [--steps N] [--ram MiB] [--trace]

Arguments:
  ELF            Path to an RV64 ELF64 executable, or '-' for stdin

Options:
  --steps N      Maximum steps to execute (default: 1000000)
  --ram MiB      Guest RAM size in MiB (default: 16)
  --trace        Print up to 1000 'pc inst' lines during execution
  -h, --help     Print help";

const RUN_ARGS: &[Arg] = &[
    Arg {
        name: "ELF",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--steps",
        kind: Kind::Int,
        required: false,
        default: Some("1000000"),
        choices: &[],
    },
    Arg {
        name: "--ram",
        kind: Kind::Int,
        required: false,
        default: Some("16"),
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[Command {
    name: "run",
    summary: "Load an RV64 ELF and run it on Silicon (--trace is text-only)",
    effect: Effect::Exec,
    args: RUN_ARGS,
    returns: "exit (number, or null at the step limit), stop (exit or step_limit), stdout (capped, with stdout_truncated, stdout_bytes, stdout_lossy when they apply), steps, duration_ms",
    example: "silicon --json run program.elf --steps 50000",
}];

const MANIFEST: Manifest = Manifest {
    tool: "silicon",
    version: env!("CARGO_PKG_VERSION"),
    summary: "RISC-V CPU emulator (Silicon) for Monolith ELF programs",
    commands: COMMANDS,
};

struct RunArgs {
    elf_path: String,
    steps: u64,
    ram_bytes: usize,
    trace: bool,
}

enum Action {
    Help(&'static str),
    Version,
    Run(RunArgs),
}

/// A failed run: the code and message for the envelope, the hint, and the process exit code.
struct Fail {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: u8,
}

impl Fail {
    fn usage(message: String) -> Fail {
        Fail {
            code: Code::Usage,
            message,
            hint: Some(String::from("run: silicon --describe")),
            exit: 2,
        }
    }

    fn op(code: Code, message: String) -> Fail {
        Fail {
            code,
            message,
            hint: None,
            exit: 1,
        }
    }
}

fn err_msg(msg: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "silicon: {msg}");
    ExitCode::from(1)
}

fn usage_err(msg: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "silicon: {msg}");
    ExitCode::from(2)
}

fn parse_u64_arg(flag: &str, val: &str) -> Result<u64, String> {
    val.parse::<u64>()
        .map_err(|_| format!("invalid {flag} value: {val}"))
}

fn parse_ram_bytes(val: &str) -> Result<usize, String> {
    let mib = val
        .parse::<usize>()
        .map_err(|_| format!("invalid --ram value: {val}"))?;
    if mib == 0 {
        return Err("--ram must be at least 1 MiB".to_owned());
    }
    mib.checked_mul(MIB)
        .ok_or_else(|| format!("--ram value too large: {val}"))
}

fn parse_run_args(args: &[String]) -> Result<Action, String> {
    let mut elf_path: Option<String> = None;
    let mut steps = DEFAULT_STEPS;
    let mut ram_bytes = Config::default().ram_size;
    let mut trace = false;
    let mut positional_only = false;
    let mut i = 0usize;

    while i < args.len() {
        let arg = &args[i];
        if !positional_only && (arg == "--help" || arg == "-h") {
            return Ok(Action::Help(RUN_USAGE));
        }
        if !positional_only && arg == "--" {
            positional_only = true;
            i += 1;
            continue;
        }
        if !positional_only && arg == "--trace" {
            trace = true;
            i += 1;
            continue;
        }
        if !positional_only && arg == "--steps" {
            let Some(val) = args.get(i + 1) else {
                return Err("missing value for --steps".to_owned());
            };
            steps = parse_u64_arg("--steps", val)?;
            i += 2;
            continue;
        }
        if !positional_only {
            if let Some(val) = arg.strip_prefix("--steps=") {
                steps = parse_u64_arg("--steps", val)?;
                i += 1;
                continue;
            }
        }
        if !positional_only && arg == "--ram" {
            let Some(val) = args.get(i + 1) else {
                return Err("missing value for --ram".to_owned());
            };
            ram_bytes = parse_ram_bytes(val)?;
            i += 2;
            continue;
        }
        if !positional_only {
            if let Some(val) = arg.strip_prefix("--ram=") {
                ram_bytes = parse_ram_bytes(val)?;
                i += 1;
                continue;
            }
        }
        if !positional_only && arg != "-" && arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        }
        if elf_path.is_some() {
            return Err(format!("unexpected argument: {arg}"));
        }
        elf_path = Some(arg.clone());
        i += 1;
    }

    let Some(elf_path) = elf_path else {
        return Err("missing ELF path".to_owned());
    };
    Ok(Action::Run(RunArgs {
        elf_path,
        steps,
        ram_bytes,
        trace,
    }))
}

fn parse_args(args: &[String]) -> Result<Action, String> {
    let Some(first) = args.first() else {
        return Err("missing subcommand (try --help)".to_owned());
    };
    match first.as_str() {
        "-h" | "--help" => Ok(Action::Help(USAGE)),
        "-V" | "--version" => Ok(Action::Version),
        "run" => parse_run_args(&args[1..]),
        other => Err(format!("unknown subcommand: {other}")),
    }
}

fn read_elf_bytes(path: &str) -> Result<Vec<u8>, String> {
    if path == "-" {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("stdin: {e}"))?;
        Ok(buf)
    } else {
        fs::read(path).map_err(|e| format!("{path}: {e}"))
    }
}

fn format_inst(board: &mut impl Bus, pc: u64) -> String {
    let Ok(lo) = board.read(pc, 2) else {
        return "????????".to_owned();
    };
    let lo = lo as u16;
    if (lo & 0b11) != 0b11 {
        format!("0x{lo:04x}")
    } else if let Ok(hi) = board.read(pc.wrapping_add(2), 2) {
        let inst = u32::from(lo) | (u32::from(hi as u16) << 16);
        format!("0x{inst:08x}")
    } else {
        "????????".to_owned()
    }
}

/// What a finished run produced: the stop reason, the instruction count and the UART bytes.
struct Outcome {
    exit: Exit,
    instret: u64,
    uart: Vec<u8>,
}

/// Load the ELF and run it. `trace` is the writer for the per-step lines, or None when not tracing.
/// Both the text and the `--json` paths call this.
fn simulate(args: &RunArgs, trace: Option<&mut dyn Write>) -> Result<Outcome, Fail> {
    let bytes = read_elf_bytes(&args.elf_path).map_err(|m| Fail::op(Code::Io, m))?;
    let img = chisel::elf::read(&bytes)
        .map_err(|e| Fail::op(Code::Invalid, format!("invalid ELF: {e}")))?;

    let cfg = Config {
        ram_size: args.ram_bytes,
        ..Config::default()
    };
    let mut machine = Machine::new(cfg);

    if let Err(fault) = chisel::elf::load(&img, &mut machine) {
        return Err(Fail::op(
            Code::Invalid,
            format!("failed to load ELF: {fault:?}"),
        ));
    }

    let exit = match trace {
        Some(w) => {
            let trace_steps = args.steps.min(TRACE_LIMIT);
            let mut outcome = Exit::OutOfSteps;
            for _ in 0..trace_steps {
                let pc = machine.cpu.pc;
                let inst = format_inst(&mut machine.board, pc);
                writeln!(w, "0x{pc:016x} {inst}")
                    .map_err(|e| Fail::op(Code::Io, format!("stdout: {e}")))?;
                outcome = machine.run(1);
                if outcome != Exit::OutOfSteps {
                    break;
                }
            }
            if outcome == Exit::OutOfSteps && args.steps > trace_steps {
                outcome = machine.run(args.steps - trace_steps);
            }
            outcome
        }
        None => machine.run(args.steps),
    };

    Ok(Outcome {
        exit,
        instret: machine.cpu.instret,
        uart: machine.board.take_uart_output(),
    })
}

fn run_elf(args: RunArgs) -> ExitCode {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let trace: Option<&mut dyn Write> = if args.trace { Some(&mut out) } else { None };
    let outcome = match simulate(&args, trace) {
        Ok(o) => o,
        Err(fail) => return err_msg(&fail.message),
    };
    let exit = outcome.exit;

    if !outcome.uart.is_empty() {
        if let Err(e) = out.write_all(&outcome.uart) {
            return err_msg(&format!("stdout: {e}"));
        }
        if !outcome.uart.ends_with(b"\n") {
            if let Err(e) = out.write_all(b"\n") {
                return err_msg(&format!("stdout: {e}"));
            }
        }
    }

    if let Err(e) = writeln!(out, "exit: {exit:?}") {
        return err_msg(&format!("stdout: {e}"));
    }
    if let Err(e) = writeln!(out, "instret: {}", outcome.instret) {
        return err_msg(&format!("stdout: {e}"));
    }
    if let Err(e) = out.flush() {
        return err_msg(&format!("stdout: {e}"));
    }

    match exit {
        Exit::Pass | Exit::OutOfSteps => ExitCode::SUCCESS,
        Exit::Fail(code) => err_msg(&format!("guest exited with Fail({code})")),
    }
}

/// `silicon --json ...`: one envelope on stdout; stderr keeps the `silicon: message` line.
fn run_json(args: &[OsString]) -> ExitCode {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cmd = if args.first().map(String::as_str) == Some("run") {
        "run"
    } else {
        ""
    };
    match json_command(&args) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("silicon", cmd, data));
            ExitCode::from(exit)
        }
        Err(fail) => {
            eprintln!("silicon: {}", fail.message);
            print!(
                "{}",
                envelope::err(
                    "silicon",
                    cmd,
                    fail.code,
                    &fail.message,
                    fail.hint.as_deref()
                )
            );
            ExitCode::from(fail.exit)
        }
    }
}

fn json_command(args: &[String]) -> Result<(Json, u8), Fail> {
    match parse_args(args).map_err(Fail::usage)? {
        Action::Help(text) => Ok((Json::obj([("usage", Json::Str(text.into()))]), 0)),
        Action::Version => Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0)),
        Action::Run(run) => {
            if run.trace {
                return Err(Fail {
                    code: Code::Usage,
                    message: String::from("--trace is not available under --json"),
                    hint: Some(String::from("run: silicon --describe")),
                    exit: 2,
                });
            }
            let started = Instant::now();
            let outcome = simulate(&run, None)?;
            let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            let (exit, stop, tool_exit) = match outcome.exit {
                Exit::Pass => (Json::Int(0), "exit", 0),
                Exit::Fail(code) => {
                    eprintln!("silicon: guest exited with Fail({code})");
                    (Json::Int(i64::from(code)), "exit", 1)
                }
                Exit::OutOfSteps => (Json::Null, "step_limit", 0),
            };
            let mut pairs = vec![
                ("exit".to_string(), exit),
                ("stop".to_string(), Json::Str(stop.into())),
            ];
            pairs.extend(stream("stdout", &outcome.uart));
            pairs.push((
                "steps".to_string(),
                Json::Int(i64::try_from(outcome.instret).unwrap_or(i64::MAX)),
            ));
            pairs.push(("duration_ms".to_string(), Json::Int(duration_ms)));
            Ok((Json::Obj(pairs), tool_exit))
        }
    }
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

fn main() -> ExitCode {
    let mut raw: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    if json {
        return run_json(&raw);
    }
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(Action::Help(text)) => {
            if let Err(e) = writeln!(io::stdout(), "{text}") {
                return err_msg(&format!("stdout: {e}"));
            }
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            if let Err(e) = writeln!(io::stdout(), "{VERSION}") {
                return err_msg(&format!("stdout: {e}"));
            }
            ExitCode::SUCCESS
        }
        Ok(Action::Run(run_args)) => run_elf(run_args),
        Err(msg) => usage_err(&msg),
    }
}
