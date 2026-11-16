//! `krypton --json` and `krypton --describe`: one ACI envelope per run (docs/ai-computer/ACI.md).
//!
//! The script runs through the same shell calls as the text path (`lf_only`, `set_positional`), with
//! stdout and stderr captured into buffers instead of the process streams.

use std::io;
use std::time::Instant;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use krypton::exec::Shell;

use crate::{lf_only, set_positional};

const SCRIPT_ARGS: &[Arg] = &[
    Arg {
        name: "SCRIPT",
        kind: Kind::Text,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "ARG0",
        kind: Kind::Text,
        required: false,
        default: None,
        choices: &[],
    },
    Arg {
        name: "ARGS",
        kind: Kind::Text,
        required: false,
        default: None,
        choices: &[],
    },
];

const FILE_ARGS: &[Arg] = &[
    Arg {
        name: "FILE",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "ARGS",
        kind: Kind::Text,
        required: false,
        default: None,
        choices: &[],
    },
];

const RUN_RETURNS: &str = "exit, stdout, stderr (capped), duration_ms; long output adds stdout_truncated and stdout_bytes, non-UTF-8 output adds stdout_lossy (same names for stderr)";

const COMMANDS: &[Command] = &[
    Command {
        name: "-c",
        summary: "Run SCRIPT text with optional $0 and positional arguments",
        effect: Effect::Exec,
        args: SCRIPT_ARGS,
        returns: RUN_RETURNS,
        example: "krypton --json -c 'echo hello'",
    },
    Command {
        name: "script",
        summary: "run a script file: krypton --json PATH [ARGS] (no subcommand word)",
        effect: Effect::Exec,
        args: FILE_ARGS,
        returns: RUN_RETURNS,
        example: "krypton --json script.sh arg1",
    },
    Command {
        name: "interactive",
        summary: "Interactive shell; refused under --json, use -c",
        effect: Effect::Exec,
        args: &[],
        returns: "nothing: always code unsupported",
        example: "krypton --json",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "krypton",
    version: env!("CARGO_PKG_VERSION"),
    summary: "POSIX shell for Monolith",
    commands: COMMANDS,
};

/// The manifest text `krypton --describe` prints, without the newline.
pub fn describe() -> String {
    MANIFEST.to_json().write()
}

/// What a script produced.
struct Captured {
    exit: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    duration_ms: i64,
}

/// A run krypton refused or could not start: the error envelope and the process exit code.
struct Refused {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: u8,
}

fn refuse(code: Code, message: String, hint: Option<&str>, exit: u8) -> Refused {
    Refused {
        code,
        message,
        hint: hint.map(str::to_string),
        exit,
    }
}

/// Run `krypton --json ARGS...`, where `args` are the arguments after `--json`. Prints one envelope.
pub fn run(args: &[String]) -> std::process::ExitCode {
    let (cmd, result) = dispatch(args);
    match result {
        Ok(c) => {
            let line = envelope::ok("krypton", cmd, data(&c));
            print!("{line}");
            std::process::ExitCode::from((c.exit & 0xff) as u8)
        }
        Err(r) => {
            let line = envelope::err("krypton", cmd, r.code, &r.message, r.hint.as_deref());
            print!("{line}");
            std::process::ExitCode::from(r.exit)
        }
    }
}

fn dispatch(args: &[String]) -> (&'static str, Result<Captured, Refused>) {
    let interactive = || {
        refuse(
            Code::Unsupported,
            "the interactive shell is not available under --json".into(),
            Some("run: krypton --json -c SCRIPT"),
            1,
        )
    };
    match args.first().map(String::as_str) {
        None => ("interactive", Err(interactive())),
        Some("-c") => match args.get(1) {
            None => (
                "-c",
                Err(refuse(
                    Code::Usage,
                    "-c: option requires an argument".into(),
                    Some("run: krypton --describe"),
                    2,
                )),
            ),
            Some(script) => (
                "-c",
                run_text(
                    script,
                    args.get(2).map(String::as_str),
                    args.get(3..).unwrap_or(&[]),
                ),
            ),
        },
        Some("--") => match args.get(1) {
            None => ("interactive", Err(interactive())),
            Some(file) => ("script", run_file(file, args.get(2..).unwrap_or(&[]))),
        },
        Some(opt) if opt.starts_with('-') => (
            "",
            Err(refuse(
                Code::Usage,
                format!("unknown option: {opt}"),
                Some("run: krypton --describe"),
                2,
            )),
        ),
        Some(file) => ("script", run_file(file, args.get(1..).unwrap_or(&[]))),
    }
}

fn run_text(script: &str, arg0: Option<&str>, pos: &[String]) -> Result<Captured, Refused> {
    let src = lf_only(script);
    check_syntax(&src)?;
    Ok(execute(&src, arg0, pos))
}

fn run_file(file: &str, pos: &[String]) -> Result<Captured, Refused> {
    let text = std::fs::read_to_string(file).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            refuse(
                Code::NotFound,
                format!("no such file: {file}"),
                Some("check the path, then run: krypton --json FILE"),
                1,
            )
        } else {
            refuse(Code::Io, format!("cannot read {file}: {e}"), None, 1)
        }
    })?;
    let src = lf_only(&text);
    check_syntax(&src)?;
    Ok(execute(&src, Some(file), pos))
}

/// A script that does not parse is `invalid`; the message and hint both name the line.
fn check_syntax(src: &str) -> Result<(), Refused> {
    match krypton::parse(src, krypton::Escapes::host()) {
        Ok(_) => Ok(()),
        Err(e) => Err(refuse(
            Code::Invalid,
            format!(
                "syntax error at line {}, column {}: {}",
                e.line, e.col, e.msg
            ),
            Some(&format!("fix the script at line {} and run again", e.line)),
            2, // same exit code as the text path gives a syntax error
        )),
    }
}

/// Run a checked script with both streams captured. Mirrors the text path's exit status handling.
fn execute(src: &str, arg0: Option<&str>, pos: &[String]) -> Captured {
    let started = Instant::now();
    let mut shell = Shell::new();
    shell.set_escapes(krypton::Escapes::host());
    if let Some(name) = arg0 {
        shell.set_arg0(name);
        set_positional(&mut shell, pos);
    }
    let mut stdin = io::stdin();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let st = shell.run(src, &mut stdin, &mut stdout, &mut stderr);
    let code = shell.exited().unwrap_or(st);
    let exit = shell.run_exit_trap(code, &mut io::empty(), &mut stdout, &mut stderr);
    Captured {
        exit,
        stdout,
        stderr,
        duration_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
    }
}

fn data(c: &Captured) -> Json {
    let mut pairs = vec![("exit".to_string(), Json::Int(i64::from(c.exit)))];
    pairs.extend(stream("stdout", &c.stdout));
    pairs.extend(stream("stderr", &c.stderr));
    pairs.push(("duration_ms".to_string(), Json::Int(c.duration_ms)));
    Json::Obj(pairs)
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
