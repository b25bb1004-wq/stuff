//! Command-line runner for Logos (`logos run` and `logos test`).
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

const DEFAULT_FUEL: u64 = 100_000_000;
const DEFAULT_SEED: u64 = 1;
const DEFAULT_CASES: u32 = 100;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        eprintln!("usage: logos <run|test> FILE [options]");
        process::exit(2);
    };
    match cmd {
        "run" => cmd_run(&args[1..]),
        "test" => cmd_test(&args[1..]),
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
