//! `aci`: checks Monolith agent-interface manifests and envelopes (docs/ai-computer/ACI.md).
//!
//! Exit codes: 0 valid, 1 invalid or unreadable, 2 usage. Under `--json` the result is one envelope on stdout.

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;

const VERSION: &str = "0.1.0";
const MAX_INPUT: u64 = 8 * 1024 * 1024;

const HELP: &str =
    "aci: checks Monolith agent-interface manifests and envelopes (docs/ai-computer/ACI.md)

usage:
  aci --describe                 print this tool's manifest
  aci check-manifest FILE|-      exit 0 if the manifest is valid
  aci check-envelope FILE|-      exit 0 if the envelope is valid
  aci --version
  aci --json CMD ARGS...         print the result as one JSON envelope

exit codes: 0 valid, 1 invalid or unreadable, 2 usage
";

const FILE_ARG: &[Arg] = &[Arg {
    name: "FILE",
    kind: Kind::Path,
    required: true,
    default: None,
    choices: &[],
}];

const COMMANDS: &[Command] = &[
    Command {
        name: "check-manifest",
        summary: "Check a tool manifest read from FILE or stdin",
        effect: Effect::Read,
        args: FILE_ARG,
        returns: "nothing on success; with --json, {valid: true}",
        example: "aci check-manifest manifest.json",
    },
    Command {
        name: "check-envelope",
        summary: "Check an envelope read from FILE or stdin",
        effect: Effect::Read,
        args: FILE_ARG,
        returns: "nothing on success; with --json, {valid: true}",
        example: "aci check-envelope envelope.json",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "aci",
    version: VERSION,
    summary: "Agent-interface checker for Monolith tools",
    commands: COMMANDS,
};

fn main() -> ExitCode {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut args);
    if envelope::take_describe_flag(&args) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    let name = args
        .first()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default();
    let code = match name.as_str() {
        "--version" => {
            println!("aci {VERSION}");
            0
        }
        "--help" | "-h" => {
            print!("{HELP}");
            0
        }
        "check-manifest" => check(
            &args,
            json,
            "check-manifest",
            aci::manifest::validate_manifest,
        ),
        "check-envelope" => check(&args, json, "check-envelope", envelope::validate_envelope),
        "" => usage(json, "", "no command given"),
        other => usage(json, "", &format!("unknown command: {other}")),
    };
    ExitCode::from(code)
}

/// Report a usage error (exit 2) as text on stderr, or as an envelope on stdout under `--json`.
fn usage(json: bool, cmd: &str, message: &str) -> u8 {
    if json {
        print!(
            "{}",
            envelope::err("aci", cmd, Code::Usage, message, Some("run: aci --help"))
        );
    } else {
        eprintln!("aci: {message}");
    }
    2
}

fn check(
    args: &[OsString],
    json: bool,
    cmd: &str,
    validator: fn(&Json) -> Result<(), String>,
) -> u8 {
    if args.len() != 2 {
        return usage(json, cmd, &format!("{cmd} needs one argument: FILE or -"));
    }
    let result = read_source(&args[1])
        .map_err(|e| (Code::Io, format!("cannot read input: {e}")))
        .and_then(|buf| check_bytes(&buf, validator));
    match result {
        Ok(()) => {
            if json {
                print!(
                    "{}",
                    envelope::ok("aci", cmd, Json::obj([("valid", Json::Bool(true))]))
                );
            }
            0
        }
        Err((code, message)) => {
            if json {
                print!("{}", envelope::err("aci", cmd, code, &message, None));
            } else {
                eprintln!("aci: {message}");
            }
            1
        }
    }
}

/// Read a file, or stdin for `-`, reading at most one byte past the size limit.
fn read_source(path: &OsStr) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    if path == "-" {
        io::stdin()
            .lock()
            .take(MAX_INPUT + 1)
            .read_to_end(&mut buf)?;
    } else {
        File::open(path)?
            .take(MAX_INPUT + 1)
            .read_to_end(&mut buf)?;
    }
    Ok(buf)
}

fn check_bytes(
    buf: &[u8],
    validator: fn(&Json) -> Result<(), String>,
) -> Result<(), (Code, String)> {
    let invalid = |message: String| (Code::Invalid, message);
    if buf.len() as u64 > MAX_INPUT {
        return Err(invalid("input is larger than 8 MiB".into()));
    }
    let text = std::str::from_utf8(buf).map_err(|_| invalid("input is not valid UTF-8".into()))?;
    let value = Json::parse(text).map_err(|e| invalid(format!("invalid JSON: {e}")))?;
    validator(&value).map_err(invalid)
}
