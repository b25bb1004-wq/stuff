#![forbid(unsafe_code)]
//! Command-line tool for the Athena Logos IDE services (`check`, `fmt`, `test`).

use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use athena::lang;
use athena::service;

const VERSION: &str = "athena 0.1.0";
const DEFAULT_FUEL: u64 = 1_000_000;

const USAGE: &str = "\
athena - Logos checker, formatter, and test runner

USAGE:
    athena <SUBCOMMAND> [OPTIONS] <FILE.logos | ->

SUBCOMMANDS:
    check FILE.logos           Parse and type-check Logos source
    fmt FILE.logos [--write]   Format Logos source (refuses files with comments)
    test FILE.logos            Run unit tests and property tests
    serve [FOLDER] [--port N] [--web DIR]
                               Open FOLDER in the Athena IDE on 127.0.0.1 (default port 8480)

OPTIONS:
    -h, --help                 Print this help message
    -V, --version              Print version information

EXAMPLES:
    athena check main.logos
    athena fmt main.logos --write
    athena test main.logos
    athena serve ~/notes
";

enum Command {
    Help,
    Version,
    Check {
        path: String,
    },
    Fmt {
        path: String,
        write: bool,
    },
    Test {
        path: String,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Serve(athena::serve::ServeArgs),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(cmd) => run(cmd),
        Err(msg) => {
            eprintln!("athena: {msg}");
            ExitCode::from(2)
        }
    }
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(first) = args.first() else {
        return Err(String::from("missing subcommand; try 'athena --help'"));
    };
    match first.as_str() {
        "-h" | "--help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "check" => match parse_single_file(&args[1..])? {
            Some(path) => Ok(Command::Check { path }),
            None => Ok(Command::Help),
        },
        "fmt" => parse_fmt_args(&args[1..]),
        "test" => match parse_single_file(&args[1..])? {
            Some(path) => Ok(Command::Test { path }),
            None => Ok(Command::Help),
        },
        #[cfg(not(target_arch = "wasm32"))]
        "serve" => match athena::serve::parse_serve_args(&args[1..])? {
            Some(opts) => Ok(Command::Serve(opts)),
            None => Ok(Command::Help),
        },
        other if is_flag(other) => Err(format!("unknown option '{other}'; try 'athena --help'")),
        other => Err(format!("unknown subcommand '{other}'; try 'athena --help'")),
    }
}

fn is_flag(arg: &str) -> bool {
    arg.starts_with('-') && arg != "-"
}

fn parse_single_file(args: &[String]) -> Result<Option<String>, String> {
    let mut path: Option<String> = None;
    let mut literal = false;
    for arg in args {
        if !literal && (arg == "-h" || arg == "--help") {
            return Ok(None);
        }
        if !literal && arg == "--" {
            literal = true;
            continue;
        }
        if !literal && is_flag(arg) {
            return Err(format!("unknown option '{arg}'"));
        }
        if path.is_some() {
            return Err(format!("unexpected argument '{arg}'"));
        }
        path = Some(arg.clone());
    }
    path.map(Some)
        .ok_or_else(|| String::from("missing FILE.logos argument"))
}

fn parse_fmt_args(args: &[String]) -> Result<Command, String> {
    let mut path: Option<String> = None;
    let mut write = false;
    let mut literal = false;
    for arg in args {
        if !literal && (arg == "-h" || arg == "--help") {
            return Ok(Command::Help);
        }
        if !literal && arg == "--" {
            literal = true;
            continue;
        }
        if !literal && arg == "--write" {
            write = true;
            continue;
        }
        if !literal && is_flag(arg) {
            return Err(format!("unknown option '{arg}'"));
        }
        if path.is_some() {
            return Err(format!("unexpected argument '{arg}'"));
        }
        path = Some(arg.clone());
    }
    let path = path.ok_or_else(|| String::from("missing FILE.logos argument"))?;
    if write && path == "-" {
        return Err(String::from("cannot use --write with '-'"));
    }
    Ok(Command::Fmt { path, write })
}

fn run(cmd: Command) -> ExitCode {
    match cmd {
        Command::Help => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Command::Check { path } => run_check(&path),
        Command::Fmt { path, write } => run_fmt(&path, write),
        Command::Test { path } => run_test(&path),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Serve(opts) => run_serve(&opts),
    }
}

/// Serve until the process is stopped (Ctrl-C, or kill by PID). The server runs on its own threads.
#[cfg(not(target_arch = "wasm32"))]
fn run_serve(opts: &athena::serve::ServeArgs) -> ExitCode {
    let served = match athena::serve::start(
        std::path::Path::new(&opts.folder),
        std::path::Path::new(&opts.web),
        opts.port,
    ) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("athena: {msg}");
            return ExitCode::from(1);
        }
    };
    println!("{}", served.url);
    eprintln!(
        "athena: serving {} on 127.0.0.1; Ctrl-C stops it",
        opts.folder
    );
    loop {
        std::thread::park();
    }
}

fn read_source(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("-: {e}"))?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
    }
}

fn print_diagnostics(path: &str, diags: &[service::Diagnostic]) {
    for d in diags {
        eprintln!("{path}:{}:{}: error: {}", d.line, d.col, d.message);
    }
}

fn run_check(path: &str) -> ExitCode {
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("athena: {e}");
            return ExitCode::from(1);
        }
    };
    let diags = service::check(&src);
    if diags.is_empty() {
        return ExitCode::SUCCESS;
    }
    print_diagnostics(path, &diags);
    eprintln!("athena: check failed ({} error(s))", diags.len());
    ExitCode::from(1)
}

fn run_fmt(path: &str, write: bool) -> ExitCode {
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("athena: {e}");
            return ExitCode::from(1);
        }
    };
    let formatted = match lang::format(&src) {
        Ok(f) => f,
        Err(msg) => {
            eprintln!("athena: {path}: {msg}");
            return ExitCode::from(1);
        }
    };
    if write {
        if let Err(e) = fs::write(path, formatted.as_bytes()) {
            eprintln!("athena: {path}: {e}");
            return ExitCode::from(1);
        }
    } else if let Err(e) = io::stdout().write_all(formatted.as_bytes()) {
        eprintln!("athena: stdout: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run_test(path: &str) -> ExitCode {
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("athena: {e}");
            return ExitCode::from(1);
        }
    };
    let results = match service::run_tests(&src, DEFAULT_FUEL) {
        Ok(r) => r,
        Err(diags) => {
            print_diagnostics(path, &diags);
            eprintln!("athena: test failed ({} error(s))", diags.len());
            return ExitCode::from(1);
        }
    };
    let mut passed = 0usize;
    let mut failed = 0usize;
    for r in &results {
        if r.passed {
            passed = passed.saturating_add(1);
            let detail = r.detail.trim();
            if detail.is_empty() {
                println!("PASS {}", r.name);
            } else {
                println!("PASS {} ({detail})", r.name);
            }
        } else {
            failed = failed.saturating_add(1);
            println!("FAIL {}: {}", r.name, r.detail);
        }
    }
    println!("summary: {passed} passed, {failed} failed");
    if failed > 0 {
        eprintln!("athena: {failed} of {} test(s) failed", results.len());
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
