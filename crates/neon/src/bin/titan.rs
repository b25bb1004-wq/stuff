//! Command-line runner for booting the Titan kernel on Silicon.

#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use neon::session::{default_disk, Pump, Titan};

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
        disk: Option<PathBuf>,
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
        Action::Boot {
            kernel,
            script,
            disk,
        } => run_boot(&kernel, script.as_deref(), disk.as_deref()),
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
    let mut disk: Option<PathBuf> = None;

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
                let val = iter
                    .next()
                    .ok_or_else(|| CliError::usage("missing path after --disk"))?;
                disk = Some(PathBuf::from(val));
            }
            other => {
                return Err(CliError::usage(format!("unexpected argument '{other}'")));
            }
        }
    }

    Ok(Action::Boot {
        kernel,
        script,
        disk,
    })
}

fn print_help() -> Result<(), String> {
    let text = "\
Usage: titan <subcommand> [options]

Subcommands:
  boot [--kernel PATH] [--script FILE] [--disk IMG]
      Boot the Titan kernel on Silicon through neon::session::Titan.
      Interactive by default (stdin lines -> shell, output -> stdout).
      With --script FILE (or - for stdin), feed lines and exit.

Options:
  --kernel PATH   Kernel ELF path
                  (default: target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel)
  --script FILE   Read shell commands from FILE (or - for stdin) then exit
  --disk IMG      Load Hades disk image IMG (or create if missing) and save on exit
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

fn run_boot(
    kernel_path: &Path,
    script_path: Option<&str>,
    disk_path: Option<&Path>,
) -> Result<(), CliError> {
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

    let mut titan = match disk_path {
        Some(path) => {
            let disk_bytes = load_or_default_disk(path).map_err(CliError::failed)?;
            Titan::boot_with_disk(&elf_bytes, disk_bytes).map_err(CliError::failed)?
        }
        None => Titan::boot(&elf_bytes).map_err(CliError::failed)?,
    };

    let exit_code = run_session(&mut titan, script_text.as_deref())?;
    if let Some(path) = disk_path {
        write_disk_atomic(path, &titan.disk_bytes()).map_err(CliError::failed)?;
    }

    match exit_code {
        Some(code) => finish_guest_exit(code),
        None => Ok(()),
    }
}

fn run_session(titan: &mut Titan, script_text: Option<&str>) -> Result<Option<i32>, CliError> {
    let mut stdout = io::stdout().lock();

    match pump_until_ready(titan, &mut stdout).map_err(CliError::failed)? {
        StopReason::Prompt => {}
        StopReason::Exited(code) => return Ok(Some(code)),
    }

    if let Some(text) = script_text {
        for line in text.lines() {
            titan.send(line);
            match pump_until_ready(titan, &mut stdout).map_err(CliError::failed)? {
                StopReason::Prompt => {}
                StopReason::Exited(code) => return Ok(Some(code)),
            }
        }
        return Ok(None);
    }

    let stdin = io::stdin();
    for line_res in stdin.lock().lines() {
        let line = line_res.map_err(|e| CliError::failed(format!("stdin read failed: {e}")))?;
        titan.send(&line);
        match pump_until_ready(titan, &mut stdout).map_err(CliError::failed)? {
            StopReason::Prompt => {}
            StopReason::Exited(code) => return Ok(Some(code)),
        }
    }

    Ok(None)
}

fn load_or_default_disk(path: &Path) -> Result<Vec<u8>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == io::ErrorKind::NotFound => default_disk(),
        Err(err) => Err(format!("cannot read disk '{}': {err}", path.display())),
    }
}

fn temp_disk_path(disk_path: &Path) -> PathBuf {
    let parent = match disk_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let file_name = disk_path.file_name().unwrap_or_else(|| OsStr::new("disk"));
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut tmp_name = OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".tmp.{}.{nanos}", std::process::id()));
    parent.join(tmp_name)
}

fn write_disk_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp_path = temp_disk_path(path);
    let write_res = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!(
            "cannot write temporary disk '{}': {err}",
            tmp_path.display()
        ));
    }
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("cannot replace disk '{}': {err}", path.display()));
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
