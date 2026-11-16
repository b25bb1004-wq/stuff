//! CLI runner for `bench [--full] [--only SUITE]...`.

use std::process::ExitCode;
use std::thread::available_parallelism;
use std::time::Duration;

use bench::{aurum, iridium, mercury, table, xenon, Opts, Sample};

const USAGE: &str =
    "usage: bench [--full] [--only SUITE]...\nsuites: mercury, iridium, xenon, aurum";

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
    let (full, suites) = match parse_args_from(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(msg) => {
            eprintln!("error: {msg}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (budget_ms, scale) = if full {
        (2000u64, 10u32)
    } else {
        (200u64, 1u32)
    };
    let opts = Opts {
        budget: Duration::from_millis(budget_ms),
        scale,
    };

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
