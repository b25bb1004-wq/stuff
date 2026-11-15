//! Command-line runner for booting the Titan kernel on Silicon.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use neon::session::{Pump, Titan};

const DEFAULT_KERNEL: &str = "target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel";
const BUILD_CMD: &str =
    "cargo build --target-dir target/titan-kernel -p titan --bin kernel --release --target riscv64gc-unknown-none-elf";
const CHUNK_STEPS: u64 = 500_000;
const MAX_WAIT_STEPS: u64 = 100_000_000;

enum Action {
    Help,
    Version,
    Boot {
        kernel: PathBuf,
        script: Option<String>,
    },
}

enum StopReason {
    Prompt,
    Exited(i32),
}

struct CliError {
    code: u8,
    message: String,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
        }
    }
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "titan: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}

fn run(args: impl Iterator<Item = OsString>) -> Result<(), CliError> {
    match parse_args(args)? {
        Action::Help => print_help().map_err(CliError::failed),
        Action::Version => print_version().map_err(CliError::failed),
        Action::Boot { kernel, script } => run_boot(&kernel, script.as_deref()),
    }
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Action, CliError> {
    let mut iter = args.peekable();
    let Some(first) = iter.next() else {
        return Err(CliError::usage(
            "missing subcommand (run `titan --help` for usage)",
        ));
    };

    let first_str = first.to_string_lossy();
    match first_str.as_ref() {
        "--help" | "-h" => Ok(Action::Help),
        "--version" | "-V" => Ok(Action::Version),
        "boot" => parse_boot_args(iter),
        other => Err(CliError::usage(format!(
            "unknown subcommand '{other}' (run `titan --help` for usage)"
        ))),
    }
}

fn parse_boot_args(mut iter: impl Iterator<Item = OsString>) -> Result<Action, CliError> {
    let mut kernel = PathBuf::from(DEFAULT_KERNEL);
    let mut script: Option<String> = None;

    while let Some(arg) = iter.next() {
        let s = arg.to_string_lossy();
        match s.as_ref() {
            "--help" | "-h" => return Ok(Action::Help),
            "--kernel" => {
                let val = iter
                    .next()
                    .ok_or_else(|| CliError::usage("missing path after --kernel"))?;
                kernel = PathBuf::from(val);
            }
            "--script" => {
                let val = iter
                    .next()
                    .ok_or_else(|| CliError::usage("missing file after --script"))?;
                let val_str = val
                    .into_string()
                    .map_err(|_| CliError::usage("--script path must be valid UTF-8"))?;
                script = Some(val_str);
            }
            "--disk" => {
                return Err(CliError::usage(
                    "--disk is not supported: neon::session::Titan does not expose disk load/save",
                ));
            }
            other => {
                return Err(CliError::usage(format!("unexpected argument '{other}'")));
            }
        }
    }

    Ok(Action::Boot { kernel, script })
}

fn print_help() -> Result<(), String> {
    let text = "\
Usage: titan <subcommand> [options]

Subcommands:
  boot [--kernel PATH] [--script FILE]
      Boot the Titan kernel on Silicon through neon::session::Titan.
      Interactive by default (stdin lines -> shell, output -> stdout).
      With --script FILE (or - for stdin), feed lines and exit.

Options:
  --kernel PATH   Kernel ELF path
                  (default: target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel)
  --script FILE   Read shell commands from FILE (or - for stdin) then exit
  -h, --help      Print this help text and exit
  -V, --version   Print version and exit

Example:
  titan boot --script commands.txt
";
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())
        .map_err(|e| format!("stdout write failed: {e}"))?;
    out.flush().map_err(|e| format!("stdout flush failed: {e}"))
}

fn print_version() -> Result<(), String> {
    let mut out = io::stdout().lock();
    out.write_all(b"titan 0.1.0\n")
        .map_err(|e| format!("stdout write failed: {e}"))?;
    out.flush().map_err(|e| format!("stdout flush failed: {e}"))
}

fn run_boot(kernel_path: &Path, script_path: Option<&str>) -> Result<(), CliError> {
    let script_text = match script_path {
        Some("-") => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| CliError::failed(format!("cannot read script from stdin: {e}")))?;
            Some(buf)
        }
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| CliError::failed(format!("cannot read script '{path}': {e}")))?;
            Some(text)
        }
        None => None,
    };

    let elf_bytes = fs::read(kernel_path).map_err(|e| {
        CliError::failed(format!(
            "cannot read kernel '{}': {e} (build with: {BUILD_CMD})",
            kernel_path.display()
        ))
    })?;

    let mut titan = Titan::boot(&elf_bytes).map_err(CliError::failed)?;
    let mut stdout = io::stdout().lock();

    match pump_until_ready(&mut titan, &mut stdout).map_err(CliError::failed)? {
        StopReason::Prompt => {}
        StopReason::Exited(code) => return finish_guest_exit(code),
    }

    if let Some(text) = script_text {
        for line in text.lines() {
            titan.send(line);
            match pump_until_ready(&mut titan, &mut stdout).map_err(CliError::failed)? {
                StopReason::Prompt => {}
                StopReason::Exited(code) => return finish_guest_exit(code),
            }
        }
        return Ok(());
    }

    let stdin = io::stdin();
    for line_res in stdin.lock().lines() {
        let line = line_res.map_err(|e| CliError::failed(format!("stdin read failed: {e}")))?;
        titan.send(&line);
        match pump_until_ready(&mut titan, &mut stdout).map_err(CliError::failed)? {
            StopReason::Prompt => {}
            StopReason::Exited(code) => return finish_guest_exit(code),
        }
    }

    Ok(())
}

fn pump_until_ready(titan: &mut Titan, out: &mut impl Write) -> Result<StopReason, String> {
    let mut steps_run: u64 = 0;
    loop {
        let pump = titan.pump(CHUNK_STEPS);
        let bytes = titan.take_output();
        if !bytes.is_empty() {
            out.write_all(&bytes)
                .map_err(|e| format!("stdout write failed: {e}"))?;
            out.flush()
                .map_err(|e| format!("stdout flush failed: {e}"))?;
        }
        match pump {
            Pump::WaitingForInput => return Ok(StopReason::Prompt),
            Pump::Exited(code) => return Ok(StopReason::Exited(code)),
            Pump::Busy => {
                steps_run = steps_run.saturating_add(CHUNK_STEPS);
                if steps_run >= MAX_WAIT_STEPS {
                    return Err(String::from("guest timed out before reaching prompt"));
                }
            }
        }
    }
}

fn finish_guest_exit(code: i32) -> Result<(), CliError> {
    if code == 0 {
        Ok(())
    } else {
        Err(CliError::failed(format!("guest exited with status {code}")))
    }
}
