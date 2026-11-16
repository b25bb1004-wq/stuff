//! CLI runner for `bench [--full] [--only SUITE]...`.

use std::ffi::OsString;
use std::process::ExitCode;
use std::thread::available_parallelism;
use std::time::Duration;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use bench::{aurum, iridium, mercury, table, xenon, Opts, Sample};

const USAGE: &str =
    "usage: bench [--full] [--only SUITE]...\nsuites: mercury, iridium, xenon, aurum";

const SUITE_NAMES: &[&str] = &["mercury", "iridium", "xenon", "aurum"];

const RUN_ARGS: &[Arg] = &[
    Arg {
        name: "--full",
        kind: Kind::Flag,
        required: false,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--only",
        kind: Kind::Choice,
        required: false,
        default: None,
        choices: SUITE_NAMES,
    },
];

const COMMANDS: &[Command] = &[Command {
    name: "bench",
    summary: "Run throughput benchmarks; repeat --only to pick suites",
    effect: Effect::Write,
    args: RUN_ARGS,
    returns: "suites: [{name, results: [{name, value, unit}]}]; value is the rate in unit",
    example: "bench --json bench --only mercury",
}];

const MANIFEST: Manifest = Manifest {
    tool: "bench",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Load and throughput benchmarks for mercury, iridium, xenon and aurum",
    commands: COMMANDS,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Suite {
    Mercury,
    Iridium,
    Xenon,
    Aurum,
}

impl Suite {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "mercury" => Some(Self::Mercury),
            "iridium" => Some(Self::Iridium),
            "xenon" => Some(Self::Xenon),
            "aurum" => Some(Self::Aurum),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Mercury => "mercury",
            Self::Iridium => "iridium",
            Self::Xenon => "xenon",
            Self::Aurum => "aurum",
        }
    }

    fn run(self, opts: &Opts) -> Result<Vec<Sample>, String> {
        match self {
            Self::Mercury => mercury::run(opts),
            Self::Iridium => iridium::run(opts),
            Self::Xenon => xenon::run(opts),
            Self::Aurum => aurum::run(opts),
        }
    }
}

fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> Result<(bool, Vec<Suite>), String> {
    let mut full = false;
    let mut selected: Vec<Suite> = Vec::new();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--full" => full = true,
            "--only" => {
                let Some(name) = args.next() else {
                    return Err("missing argument for --only".to_string());
                };
                let Some(suite) = Suite::parse(&name) else {
                    return Err(format!("unknown suite: {name}"));
                };
                if !selected.contains(&suite) {
                    selected.push(suite);
                }
            }
            other => {
                if let Some(name) = other.strip_prefix("--only=") {
                    let Some(suite) = Suite::parse(name) else {
                        return Err(format!("unknown suite: {name}"));
                    };
                    if !selected.contains(&suite) {
                        selected.push(suite);
                    }
                } else {
                    return Err(format!("unknown flag: {other}"));
                }
            }
        }
    }

    if selected.is_empty() {
        selected = vec![Suite::Mercury, Suite::Iridium, Suite::Xenon, Suite::Aurum];
    }

    Ok((full, selected))
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
    let (full, suites) = match parse_args_from(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("error: {msg}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (budget_ms, opts) = budget_opts(full);

    let cores = available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    println!(
        "cores: {cores}, os: {}, budget: {budget_ms}ms",
        std::env::consts::OS
    );

    let mut samples = Vec::new();
    for suite in suites {
        match suite.run(&opts) {
            Ok(mut res) => samples.append(&mut res),
            Err(err) => {
                eprintln!("bench error: {err}");
                return ExitCode::from(1);
            }
        }
    }

    print!("{}", table(&samples));
    ExitCode::SUCCESS
}

/// The budget in milliseconds and the options for a quick run, or a `--full` run.
fn budget_opts(full: bool) -> (u64, Opts) {
    let (budget_ms, scale) = if full {
        (2000u64, 10u32)
    } else {
        (200u64, 1u32)
    };
    let opts = Opts {
        budget: Duration::from_millis(budget_ms),
        scale,
    };
    (budget_ms, opts)
}

/// `bench --json ...`: one envelope on stdout; the text table and banner are not printed.
fn run_json(args: &[OsString]) -> ExitCode {
    let mut args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    // The manifest names the one command `bench`, so `bench --json bench ...` must work too.
    if args.first().map(String::as_str) == Some("bench") {
        args.remove(0);
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        let data = Json::obj([("usage", Json::Str(USAGE.into()))]);
        print!("{}", envelope::ok("bench", "--help", data));
        return ExitCode::SUCCESS;
    }
    let (full, suites) = match parse_args_from(args) {
        Ok(parsed) => parsed,
        Err(msg) => return fail_json("", Code::Usage, &msg, Some("run: bench --describe"), 2),
    };
    let (_, opts) = budget_opts(full);
    let mut groups = Vec::new();
    for suite in suites {
        match suite.run(&opts) {
            Ok(samples) => groups.push(suite_json(suite.name(), &samples)),
            Err(err) => return fail_json("bench", Code::Io, &err, None, 1),
        }
    }
    let data = Json::obj([("suites", Json::Arr(groups))]);
    print!("{}", envelope::ok("bench", "bench", data));
    ExitCode::SUCCESS
}

fn fail_json(cmd: &str, code: Code, msg: &str, hint: Option<&str>, exit: u8) -> ExitCode {
    eprintln!("bench: {msg}");
    print!("{}", envelope::err("bench", cmd, code, msg, hint));
    ExitCode::from(exit)
}

fn suite_json(name: &str, samples: &[Sample]) -> Json {
    let results = samples
        .iter()
        .map(|s| {
            let rate = s.rate();
            Json::obj([
                ("name", Json::Str(s.name.clone())),
                (
                    "value",
                    Json::Num(if rate.is_finite() { rate } else { 0.0 }),
                ),
                ("unit", Json::Str(s.unit.into())),
            ])
        })
        .collect();
    Json::obj([
        ("name", Json::Str(name.into())),
        ("results", Json::Arr(results)),
    ])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn vec_s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn cli_arg_parsing() {
        let (full, suites) = parse_args_from(vec_s(&[])).unwrap();
        assert!(!full);
        assert_eq!(suites.len(), 4);

        let (full, suites) = parse_args_from(vec_s(&[
            "--full",
            "--only",
            "mercury",
            "--only=aurum",
            "--only",
            "mercury",
        ]))
        .unwrap();
        assert!(full);
        assert!(suites == vec![Suite::Mercury, Suite::Aurum]);

        assert!(parse_args_from(vec_s(&["--unknown"])).is_err());
        assert!(parse_args_from(vec_s(&["--only"])).is_err());
        assert!(parse_args_from(vec_s(&["--only", "titan"])).is_err());
    }
}
