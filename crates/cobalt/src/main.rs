//! Command-line runner for Logos (`logos run` and `logos test`).
//!
//! Text mode prints what it always printed. `--describe` prints the manifest and `--json` prints one ACI
//! envelope (docs/ai-computer/ACI.md). The `--json` path lives at the end of this file.
#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::process;
use std::time::Instant;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};

const DEFAULT_FUEL: u64 = 100_000_000;
const DEFAULT_SEED: u64 = 1;
const DEFAULT_CASES: u32 = 100;

fn main() {
    let mut raw: Vec<OsString> = env::args_os().skip(1).collect();
    let json = envelope::take_json_flag(&mut raw);
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return;
    }
    if json {
        process::exit(run_json(&raw));
    }
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        eprintln!("usage: logos <run|test> FILE [options]");
        process::exit(2);
    };
    match cmd {
        "run" => cmd_run(&args[1..]),
        "test" => cmd_test(&args[1..]),
        "--help" | "-h" => println!(
            "usage: logos <run|test> FILE [options]
  logos run FILE [--fuel N]
  logos test FILE [options]"
        ),
        _ => {
            eprintln!("unknown subcommand '{cmd}' (expected 'run' or 'test')");
            process::exit(2);
        }
    }
}

fn cmd_run(args: &[String]) {
    let mut file: Option<&str> = None;
    let mut fuel = DEFAULT_FUEL;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--fuel" {
            i += 1;
            let Some(val) = args.get(i).and_then(|s| parse_u64(s)) else {
                eprintln!("invalid or missing value for --fuel");
                process::exit(2);
            };
            fuel = val;
        } else if let Some(rest) = a.strip_prefix("--fuel=") {
            let Some(val) = parse_u64(rest) else {
                eprintln!("invalid value for --fuel");
                process::exit(2);
            };
            fuel = val;
        } else if a.starts_with('-') {
            eprintln!("unknown option '{a}'");
            process::exit(2);
        } else if file.is_none() {
            file = Some(a);
        } else {
            eprintln!("unexpected argument '{a}'");
            process::exit(2);
        }
        i += 1;
    }
    let Some(path) = file else {
        eprintln!("usage: logos run FILE [--fuel N]");
        process::exit(2);
    };
    let (src, compiled) = load_and_compile(path);
    let mut input = String::new();
    if compiled.module.imports.iter().any(|s| s == "io.read_line") {
        let _ = io::stdin().read_to_string(&mut input);
    }
    let out = cobalt::run_main(&compiled, &input, fuel);
    print!("{}", out.output);
    let _ = io::stdout().flush();
    if let Err((trap, span)) = out.result {
        let (line, col) = logos::line_col(&src, span.start);
        eprintln!("trap: {:?} at {}:{}", trap, line, col);
        process::exit(1);
    }
}

fn cmd_test(args: &[String]) {
    let mut file: Option<&str> = None;
    let mut fuel = DEFAULT_FUEL;
    let mut seed = DEFAULT_SEED;
    let mut cases = DEFAULT_CASES;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--fuel" {
            i += 1;
            let Some(val) = args.get(i).and_then(|s| parse_u64(s)) else {
                eprintln!("invalid or missing value for --fuel");
                process::exit(2);
            };
            fuel = val;
        } else if let Some(rest) = a.strip_prefix("--fuel=") {
            let Some(val) = parse_u64(rest) else {
                eprintln!("invalid value for --fuel");
                process::exit(2);
            };
            fuel = val;
        } else if a == "--seed" {
            i += 1;
            let Some(val) = args.get(i).and_then(|s| parse_u64(s)) else {
                eprintln!("invalid or missing value for --seed");
                process::exit(2);
            };
            seed = val;
        } else if let Some(rest) = a.strip_prefix("--seed=") {
            let Some(val) = parse_u64(rest) else {
                eprintln!("invalid value for --seed");
                process::exit(2);
            };
            seed = val;
        } else if a == "--cases" {
            i += 1;
            let Some(val) = args.get(i).and_then(|s| s.parse::<u32>().ok()) else {
                eprintln!("invalid or missing value for --cases");
                process::exit(2);
            };
            cases = val;
        } else if let Some(rest) = a.strip_prefix("--cases=") {
            let Some(val) = rest.parse::<u32>().ok() else {
                eprintln!("invalid value for --cases");
                process::exit(2);
            };
            cases = val;
        } else if a.starts_with('-') {
            eprintln!("unknown option '{a}'");
            process::exit(2);
        } else if file.is_none() {
            file = Some(a);
        } else {
            eprintln!("unexpected argument '{a}'");
            process::exit(2);
        }
        i += 1;
    }
    let Some(path) = file else {
        eprintln!("usage: logos test FILE [--seed N] [--cases N]");
        process::exit(2);
    };
    let (src, compiled) = load_and_compile(path);
    let mut passed = 0usize;
    let mut failed = 0usize;

    for (idx, (name, _)) in compiled.tests.iter().enumerate() {
        let out = cobalt::run_test(&compiled, idx, fuel);
        match out.result {
            Ok(()) => {
                passed += 1;
                println!("ok {name}");
            }
            Err((trap, span)) => {
                failed += 1;
                let (line, col) = logos::line_col(&src, span.start);
                println!("FAIL {name}: {trap:?} at {line}:{col}");
            }
        }
    }

    for (idx, prop) in compiled.props.iter().enumerate() {
        let out = cobalt::run_prop(&compiled, idx, seed, cases, fuel);
        match out.result {
            Ok(()) => {
                passed += 1;
                println!("ok {}", prop.name);
            }
            Err((trap, span)) => {
                failed += 1;
                let (line, col) = logos::line_col(&src, span.start);
                let summary = out.output.lines().last().unwrap_or("");
                println!("FAIL {}: {trap:?} at {line}:{col} ({summary})", prop.name);
            }
        }
    }

    println!("{passed} passed, {failed} failed");
    if failed > 0 {
        process::exit(1);
    }
}

fn load_and_compile(path: &str) -> (String, cobalt::Compiled) {
    let Ok(src) = fs::read_to_string(path) else {
        eprintln!("{path}:1:1: failed to read file");
        process::exit(2);
    };
    let ast = match logos::parser::parse(&src) {
        Ok(m) => m,
        Err(diag) => {
            let (line, col) = logos::line_col(&src, diag.span.start);
            eprintln!("{path}:{line}:{col}: {}", diag.message);
            process::exit(2);
        }
    };
    let ir = match logos::check::check(&ast) {
        Ok(p) => p,
        Err(diag) => {
            let (line, col) = logos::line_col(&src, diag.span.start);
            eprintln!("{path}:{line}:{col}: {}", diag.message);
            process::exit(2);
        }
    };
    (src, cobalt::compile(&ir))
}

fn parse_u64(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

// ---- --describe and --json (docs/ai-computer/ACI.md). The text path above is unchanged. ----

const USAGE: &str = "usage: logos <run|test> FILE [options]
  logos run FILE [--fuel N]
  logos test FILE [options]";

const FILE_ARG: Arg = Arg {
    name: "FILE",
    kind: Kind::Path,
    required: true,
    default: None,
    choices: &[],
};

const FUEL_ARG: Arg = Arg {
    name: "--fuel",
    kind: Kind::Int,
    required: false,
    default: Some("100000000"),
    choices: &[],
};

const RUN_ARGS: &[Arg] = &[FILE_ARG, FUEL_ARG];
const TEST_ARGS: &[Arg] = &[
    FILE_ARG,
    FUEL_ARG,
    Arg {
        name: "--seed",
        kind: Kind::Int,
        required: false,
        default: Some("1"),
        choices: &[],
    },
    Arg {
        name: "--cases",
        kind: Kind::Int,
        required: false,
        default: Some("100"),
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[
    Command {
        name: "run",
        summary: "Run the main function of FILE; its io.print output is stdout",
        effect: Effect::Exec,
        args: RUN_ARGS,
        returns: "exit (0), stop (exit), stdout (capped, with stdout_truncated, stdout_bytes, stdout_lossy when they apply), result (main's return value as text, null when main returns nothing), duration_ms; a trap or compile error is code invalid",
        example: "logos --json run prog.logos",
    },
    Command {
        name: "test",
        summary: "Run the tests and properties of FILE",
        effect: Effect::Exec,
        args: TEST_ARGS,
        returns: "passed, failed, results: [{name, status (ok or fail), message}]; exit 1 when any test fails",
        example: "logos --json test prog.logos --cases 50",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "logos",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Logos language runner: run and test programs",
    commands: COMMANDS,
};

/// A failed `--json` run: envelope code and message, the hint, and the process exit code.
struct Fail {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: i32,
}

impl Fail {
    fn usage(message: String) -> Fail {
        Fail {
            code: Code::Usage,
            message,
            hint: Some(String::from("run: logos --describe")),
            exit: 2,
        }
    }
}

fn run_json(raw: &[OsString]) -> i32 {
    let args: Vec<String> = raw
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let first = args.first().map(String::as_str);
    let cmd = ["run", "test"]
        .into_iter()
        .find(|c| first == Some(*c))
        .unwrap_or("");
    match json_command(&args) {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("logos", cmd, data));
            exit
        }
        Err(fail) => {
            eprintln!("logos: {}", fail.message);
            print!(
                "{}",
                envelope::err("logos", cmd, fail.code, &fail.message, fail.hint.as_deref())
            );
            fail.exit
        }
    }
}

fn json_command(args: &[String]) -> Result<(Json, i32), Fail> {
    match args.first().map(String::as_str) {
        Some("run") => json_run(&args[1..]),
        Some("test") => json_test(&args[1..]),
        Some("--help" | "-h") => Ok((Json::obj([("usage", Json::Str(USAGE.into()))]), 0)),
        Some(other) => Err(Fail::usage(format!(
            "unknown subcommand '{other}' (expected 'run' or 'test')"
        ))),
        None => Err(Fail::usage(String::from(
            "missing subcommand (expected 'run' or 'test')",
        ))),
    }
}

/// Options and the one FILE for `run` and `test`, with the value flags the command takes.
struct Opts {
    file: String,
    fuel: u64,
    seed: u64,
    cases: u32,
}

fn parse_opts(args: &[String], value_flags: &[&str]) -> Result<Opts, Fail> {
    let mut opts = Opts {
        file: String::new(),
        fuel: DEFAULT_FUEL,
        seed: DEFAULT_SEED,
        cases: DEFAULT_CASES,
    };
    let mut file: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let (flag, inline) = a.split_once('=').map_or((a, None), |(f, v)| (f, Some(v)));
        if value_flags.contains(&flag) {
            let val = match inline {
                Some(v) => v.to_string(),
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| Fail::usage(format!("missing value for {flag}")))?
                }
            };
            let bad = || Fail::usage(format!("invalid value for {flag}: {val}"));
            match flag {
                "--fuel" => opts.fuel = parse_u64(&val).ok_or_else(bad)?,
                "--seed" => opts.seed = parse_u64(&val).ok_or_else(bad)?,
                _ => opts.cases = val.parse::<u32>().map_err(|_| bad())?,
            }
        } else if a.starts_with('-') {
            return Err(Fail::usage(format!("unknown option '{a}'")));
        } else if file.is_none() {
            file = Some(a);
        } else {
            return Err(Fail::usage(format!("unexpected argument '{a}'")));
        }
        i += 1;
    }
    opts.file = file
        .ok_or_else(|| Fail::usage(String::from("missing FILE")))?
        .to_string();
    Ok(opts)
}

fn json_run(args: &[String]) -> Result<(Json, i32), Fail> {
    let opts = parse_opts(args, &["--fuel"])?;
    let (src, compiled) = load_json(&opts.file)?;
    let mut input = String::new();
    if compiled.module.imports.iter().any(|s| s == "io.read_line") {
        let _ = io::stdin().read_to_string(&mut input);
    }
    let started = Instant::now();
    let (out, value) = cobalt::run_main_value(&compiled, &input, opts.fuel);
    if let Err((trap, span)) = out.result {
        let (line, col) = logos::line_col(&src, span.start);
        return Err(Fail {
            code: Code::Invalid,
            message: format!("trap: {trap:?} at {line}:{col}"),
            hint: None,
            exit: 1,
        });
    }
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let mut pairs = vec![
        ("exit".to_string(), Json::Int(0)),
        ("stop".to_string(), Json::Str("exit".into())),
    ];
    pairs.extend(stream("stdout", out.output.as_bytes()));
    // the value main returned, rendered as elysium --json run renders it; null when main returns nothing
    pairs.push(("result".to_string(), value.map_or(Json::Null, Json::Str)));
    pairs.push(("duration_ms".to_string(), Json::Int(duration_ms)));
    Ok((Json::Obj(pairs), 0))
}

fn json_test(args: &[String]) -> Result<(Json, i32), Fail> {
    let opts = parse_opts(args, &["--fuel", "--seed", "--cases"])?;
    let (src, compiled) = load_json(&opts.file)?;
    let mut results = Vec::new();
    let (mut passed, mut failed) = (0i64, 0i64);

    for (idx, (name, _)) in compiled.tests.iter().enumerate() {
        let out = cobalt::run_test(&compiled, idx, opts.fuel);
        let (status, message) = match out.result {
            Ok(()) => ("ok", String::new()),
            Err((trap, span)) => {
                let (line, col) = logos::line_col(&src, span.start);
                ("fail", format!("{trap:?} at {line}:{col}"))
            }
        };
        if status == "ok" {
            passed += 1;
        } else {
            failed += 1;
        }
        results.push(result_json(name, status, &message));
    }

    for (idx, prop) in compiled.props.iter().enumerate() {
        let out = cobalt::run_prop(&compiled, idx, opts.seed, opts.cases, opts.fuel);
        let (status, message) = match out.result {
            Ok(()) => ("ok", String::new()),
            Err((trap, span)) => {
                let (line, col) = logos::line_col(&src, span.start);
                let summary = out.output.lines().last().unwrap_or("");
                ("fail", format!("{trap:?} at {line}:{col} ({summary})"))
            }
        };
        if status == "ok" {
            passed += 1;
        } else {
            failed += 1;
        }
        results.push(result_json(&prop.name, status, &message));
    }

    let exit = i32::from(failed > 0);
    let data = Json::obj([
        ("passed", Json::Int(passed)),
        ("failed", Json::Int(failed)),
        ("results", Json::Arr(results)),
    ]);
    Ok((data, exit))
}

fn result_json(name: &str, status: &str, message: &str) -> Json {
    Json::obj([
        ("name", Json::Str(name.into())),
        ("status", Json::Str(status.into())),
        ("message", Json::Str(message.into())),
    ])
}

/// Read and compile a file for `--json`. Read, parse and check errors give the same messages and exit
/// codes as the text path (read failure 2 with code io, compile error 2 with code invalid).
fn load_json(path: &str) -> Result<(String, cobalt::Compiled), Fail> {
    let src = fs::read_to_string(path).map_err(|err| Fail {
        code: if err.kind() == std::io::ErrorKind::NotFound {
            Code::NotFound
        } else {
            Code::Io
        },
        message: format!("{path}:1:1: failed to read file"),
        hint: None,
        exit: 2,
    })?;
    let compile_error = |diag: &logos::Diag| Fail {
        code: Code::Invalid,
        message: {
            let (line, col) = logos::line_col(&src, diag.span.start);
            format!("{path}:{line}:{col}: {}", diag.message)
        },
        hint: None,
        exit: 2,
    };
    let ast = logos::parser::parse(&src).map_err(|d| compile_error(&d))?;
    let ir = logos::check::check(&ast).map_err(|d| compile_error(&d))?;
    Ok((src, cobalt::compile(&ir)))
}

/// A captured text value: `name` for the text, `name_<field>` for `truncated`, `bytes`, `lossy`.
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
