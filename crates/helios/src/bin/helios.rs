//! Command-line runner, input recorder, and deterministic replay tool for `helios`.
//!
//! Text mode prints the same lines as before. `--describe` prints the manifest, and `--json` prints one ACI
//! envelope (docs/ai-computer/ACI.md). Each subcommand computes an `Outcome`; text and JSON differ only in
//! how they print it.
#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use helios::scene::{replay, Recorder, Stage};
use helios::script::ScriptScene;

const VERSION: &str = "helios 0.1.0";
const DEFAULT_RUN_TICKS: u64 = 300;
const DEFAULT_REPLAY_TICKS: u64 = 60;
const FUEL_PER_FRAME: u64 = 1_000_000;

struct CliError {
    code: u8,
    kind: Code,
    hint: Option<&'static str>,
    msg: String,
}

impl CliError {
    fn fail(msg: impl Into<String>) -> Self {
        Self {
            code: 1,
            kind: Code::Invalid,
            hint: None,
            msg: msg.into(),
        }
    }

    fn usage(msg: impl Into<String>) -> Self {
        Self::refused(msg, "run: helios --describe")
    }

    fn refused(msg: impl Into<String>, hint: &'static str) -> Self {
        Self {
            code: 2,
            kind: Code::Usage,
            hint: Some(hint),
            msg: msg.into(),
        }
    }

    fn io(msg: impl Into<String>, err: &io::Error) -> Self {
        let kind = if err.kind() == io::ErrorKind::NotFound {
            Code::NotFound
        } else {
            Code::Io
        };
        Self {
            code: 1,
            kind,
            hint: None,
            msg: msg.into(),
        }
    }
}

/// What a subcommand computed. Text and JSON print it in their own way.
enum Outcome {
    Help,
    Run {
        ticks: u64,
        seed: u64,
        hashes: Vec<(u64, u64)>,
        final_hash: u64,
    },
    Record {
        path: String,
        bytes: usize,
        ticks: u64,
        final_hash: u64,
        stdout: bool,
    },
    Replay {
        ticks: u64,
        seed: u64,
        final_hash: u64,
    },
}

const fn req(name: &'static str, kind: ArgKind) -> Arg {
    Arg {
        name,
        kind,
        required: true,
        default: None,
        choices: &[],
    }
}

const fn opt(name: &'static str, kind: ArgKind, default: &'static str) -> Arg {
    Arg {
        name,
        kind,
        required: false,
        default: Some(default),
        choices: &[],
    }
}

const RUN_ARGS: &[Arg] = &[
    req("GAME", ArgKind::Path),
    opt("--ticks", ArgKind::Int, "300"),
    opt("--seed", ArgKind::Int, "0"),
];
const RECORD_ARGS: &[Arg] = &[
    req("GAME", ArgKind::Path),
    req("INPUTS", ArgKind::Path),
    req("-o", ArgKind::Path),
    opt("--ticks", ArgKind::Int, "last input frame + 1"),
    opt("--seed", ArgKind::Int, "0"),
];
const REPLAY_ARGS: &[Arg] = &[
    req("GAME", ArgKind::Path),
    req("REPLAY", ArgKind::Path),
    opt("--ticks", ArgKind::Int, "last input frame + 1"),
    opt("--seed", ArgKind::Int, "0"),
];

const COMMANDS: &[Command] = &[
    Command {
        name: "run",
        summary: "Run a Logos game headless; hashes are recorded every 60 ticks",
        effect: Effect::Read,
        args: RUN_ARGS,
        returns: "ticks, seed, hashes: [{tick, hash}] (hash as lowercase hex), final_hash",
        example: "helios --json run game.logos --ticks 180 --seed 42",
    },
    Command {
        name: "record",
        summary: "Record key-state inputs (\"<frame> <keys>\" lines) into a TREC file at -o FILE",
        effect: Effect::Write,
        args: RECORD_ARGS,
        returns: "path, bytes, ticks (-o - is refused under --json)",
        example: "helios --json record game.logos inputs.txt -o run.trec",
    },
    Command {
        name: "replay",
        summary: "Replay a TREC file against a Logos game and print the final world hash",
        effect: Effect::Read,
        args: REPLAY_ARGS,
        returns: "ticks, seed, final_hash (hash as lowercase hex)",
        example: "helios --json replay game.logos run.trec",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "helios",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Deterministic game runner and input replay tool",
    commands: COMMANDS,
};

fn help_text() -> String {
    format!(
        "\
{VERSION} - deterministic game runner and input replay tool

USAGE:
  helios <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  run GAME.logos [--ticks N] [--seed S]
      Run a Logos game headless, printing the world hash every 60 ticks and at the end.
      Defaults: --ticks {DEFAULT_RUN_TICKS} --seed 0. Use '-' for stdin.
      Example: helios run game.logos --ticks 180 --seed 42

  record GAME.logos INPUTS -o REPLAY [--ticks N] [--seed S]
      Record text key-state inputs (\"<frame> <keys>\" lines) into a TREC replay file
      and print the final world hash. Use '-' for stdin/stdout.
      Example: helios record game.logos inputs.txt -o run.trec

  replay GAME.logos REPLAY [--ticks N] [--seed S]
      Replay a TREC file against a Logos game and print the final world hash.
      Example: helios replay game.logos run.trec

OPTIONS:
  -h, --help       Print this help message
  -V, --version    Print version ({VERSION})"
    )
}

fn print_help() {
    println!("{}", help_text());
}

fn parse_u64_radix(s: &str, flag: &str) -> Result<u64, CliError> {
    let res = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    };
    res.map_err(|_| CliError::usage(format!("invalid {flag} value '{s}'")))
}

fn parse_u32_radix(s: &str, label: &str) -> Result<u32, CliError> {
    let res = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        s.parse::<u32>()
    };
    res.map_err(|_| CliError::fail(format!("invalid {label} '{s}'")))
}

fn read_bytes(path: &str) -> Result<Vec<u8>, CliError> {
    if path == "-" {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CliError::io(format!("failed to read stdin: {e}"), &e))?;
        Ok(buf)
    } else {
        fs::read(path).map_err(|e| CliError::io(format!("failed to read '{path}': {e}"), &e))
    }
}

fn read_text(path: &str) -> Result<String, CliError> {
    let bytes = read_bytes(path)?;
    String::from_utf8(bytes)
        .map_err(|e| CliError::fail(format!("'{path}' is not valid UTF-8: {e}")))
}

fn write_bytes(path: &str, data: &[u8]) -> Result<(), CliError> {
    if path == "-" {
        io::stdout()
            .write_all(data)
            .map_err(|e| CliError::io(format!("failed to write stdout: {e}"), &e))?;
        io::stdout()
            .flush()
            .map_err(|e| CliError::io(format!("failed to flush stdout: {e}"), &e))?;
        Ok(())
    } else {
        fs::write(path, data)
            .map_err(|e| CliError::io(format!("failed to write '{path}': {e}"), &e))
    }
}

fn make_stage(src: &str, seed: u64) -> Result<Stage, CliError> {
    let scene = ScriptScene::new(src, seed, FUEL_PER_FRAME)
        .map_err(|e| CliError::fail(format!("script error: {e}")))?;
    Ok(Stage::new(Box::new(scene)))
}

fn parse_inputs(text: &str) -> Result<Recorder, CliError> {
    let mut rec = Recorder::new();
    let mut prev_frame: Option<u64> = None;

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx.saturating_add(1);
        let line = match raw_line.split_once('#') {
            Some((before, _)) => before,
            None => raw_line,
        }
        .trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(frame_tok) = parts.next() else {
            continue;
        };
        let Some(keys_tok) = parts.next() else {
            return Err(CliError::fail(format!(
                "inputs line {line_no}: expected '<frame> <keys>'"
            )));
        };
        if parts.next().is_some() {
            return Err(CliError::fail(format!(
                "inputs line {line_no}: unexpected extra token"
            )));
        }

        let frame = frame_tok.parse::<u64>().map_err(|_| {
            CliError::fail(format!(
                "inputs line {line_no}: invalid frame '{frame_tok}'"
            ))
        })?;
        if let Some(prev) = prev_frame {
            if frame < prev {
                return Err(CliError::fail(format!(
                    "inputs line {line_no}: frame {frame} is earlier than previous frame {prev}"
                )));
            }
        }
        prev_frame = Some(frame);

        let keys = parse_u32_radix(keys_tok, &format!("keys on line {line_no}"))?;
        rec.record(frame, keys);
    }

    Ok(rec)
}

fn take_flag_value(
    flag: &str,
    inline: Option<&str>,
    iter: &mut std::vec::IntoIter<String>,
) -> Result<String, CliError> {
    if let Some(val) = inline {
        return Ok(val.to_owned());
    }
    iter.next()
        .ok_or_else(|| CliError::usage(format!("option '{flag}' requires a value")))
}

fn cmd_run(args: Vec<String>) -> Result<Outcome, CliError> {
    let mut game_path: Option<String> = None;
    let mut ticks = DEFAULT_RUN_TICKS;
    let mut seed = 0u64;
    let mut raw_only = false;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        if !raw_only && arg == "--" {
            raw_only = true;
            continue;
        }
        if !raw_only && (arg == "-h" || arg == "--help") {
            return Ok(Outcome::Help);
        }
        if !raw_only && arg.starts_with('-') && arg != "-" {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f, Some(v)),
                None => (arg.as_str(), None),
            };
            match flag {
                "--ticks" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    ticks = parse_u64_radix(&v, "--ticks")?;
                }
                "--seed" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    seed = parse_u64_radix(&v, "--seed")?;
                }
                _ => return Err(CliError::usage(format!("unknown option '{arg}'"))),
            }
            continue;
        }
        if game_path.is_none() {
            game_path = Some(arg);
        } else {
            return Err(CliError::usage(format!("unexpected argument '{arg}'")));
        }
    }

    let game_path = game_path.ok_or_else(|| CliError::usage("run requires GAME.logos"))?;
    let src = read_text(&game_path)?;
    let mut stage = make_stage(&src, seed)?;

    let mut hashes = Vec::new();
    for _ in 0..ticks {
        if !stage.step(0) {
            break;
        }
        let frame = stage.frame();
        if frame.is_multiple_of(60) {
            hashes.push((frame, stage.hash()));
        }
    }
    Ok(Outcome::Run {
        ticks: stage.frame(),
        seed,
        hashes,
        final_hash: stage.hash(),
    })
}

/// `json` is true under `--json`, where `-o -` is refused: TREC bytes need a file.
fn cmd_record(args: Vec<String>, json: bool) -> Result<Outcome, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut out_path: Option<String> = None;
    let mut ticks: Option<u64> = None;
    let mut seed = 0u64;
    let mut raw_only = false;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        if !raw_only && arg == "--" {
            raw_only = true;
            continue;
        }
        if !raw_only && (arg == "-h" || arg == "--help") {
            return Ok(Outcome::Help);
        }
        if !raw_only && arg.starts_with('-') && arg != "-" {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f, Some(v)),
                None => (arg.as_str(), None),
            };
            match flag {
                "-o" | "--out" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    out_path = Some(v);
                }
                "--ticks" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    ticks = Some(parse_u64_radix(&v, "--ticks")?);
                }
                "--seed" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    seed = parse_u64_radix(&v, "--seed")?;
                }
                _ => return Err(CliError::usage(format!("unknown option '{arg}'"))),
            }
            continue;
        }
        positionals.push(arg);
    }

    if positionals.len() > 2 {
        return Err(CliError::usage(format!(
            "unexpected argument '{}'",
            positionals.get(2).map_or("", String::as_str)
        )));
    }
    let game_path = positionals
        .first()
        .ok_or_else(|| CliError::usage("record requires GAME.logos and INPUTS"))?;
    let inputs_path = positionals
        .get(1)
        .ok_or_else(|| CliError::usage("record requires INPUTS"))?;
    let out_path = out_path.ok_or_else(|| CliError::usage("record requires -o REPLAY"))?;

    if json && out_path == "-" {
        return Err(CliError::refused(
            "--json needs -o FILE.trec; TREC bytes cannot go to stdout",
            "use -o FILE.trec",
        ));
    }
    if game_path == "-" && inputs_path == "-" {
        return Err(CliError::usage(
            "cannot read both GAME.logos and INPUTS from stdin",
        ));
    }

    let src = read_text(game_path)?;
    let inputs_text = read_text(inputs_path)?;
    let rec = parse_inputs(&inputs_text)?;
    let mut stage = make_stage(&src, seed)?;

    let frames = ticks.unwrap_or_else(|| {
        rec.inputs
            .last()
            .map_or(DEFAULT_REPLAY_TICKS, |inp| inp.frame.saturating_add(1))
    });
    let hash = replay(&mut stage, &rec, frames);

    let encoded = rec.encode();
    write_bytes(&out_path, &encoded)?;
    Ok(Outcome::Record {
        stdout: out_path == "-",
        path: out_path,
        bytes: encoded.len(),
        ticks: stage.frame(),
        final_hash: hash,
    })
}

fn cmd_replay(args: Vec<String>) -> Result<Outcome, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut ticks: Option<u64> = None;
    let mut seed = 0u64;
    let mut raw_only = false;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        if !raw_only && arg == "--" {
            raw_only = true;
            continue;
        }
        if !raw_only && (arg == "-h" || arg == "--help") {
            return Ok(Outcome::Help);
        }
        if !raw_only && arg.starts_with('-') && arg != "-" {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f, Some(v)),
                None => (arg.as_str(), None),
            };
            match flag {
                "--ticks" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    ticks = Some(parse_u64_radix(&v, "--ticks")?);
                }
                "--seed" => {
                    let v = take_flag_value(flag, inline, &mut iter)?;
                    seed = parse_u64_radix(&v, "--seed")?;
                }
                _ => return Err(CliError::usage(format!("unknown option '{arg}'"))),
            }
            continue;
        }
        positionals.push(arg);
    }

    if positionals.len() > 2 {
        return Err(CliError::usage(format!(
            "unexpected argument '{}'",
            positionals.get(2).map_or("", String::as_str)
        )));
    }
    let game_path = positionals
        .first()
        .ok_or_else(|| CliError::usage("replay requires GAME.logos and REPLAY"))?;
    let replay_path = positionals
        .get(1)
        .ok_or_else(|| CliError::usage("replay requires REPLAY"))?;

    if game_path == "-" && replay_path == "-" {
        return Err(CliError::usage(
            "cannot read both GAME.logos and REPLAY from stdin",
        ));
    }

    let src = read_text(game_path)?;
    let replay_bytes = read_bytes(replay_path)?;
    let rec = Recorder::decode(&replay_bytes)
        .ok_or_else(|| CliError::fail("invalid TREC replay file"))?;
    let mut stage = make_stage(&src, seed)?;

    let frames = ticks.unwrap_or_else(|| {
        rec.inputs
            .last()
            .map_or(DEFAULT_REPLAY_TICKS, |inp| inp.frame.saturating_add(1))
    });
    let hash = replay(&mut stage, &rec, frames);
    Ok(Outcome::Replay {
        ticks: stage.frame(),
        seed,
        final_hash: hash,
    })
}

/// The text output of an outcome, byte for byte what the tool printed before `--json` existed.
fn print_outcome(outcome: &Outcome) {
    match outcome {
        Outcome::Help => print_help(),
        Outcome::Run {
            ticks,
            hashes,
            final_hash,
            ..
        } => {
            for (tick, hash) in hashes {
                println!("tick {tick} hash 0x{hash:016x}");
            }
            println!("final tick {ticks} hash 0x{final_hash:016x}");
        }
        Outcome::Record {
            stdout,
            ticks,
            final_hash,
            ..
        } => {
            if !stdout {
                println!("final tick {ticks} hash 0x{final_hash:016x}");
            }
        }
        Outcome::Replay {
            ticks, final_hash, ..
        } => println!("final tick {ticks} hash 0x{final_hash:016x}"),
    }
}

fn int<T: TryInto<i64>>(n: T) -> Json {
    Json::Int(n.try_into().unwrap_or(i64::MAX))
}

fn hex(value: u64) -> Json {
    Json::Str(format!("{value:016x}"))
}

fn outcome_json(outcome: Outcome) -> Json {
    match outcome {
        Outcome::Help => Json::obj([("usage", Json::Str(help_text()))]),
        Outcome::Run {
            ticks,
            seed,
            hashes,
            final_hash,
        } => {
            let list = hashes
                .into_iter()
                .map(|(tick, hash)| Json::obj([("tick", int(tick)), ("hash", hex(hash))]))
                .collect();
            Json::obj([
                ("ticks", int(ticks)),
                ("seed", int(seed)),
                ("hashes", Json::Arr(list)),
                ("final_hash", hex(final_hash)),
            ])
        }
        Outcome::Record {
            path, bytes, ticks, ..
        } => Json::obj([
            ("path", Json::path(Path::new(&path))),
            ("bytes", int(bytes)),
            ("ticks", int(ticks)),
        ]),
        Outcome::Replay {
            ticks,
            seed,
            final_hash,
        } => Json::obj([
            ("ticks", int(ticks)),
            ("seed", int(seed)),
            ("final_hash", hex(final_hash)),
        ]),
    }
}

fn dispatch() -> Result<(), CliError> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Err(CliError::usage("missing subcommand (try --help)"));
    };

    let outcome = match first.as_str() {
        "-h" | "--help" | "help" => Outcome::Help,
        "-V" | "--version" => {
            println!("{VERSION}");
            return Ok(());
        }
        "run" => cmd_run(args.collect())?,
        "record" => cmd_record(args.collect(), false)?,
        "replay" => cmd_replay(args.collect())?,
        other => return Err(CliError::usage(format!("unknown subcommand '{other}'"))),
    };
    print_outcome(&outcome);
    Ok(())
}

/// `helios --json ...`: one envelope on stdout; stderr keeps the `helios: message` line.
fn run_json(args: &[OsString]) -> ExitCode {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cmd = match args.first().map(String::as_str) {
        Some("run") => "run",
        Some("record") => "record",
        Some("replay") => "replay",
        Some("-h" | "--help" | "help") => "--help",
        Some("-V" | "--version") => "--version",
        _ => "",
    };
    match json_command(&args) {
        Ok(data) => {
            print!("{}", envelope::ok("helios", cmd, data));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("helios: {}", err.msg);
            print!(
                "{}",
                envelope::err("helios", cmd, err.kind, &err.msg, err.hint)
            );
            ExitCode::from(err.code)
        }
    }
}

fn json_command(args: &[String]) -> Result<Json, CliError> {
    let Some((first, rest)) = args.split_first() else {
        return Err(CliError::usage("missing subcommand (try --help)"));
    };
    let rest = rest.to_vec();
    match first.as_str() {
        "-h" | "--help" | "help" => Ok(outcome_json(Outcome::Help)),
        "-V" | "--version" => Ok(Json::obj([("version", Json::Str(VERSION.into()))])),
        "run" => cmd_run(rest).map(outcome_json),
        "record" => cmd_record(rest, true).map(outcome_json),
        "replay" => cmd_replay(rest).map(outcome_json),
        other => Err(CliError::usage(format!("unknown subcommand '{other}'"))),
    }
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
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("helios: {}", err.msg);
            ExitCode::from(err.code)
        }
    }
}
