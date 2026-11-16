//! Host CLI for booting and inspecting guests under the zeus hypervisor on Silicon.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(not(target_os = "none"))]
mod host {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::time::Instant;

    use aci::envelope::{self, Code};
    use aci::json::Json;
    use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
    use silicon::machine::{Exit, RAM_BASE};
    use zeus::{build_machine, split_console, Guest, Image};

    const RUN_ARGS: &[Arg] = &[
        Arg {
            name: "--zeus",
            kind: ArgKind::Path,
            required: false,
            default: Some("target/zeus-bin/riscv64gc-unknown-none-elf/release/zeus"),
            choices: &[],
        },
        Arg {
            name: "--guest",
            kind: ArgKind::Text,
            required: false,
            default: Some("lithium:4"),
            choices: &[],
        },
        Arg {
            name: "--titan",
            kind: ArgKind::Path,
            required: false,
            default: Some("target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel"),
            choices: &[],
        },
        Arg {
            name: "--steps",
            kind: ArgKind::Int,
            required: false,
            default: Some("20000000"),
            choices: &[],
        },
    ];

    const COMMANDS: &[Command] = &[Command {
        name: "run",
        summary: "Boot zeus with one or more guests (lithium, titan; up to 3) and report their console output",
        effect: Effect::Read,
        args: RUN_ARGS,
        returns: "exit (only when stop is exit), stdout (console lines as [source] line, capped, with stdout_truncated, stdout_bytes, stdout_lossy when they apply), steps, duration_ms, stop: exit, step_limit",
        example: "zeusctl --json run --guest lithium:4 --steps 80000000",
    }];

    const MANIFEST: Manifest = Manifest {
        tool: "zeusctl",
        version: env!("CARGO_PKG_VERSION"),
        summary: "Host CLI for booting guests under the zeus hypervisor",
        commands: COMMANDS,
    };

    const MIB: u64 = 1024 * 1024;
    const DEFAULT_STEPS: u64 = 20_000_000;
    const DEFAULT_ZEUS_PATH: &str = "target/zeus-bin/riscv64gc-unknown-none-elf/release/zeus";
    const DEFAULT_TITAN_PATH: &str =
        "target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel";
    const ZEUS_BUILD_CMD: &str =
        "CARGO_TARGET_DIR=target/zeus-bin cargo build -p zeus --bin zeus --release --target riscv64gc-unknown-none-elf";
    const TITAN_BUILD_CMD: &str =
        "CARGO_TARGET_DIR=target/titan-kernel cargo build -p titan --bin kernel --release --target riscv64gc-unknown-none-elf";
    const VERSION: &str = "zeusctl 0.1.0";

    const HELP: &str = "\
zeusctl - Boot and inspect guests under the zeus hypervisor on Silicon

USAGE:
    zeusctl <COMMAND> [OPTIONS]
    zeusctl --help
    zeusctl --version

COMMANDS:
    run     Boot zeus with one or more guests and print per-source console output

OPTIONS (run):
    --zeus PATH         Path to zeus RV64 ELF
                        [default: target/zeus-bin/riscv64gc-unknown-none-elf/release/zeus]
    --guest NAME[:MiB]  Guest to boot: lithium (default 4 MiB) or titan (default 16 MiB);
                        repeatable up to 3 guests, even MiB in 2..=16
                        [default: lithium:4]
    --titan PATH        Path to titan RV64 kernel ELF
                        [default: target/titan-kernel/riscv64gc-unknown-none-elf/release/kernel]
    --steps N           Maximum Silicon steps to execute [default: 20000000]
    -h, --help          Print help

EXAMPLES:
    zeusctl run
    zeusctl run --guest lithium:4 --guest titan:16 --steps 80000000";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum GuestKind {
        Lithium,
        Titan,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct GuestSpec {
        kind: GuestKind,
        size_mib: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RunArgs {
        zeus_path: PathBuf,
        titan_path: PathBuf,
        guests: Vec<GuestSpec>,
        steps: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Action {
        Help,
        Version,
        Run(RunArgs),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
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

        fn fail(message: impl Into<String>) -> Self {
            Self {
                code: 1,
                message: message.into(),
                kind: Code::Invalid,
            }
        }

        fn io(message: impl Into<String>, err: &std::io::Error) -> Self {
            let kind = if err.kind() == std::io::ErrorKind::NotFound {
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

    pub fn main() -> ExitCode {
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
        match run_cli(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("zeusctl: {}", err.message);
                ExitCode::from(err.code)
            }
        }
    }

    /// The subcommand name for the envelope, or the empty string when the name is not one of ours.
    fn known_command(first: Option<&String>) -> &'static str {
        const NAMES: [&str; 4] = ["run", "--help", "--version", "-h"];
        first
            .and_then(|s| NAMES.iter().copied().find(|n| *n == s.as_str()))
            .unwrap_or("")
    }

    /// `zeusctl --json ...`: one envelope on stdout; stderr keeps the `zeusctl: message` line.
    fn run_json(args: &[String]) -> ExitCode {
        let cmd = known_command(args.first());
        match json_cli(args) {
            Ok((data, exit)) => {
                print!("{}", envelope::ok("zeusctl", cmd, data));
                ExitCode::from(exit)
            }
            Err(err) => {
                eprintln!("zeusctl: {}", err.message);
                let hint = match err.kind {
                    Code::Usage => Some("run: zeusctl --describe"),
                    _ => None,
                };
                print!(
                    "{}",
                    envelope::err("zeusctl", cmd, err.kind, &err.message, hint)
                );
                ExitCode::from(err.code)
            }
        }
    }

    /// Run one command under `--json`. Returns the data and the exit code the text path gives.
    fn json_cli(args: &[String]) -> Result<(Json, u8), CliError> {
        match parse_args(args)? {
            Action::Help => Ok((Json::obj([("usage", Json::Str(HELP.into()))]), 0)),
            Action::Version => Ok((Json::obj([("version", Json::Str(VERSION.into()))]), 0)),
            Action::Run(run_args) => {
                let booted = boot(&run_args)?;
                // A guest run that did not pass is ok: true with the text path's exit code 1.
                let (stop, guest_exit, exit) = match booted.exit {
                    Exit::Pass => ("exit", Some(0), 0),
                    Exit::Fail(code) => ("exit", Some(i64::from(code)), 1),
                    Exit::OutOfSteps => ("step_limit", None, 0),
                };
                let mut console = String::new();
                for (source, lines) in split_console(&booted.console) {
                    for line in lines {
                        console.push_str(&format!("[{source}] {line}\n"));
                    }
                }
                let mut pairs = Vec::new();
                if let Some(code) = guest_exit {
                    pairs.push(("exit".to_string(), Json::Int(code)));
                }
                pairs.extend(stream("stdout", console.as_bytes()));
                pairs.push(("steps".to_string(), int(booted.steps)));
                pairs.push(("duration_ms".to_string(), Json::Int(booted.duration_ms)));
                pairs.push(("stop".to_string(), Json::Str(stop.into())));
                Ok((Json::Obj(pairs), exit))
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

    fn run_cli(args: &[String]) -> Result<(), CliError> {
        match parse_args(args)? {
            Action::Help => {
                println!("{HELP}");
                Ok(())
            }
            Action::Version => {
                println!("{VERSION}");
                Ok(())
            }
            Action::Run(run_args) => execute_run(&run_args),
        }
    }

    fn parse_args(args: &[String]) -> Result<Action, CliError> {
        let Some(first) = args.first() else {
            return Err(CliError::usage("missing subcommand (try --help)"));
        };

        match first.as_str() {
            "--help" | "-h" => return Ok(Action::Help),
            "--version" | "-V" => return Ok(Action::Version),
            "run" => {}
            other if other.starts_with('-') => {
                return Err(CliError::usage(format!("unknown option '{other}'")));
            }
            other => {
                return Err(CliError::usage(format!("unknown subcommand '{other}'")));
            }
        }

        let mut zeus_path = PathBuf::from(DEFAULT_ZEUS_PATH);
        let mut titan_path = PathBuf::from(DEFAULT_TITAN_PATH);
        let mut guests = Vec::new();
        let mut steps = DEFAULT_STEPS;

        let mut i = 1;
        while let Some(arg) = args.get(i) {
            match arg.as_str() {
                "--help" | "-h" => return Ok(Action::Help),
                "--version" | "-V" => return Ok(Action::Version),
                "--zeus" => {
                    let val = take_value(args, &mut i, "--zeus")?;
                    zeus_path = parse_nonempty_path("--zeus", val)?;
                }
                "--titan" => {
                    let val = take_value(args, &mut i, "--titan")?;
                    titan_path = parse_nonempty_path("--titan", val)?;
                }
                "--guest" => {
                    let val = take_value(args, &mut i, "--guest")?;
                    guests.push(parse_guest_spec(val)?);
                }
                "--steps" => {
                    let val = take_value(args, &mut i, "--steps")?;
                    steps = parse_steps(val)?;
                }
                other => {
                    if let Some(val) = other.strip_prefix("--zeus=") {
                        zeus_path = parse_nonempty_path("--zeus", val)?;
                    } else if let Some(val) = other.strip_prefix("--titan=") {
                        titan_path = parse_nonempty_path("--titan", val)?;
                    } else if let Some(val) = other.strip_prefix("--guest=") {
                        guests.push(parse_guest_spec(val)?);
                    } else if let Some(val) = other.strip_prefix("--steps=") {
                        steps = parse_steps(val)?;
                    } else if other.starts_with('-') {
                        return Err(CliError::usage(format!("unknown option '{other}'")));
                    } else {
                        return Err(CliError::usage(format!("unexpected argument '{other}'")));
                    }
                }
            }
            i += 1;
        }

        if guests.is_empty() {
            guests.push(GuestSpec {
                kind: GuestKind::Lithium,
                size_mib: 4,
            });
        }
        if guests.len() > 3 {
            return Err(CliError::usage(format!(
                "at most 3 guests are allowed, got {}",
                guests.len()
            )));
        }

        Ok(Action::Run(RunArgs {
            zeus_path,
            titan_path,
            guests,
            steps,
        }))
    }

    fn take_value<'a>(
        args: &'a [String],
        index: &mut usize,
        flag: &str,
    ) -> Result<&'a str, CliError> {
        let next = *index + 1;
        let Some(val) = args.get(next) else {
            return Err(CliError::usage(format!("option '{flag}' requires a value")));
        };
        *index = next;
        Ok(val.as_str())
    }

    fn parse_nonempty_path(flag: &str, val: &str) -> Result<PathBuf, CliError> {
        if val.is_empty() {
            return Err(CliError::usage(format!("{flag} path must not be empty")));
        }
        Ok(PathBuf::from(val))
    }

    fn parse_steps(val: &str) -> Result<u64, CliError> {
        let parsed = val
            .parse::<u64>()
            .map_err(|_| CliError::usage(format!("invalid --steps value '{val}'")))?;
        if parsed == 0 {
            return Err(CliError::usage("--steps must be greater than 0"));
        }
        Ok(parsed)
    }

    fn parse_guest_spec(spec: &str) -> Result<GuestSpec, CliError> {
        let (name, size_opt) = match spec.split_once(':') {
            Some((n, s)) => (n, Some(s)),
            None => (spec, None),
        };

        let (kind, default_mib) = match name {
            "lithium" => (GuestKind::Lithium, 4),
            "titan" => (GuestKind::Titan, 16),
            _ => {
                return Err(CliError::usage(format!(
                    "unknown guest '{name}' (expected 'lithium' or 'titan')"
                )));
            }
        };

        let size_mib = match size_opt {
            Some(s) => {
                let mib = s.parse::<u64>().map_err(|_| {
                    CliError::usage(format!(
                        "invalid guest size '{s}' for '{name}' (expected even MiB in 2..=16)"
                    ))
                })?;
                if mib == 0 || mib > 16 || mib % 2 != 0 {
                    return Err(CliError::usage(format!(
                        "invalid guest size '{mib}' MiB for '{name}' (expected even MiB in 2..=16)"
                    )));
                }
                mib
            }
            None => default_mib,
        };

        Ok(GuestSpec { kind, size_mib })
    }

    /// One boot: how it stopped, the console bytes, and the steps and time it used.
    struct Booted {
        exit: Exit,
        console: Vec<u8>,
        steps: u64,
        duration_ms: i64,
    }

    fn execute_run(args: &RunArgs) -> Result<(), CliError> {
        let booted = boot(args)?;
        for (source, lines) in split_console(&booted.console) {
            for line in lines {
                println!("[{source}] {line}");
            }
        }
        println!("exit: {:?}", booted.exit);
        println!("steps: {}", booted.steps);

        if booted.exit == Exit::Pass {
            Ok(())
        } else {
            Err(CliError::fail(format!(
                "machine exited with {:?}",
                booted.exit
            )))
        }
    }

    fn boot(args: &RunArgs) -> Result<Booted, CliError> {
        let zeus_elf = std::fs::read(&args.zeus_path).map_err(|err| {
            CliError::io(
                format!(
                    "missing or unreadable zeus ELF at {}: {err}; build it with: {ZEUS_BUILD_CMD}",
                    args.zeus_path.display()
                ),
                &err,
            )
        })?;

        let needs_titan = args.guests.iter().any(|g| g.kind == GuestKind::Titan);
        let titan_elf = if needs_titan {
            Some(std::fs::read(&args.titan_path).map_err(|err| {
                CliError::io(
                    format!(
                        "missing or unreadable titan kernel at {}: {err}; build it with: {TITAN_BUILD_CMD}",
                        args.titan_path.display()
                    ),
                    &err,
                )
            })?)
        } else {
            None
        };

        let needs_lithium = args.guests.iter().any(|g| g.kind == GuestKind::Lithium);
        let lithium_bytes =
            if needs_lithium {
                Some(lithium::image().map_err(|err| {
                    CliError::fail(format!("failed to build lithium image: {err:?}"))
                })?)
            } else {
                None
            };

        let mut built_guests = Vec::with_capacity(args.guests.len());
        for spec in &args.guests {
            let size = spec
                .size_mib
                .checked_mul(MIB)
                .ok_or_else(|| CliError::usage("guest size overflow"))?;
            let guest = match spec.kind {
                GuestKind::Lithium => {
                    let bytes = lithium_bytes
                        .as_ref()
                        .ok_or_else(|| CliError::fail("lithium image unavailable"))?
                        .clone();
                    Guest {
                        name: "lithium".to_owned(),
                        image: Image::Raw {
                            bytes,
                            gpa: RAM_BASE,
                        },
                        size,
                        a1: 0,
                    }
                }
                GuestKind::Titan => {
                    let elf = titan_elf
                        .as_ref()
                        .ok_or_else(|| CliError::fail("titan kernel unavailable"))?
                        .clone();
                    Guest {
                        name: "titan".to_owned(),
                        image: Image::Elf(elf),
                        size,
                        a1: size,
                    }
                }
            };
            built_guests.push(guest);
        }

        let mut machine = build_machine(&zeus_elf, &built_guests).map_err(CliError::fail)?;
        let started = Instant::now();
        let exit = machine.run(args.steps);
        let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let steps = machine.board.mtime();
        let console = machine.board.take_uart_output();
        Ok(Booted {
            exit,
            console,
            steps,
            duration_ms,
        })
    }

    #[cfg(test)]
    mod tests {
        #![allow(clippy::unwrap_used)]

        use super::*;

        fn s(items: &[&str]) -> Vec<String> {
            items.iter().map(|x| (*x).to_owned()).collect()
        }

        #[test]
        fn parse_defaults() {
            let action = parse_args(&s(&["run"])).unwrap();
            assert_eq!(
                action,
                Action::Run(RunArgs {
                    zeus_path: PathBuf::from(DEFAULT_ZEUS_PATH),
                    titan_path: PathBuf::from(DEFAULT_TITAN_PATH),
                    guests: vec![GuestSpec {
                        kind: GuestKind::Lithium,
                        size_mib: 4,
                    }],
                    steps: DEFAULT_STEPS,
                })
            );
        }

        #[test]
        fn parse_multi_guests_and_flags() {
            let action = parse_args(&s(&[
                "run",
                "--zeus",
                "/tmp/z",
                "--titan=/tmp/t",
                "--guest",
                "lithium:2",
                "--guest=titan",
                "--steps",
                "5000",
            ]))
            .unwrap();
            assert_eq!(
                action,
                Action::Run(RunArgs {
                    zeus_path: PathBuf::from("/tmp/z"),
                    titan_path: PathBuf::from("/tmp/t"),
                    guests: vec![
                        GuestSpec {
                            kind: GuestKind::Lithium,
                            size_mib: 2,
                        },
                        GuestSpec {
                            kind: GuestKind::Titan,
                            size_mib: 16,
                        },
                    ],
                    steps: 5000,
                })
            );
        }

        #[test]
        fn reject_bad_guest_specs() {
            for bad in [
                "foo",
                "lithium:",
                "lithium:0",
                "lithium:1",
                "lithium:3",
                "lithium:18",
                "titan:abc",
                "titan:-4",
            ] {
                let err = parse_args(&s(&["run", "--guest", bad])).unwrap_err();
                assert_eq!(err.code, 2, "spec {bad} should exit 2");
            }
        }

        #[test]
        fn reject_more_than_three_guests() {
            let err = parse_args(&s(&[
                "run", "--guest", "lithium", "--guest", "lithium", "--guest", "lithium", "--guest",
                "titan",
            ]))
            .unwrap_err();
            assert_eq!(err.code, 2);
        }
    }
}

#[cfg(not(target_os = "none"))]
fn main() -> std::process::ExitCode {
    host::main()
}
