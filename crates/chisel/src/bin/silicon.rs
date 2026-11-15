//! Command-line RISC-V machine runner using `chisel::elf` and `silicon`.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

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

fn run_elf(args: RunArgs) -> ExitCode {
    let bytes = match read_elf_bytes(&args.elf_path) {
        Ok(b) => b,
        Err(msg) => return err_msg(&msg),
    };
    let img = match chisel::elf::read(&bytes) {
        Ok(img) => img,
        Err(e) => return err_msg(&format!("invalid ELF: {e}")),
    };

    let cfg = Config {
        ram_size: args.ram_bytes,
        ..Config::default()
    };
    let mut machine = Machine::new(cfg);

    if let Err(fault) = chisel::elf::load(&img, &mut machine) {
        return err_msg(&format!("failed to load ELF: {fault:?}"));
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    let exit = if args.trace {
        let trace_steps = args.steps.min(TRACE_LIMIT);
        let mut outcome = Exit::OutOfSteps;
        for _ in 0..trace_steps {
            let pc = machine.cpu.pc;
            let inst = format_inst(&mut machine.board, pc);
            if let Err(e) = writeln!(out, "0x{pc:016x} {inst}") {
                return err_msg(&format!("stdout: {e}"));
            }
            outcome = machine.run(1);
            if outcome != Exit::OutOfSteps {
                break;
            }
        }
        if outcome == Exit::OutOfSteps && args.steps > trace_steps {
            outcome = machine.run(args.steps - trace_steps);
        }
        outcome
    } else {
        machine.run(args.steps)
    };

    let uart = machine.board.take_uart_output();
    if !uart.is_empty() {
        if let Err(e) = out.write_all(&uart) {
            return err_msg(&format!("stdout: {e}"));
        }
        if !uart.ends_with(b"\n") {
            if let Err(e) = out.write_all(b"\n") {
                return err_msg(&format!("stdout: {e}"));
            }
        }
    }

    if let Err(e) = writeln!(out, "exit: {exit:?}") {
        return err_msg(&format!("stdout: {e}"));
    }
    if let Err(e) = writeln!(out, "instret: {}", machine.cpu.instret) {
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

fn main() -> ExitCode {
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
