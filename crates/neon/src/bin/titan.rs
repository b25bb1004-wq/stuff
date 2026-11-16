//! Command-line runner for booting the Titan kernel on Silicon.
//!
//! Text mode is unchanged. `--describe` prints the manifest and `--json` prints one ACI envelope
//! (docs/ai-computer/ACI.md). `--json` runs only scripted boots; the interactive console is `unsupported`.

#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use neon::session::{default_disk, Pump, Titan};

const DEFAULT_KERNEL: &str = "target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel";
/// Relative to the folder of the running exe: where `tools/install.sh` and `install.ps1` put the kernel.
const INSTALLED_KERNEL: &str = "../share/titan/kernel";
const KERNEL_DEFAULT_TEXT: &str =
    "target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel, else <exe folder>/../share/titan/kernel";
const BUILD_CMD: &str =
    "cargo build --target-dir target/titan-kernel -p titan --bin kernel --release --target riscv64gc-unknown-none-elf";
const CHUNK_STEPS: u64 = 500_000;
const MAX_WAIT_STEPS: u64 = 100_000_000;

const BOOT_ARGS: &[Arg] = &[
    Arg {
        name: "--kernel",
        kind: ArgKind::Path,
        required: false,
        default: Some(KERNEL_DEFAULT_TEXT),
        choices: &[],
    },
    Arg {
        name: "--script",
        kind: ArgKind::Path,
        required: false,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--disk",
        kind: ArgKind::Path,
        required: false,
        default: None,
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[Command {
    name: "boot",
    summary: "Boot the Titan kernel on Silicon and run the shell commands in --script FILE (or - for stdin); --json needs --script",
    effect: Effect::Write,
    args: BOOT_ARGS,
    returns: "stdout (the console transcript, capped, with stdout_truncated, stdout_bytes, stdout_lossy when they apply), duration_ms, stop: exit (exit only when stop is exit), halt (script ended with the shell at its prompt)",
    example: "titan --json boot --script commands.txt",
}];

const MANIFEST: Manifest = Manifest {
    tool: "titan",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Titan OS boot tool: runs the Titan kernel on Silicon",
    commands: COMMANDS,
};

const HELP: &str = "\
Usage: titan <subcommand> [options]

Subcommands:
  boot [--kernel PATH] [--script FILE] [--disk IMG]
      Boot the Titan kernel on Silicon through neon::session::Titan.
      Interactive by default (stdin lines -> shell, output -> stdout).
      With --script FILE (or - for stdin), feed lines and exit.

Options:
  --kernel PATH   Kernel ELF path
                  (default: target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel,
                  else <exe folder>/../share/titan/kernel)
  --script FILE   Read shell commands from FILE (or - for stdin) then exit
  --disk IMG      Load Hades disk image IMG (or create if missing) and save on exit
  -h, --help      Print this help text and exit
  -V, --version   Print version and exit

Example:
  titan boot --script commands.txt
";

enum Action {
    Help,
    Version,
    Boot {
        /// None when `--kernel` was not given: the default is looked up at boot time.
        kernel: Option<PathBuf>,
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
    /// The error code under `--json`.
    kind: Code,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
            kind: Code::Usage,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
            kind: Code::Invalid,
        }
    }

    fn io(message: impl Into<String>, err: &io::Error) -> Self {
        let kind = if err.kind() == io::ErrorKind::NotFound {
            Code::NotFound
        } else {
            Code::Io
        };
        Self {
            code: 1,
            message: message.into(),
            kind,
        }
    }
}

fn main() -> ExitCode {
    let mut raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    if json {
        return run_json(&raw);
    }
    match run(raw.into_iter()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "titan: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(first: Option<&OsString>) -> &'static str {
    const NAMES: [&str; 5] = ["boot", "--help", "-h", "--version", "-V"];
    first
        .and_then(|a| a.to_str())
        .and_then(|s| NAMES.iter().copied().find(|n| *n == s))
        .unwrap_or("")
}

/// `titan --json ...`: one envelope on stdout; stderr keeps the `titan: message` line.
fn run_json(args: &[OsString]) -> ExitCode {
    let cmd = known_command(args.first());
    match json_run(args.to_vec()) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("titan", cmd, data));
            ExitCode::from(exit)
        }
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "titan: {}", err.message);
            let hint = match err.kind {
                Code::Usage => Some("run: titan --describe"),
                Code::Unsupported => Some("run: titan --json boot --script FILE"),
                _ => None,
            };
            print!(
                "{}",
                envelope::err("titan", cmd, err.kind, &err.message, hint)
            );
            ExitCode::from(err.code)
        }
    }
}

/// Run one command under `--json`. Returns the data and the exit code the text path gives.
fn json_run(args: Vec<OsString>) -> Result<(Json, u8), CliError> {
    match parse_args(args.into_iter())? {
        Action::Help => Ok((Json::obj([("usage", Json::Str(HELP.into()))]), 0)),
        Action::Version => Ok((Json::obj([("version", Json::Str("titan 0.1.0".into()))]), 0)),
        Action::Boot {
            kernel,
            script,
            disk,
        } => {
            let Some(script) = script else {
                return Err(CliError {
                    code: 1,
                    message: String::from("the interactive console is not available under --json"),
                    kind: Code::Unsupported,
                });
            };
            let script_text = read_script(Some(script.as_str()))?;
            let mut console = Vec::new();
            let started = Instant::now();
            let exit = boot_and_run(
                kernel.as_deref(),
                script_text.as_deref(),
                disk.as_deref(),
                &mut console,
            )?;
            let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            // A guest that exits non-zero is ok: true with the text path's exit code 1.
            let (stop, guest_exit, exit_code) = match exit {
                Some(code) => ("exit", Some(i64::from(code)), if code == 0 { 0 } else { 1 }),
                None => ("halt", None, 0),
            };
            let mut pairs = Vec::new();
            if let Some(code) = guest_exit {
                pairs.push(("exit".to_string(), Json::Int(code)));
            }
            pairs.extend(stream("stdout", &console));
            pairs.push(("duration_ms".to_string(), Json::Int(duration_ms)));
            pairs.push(("stop".to_string(), Json::Str(stop.into())));
            Ok((Json::Obj(pairs), exit_code))
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

fn run(args: impl Iterator<Item = OsString>) -> Result<(), CliError> {
    match parse_args(args)? {
        Action::Help => print_help().map_err(CliError::failed),
        Action::Version => print_version().map_err(CliError::failed),
        Action::Boot {
            kernel,
            script,
            disk,
        } => run_boot(kernel.as_deref(), script.as_deref(), disk.as_deref()),
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
    let mut kernel: Option<PathBuf> = None;
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
                kernel = Some(PathBuf::from(val));
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
    let mut out = io::stdout().lock();
    out.write_all(HELP.as_bytes())
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
    kernel_path: Option<&Path>,
    script_path: Option<&str>,
    disk_path: Option<&Path>,
) -> Result<(), CliError> {
    let script_text = read_script(script_path)?;
    let mut stdout = io::stdout().lock();
    let exit_code = boot_and_run(kernel_path, script_text.as_deref(), disk_path, &mut stdout)?;
    match exit_code {
        Some(code) => finish_guest_exit(code),
        None => Ok(()),
    }
}

fn read_script(script_path: Option<&str>) -> Result<Option<String>, CliError> {
    match script_path {
        Some("-") => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| CliError::io(format!("cannot read script from stdin: {e}"), &e))?;
            Ok(Some(buf))
        }
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| CliError::io(format!("cannot read script '{path}': {e}"), &e))?;
            Ok(Some(text))
        }
        None => Ok(None),
    }
}

/// The installed kernel: `../share/titan/kernel` beside the running exe's folder.
fn installed_kernel() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(INSTALLED_KERNEL))
}

/// The kernel bytes: the given `--kernel` path, else the repo build, else the installed copy.
fn read_kernel(kernel_path: Option<&Path>) -> Result<Vec<u8>, CliError> {
    if let Some(path) = kernel_path {
        return fs::read(path).map_err(|e| {
            CliError::io(
                format!(
                    "cannot read kernel '{}': {e} (build with: {BUILD_CMD})",
                    path.display()
                ),
                &e,
            )
        });
    }
    let repo = PathBuf::from(DEFAULT_KERNEL);
    let installed = installed_kernel();
    let path = if repo.exists() {
        repo.clone()
    } else {
        match &installed {
            Some(p) if p.exists() => p.clone(),
            _ => repo.clone(),
        }
    };
    fs::read(&path).map_err(|e| {
        let looked = match &installed {
            Some(p) => format!("'{}' or '{}'", repo.display(), p.display()),
            None => format!("'{}'", repo.display()),
        };
        CliError::io(
            format!("cannot read kernel (looked in {looked}): {e} (build with: {BUILD_CMD})"),
            &e,
        )
    })
}

/// Boot the kernel, run the session with its console going to `out`, and save the disk if asked.
/// Returns the guest's exit code when it exited, None when the session ended at the prompt.
fn boot_and_run(
    kernel_path: Option<&Path>,
    script_text: Option<&str>,
    disk_path: Option<&Path>,
    out: &mut impl Write,
) -> Result<Option<i32>, CliError> {
    let elf_bytes = read_kernel(kernel_path)?;

    let mut titan = match disk_path {
        Some(path) => {
            let disk_bytes = load_or_default_disk(path).map_err(CliError::failed)?;
            Titan::boot_with_disk(&elf_bytes, disk_bytes).map_err(CliError::failed)?
        }
        None => Titan::boot(&elf_bytes).map_err(CliError::failed)?,
    };

    let exit_code = run_session(&mut titan, script_text, out)?;
    if let Some(path) = disk_path {
        write_disk_atomic(path, &titan.disk_bytes()).map_err(CliError::failed)?;
    }
    Ok(exit_code)
}

fn run_session(
    titan: &mut Titan,
    script_text: Option<&str>,
    stdout: &mut impl Write,
) -> Result<Option<i32>, CliError> {
    match pump_until_ready(titan, stdout).map_err(CliError::failed)? {
        StopReason::Prompt => {}
        StopReason::Exited(code) => return Ok(Some(code)),
    }

    if let Some(text) = script_text {
        for line in text.lines() {
            titan.send(line);
            match pump_until_ready(titan, stdout).map_err(CliError::failed)? {
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
        match pump_until_ready(titan, stdout).map_err(CliError::failed)? {
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
