//! Command-line runner, input recorder, and deterministic replay tool for `helios`.
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use helios::scene::{replay, Recorder, Stage};
use helios::script::ScriptScene;

const VERSION: &str = "helios 0.1.0";
const DEFAULT_RUN_TICKS: u64 = 300;
const DEFAULT_REPLAY_TICKS: u64 = 60;
const FUEL_PER_FRAME: u64 = 1_000_000;

struct CliError {
    code: u8,
    msg: String,
}

impl CliError {
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

fn print_help() {
    println!(
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
    );
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
            .map_err(|e| CliError::fail(format!("failed to read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read(path).map_err(|e| CliError::fail(format!("failed to read '{path}': {e}")))
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
            .map_err(|e| CliError::fail(format!("failed to write stdout: {e}")))?;
        io::stdout()
            .flush()
            .map_err(|e| CliError::fail(format!("failed to flush stdout: {e}")))?;
        Ok(())
    } else {
        fs::write(path, data).map_err(|e| CliError::fail(format!("failed to write '{path}': {e}")))
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

fn cmd_run(args: Vec<String>) -> Result<bool, CliError> {
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
            print_help();
            return Ok(true);
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

    for _ in 0..ticks {
        if !stage.step(0) {
            break;
        }
        let frame = stage.frame();
        if frame.is_multiple_of(60) {
            println!("tick {frame} hash 0x{:016x}", stage.hash());
        }
    }
    println!("final tick {} hash 0x{:016x}", stage.frame(), stage.hash());
    Ok(false)
}

fn cmd_record(args: Vec<String>) -> Result<bool, CliError> {
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
            print_help();
            return Ok(true);
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
    if out_path != "-" {
        println!("final tick {} hash 0x{hash:016x}", stage.frame());
    }
    Ok(false)
}

fn cmd_replay(args: Vec<String>) -> Result<bool, CliError> {
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
            print_help();
            return Ok(true);
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
    println!("final tick {} hash 0x{hash:016x}", stage.frame());
    Ok(false)
}

fn dispatch() -> Result<(), CliError> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Err(CliError::usage("missing subcommand (try --help)"));
    };

    match first.as_str() {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        "run" => {
            let rest: Vec<String> = args.collect();
            cmd_run(rest).map(|_| ())
        }
        "record" => {
            let rest: Vec<String> = args.collect();
            cmd_record(rest).map(|_| ())
        }
        "replay" => {
            let rest: Vec<String> = args.collect();
            cmd_replay(rest).map(|_| ())
        }
        other => Err(CliError::usage(format!("unknown subcommand '{other}'"))),
    }
}

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("helios: {}", err.msg);
            ExitCode::from(err.code)
        }
    }
}
