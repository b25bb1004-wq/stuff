#![forbid(unsafe_code)]
//! Command-line tool for the Athena Logos IDE services (`check`, `fmt`, `test`, `serve`).

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use aci::envelope::{self, Code, DEFAULT_LIMIT};
use aci::json::Json;
use aci::manifest::{Arg, Command as Entry, Effect, Kind, Manifest};
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
    serve [FOLDER] [--port N] [--web DIR]   (web DIR defaults: <bin>/../share/athena-web, then target/athena-web)
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

const FILE_ARGS: &[Arg] = &[Arg {
    name: "FILE...",
    kind: Kind::Path,
    required: true,
    default: None,
    choices: &[],
}];

const FMT_ARGS: &[Arg] = &[
    Arg {
        name: "FILE...",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--write",
        kind: Kind::Flag,
        required: false,
        default: None,
        choices: &[],
    },
];

const SERVE_ARGS: &[Arg] = &[
    Arg {
        name: "FOLDER",
        kind: Kind::Path,
        required: false,
        default: Some("."),
        choices: &[],
    },
    Arg {
        name: "--port",
        kind: Kind::Int,
        required: false,
        default: Some("8480"),
        choices: &[],
    },
    Arg {
        name: "--web",
        kind: Kind::Path,
        required: false,
        default: None,
        choices: &[],
    },
];

const COMMANDS: &[Entry] = &[
    Entry {
        name: "check",
        summary: "Parse and type-check Logos files; diagnostics are data, not failures",
        effect: Effect::Read,
        args: FILE_ARGS,
        returns: "diagnostics: [{path, line, col, severity, message}], count, clean",
        example: "athena --json check main.logos",
    },
    Entry {
        name: "fmt",
        summary: "Format Logos files; --write saves them in place",
        effect: Effect::Write,
        args: FMT_ARGS,
        returns: "items: [{path, changed}], total",
        example: "athena --json fmt main.logos",
    },
    Entry {
        name: "test",
        summary: "Run the unit tests and property tests of Logos files",
        effect: Effect::Read,
        args: FILE_ARGS,
        returns: "passed, failed, results: [{name, status, message}]",
        example: "athena --json test main.logos",
    },
    Entry {
        name: "serve",
        summary: "Serve FOLDER in the Athena IDE on 127.0.0.1 until stopped",
        effect: Effect::Net,
        args: SERVE_ARGS,
        returns: "listen, url (never returns: one envelope, then it keeps serving)",
        example: "athena --json serve ~/notes",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "athena",
    version: env!("CARGO_PKG_VERSION"),
    summary: "Logos checker, formatter, test runner and IDE server",
    commands: COMMANDS,
};

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
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    if envelope::take_describe_flag(&raw) {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    if raw.first().and_then(|a| a.to_str()) == Some("--json") {
        let rest: Vec<String> = raw[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        return run_json(&rest);
    }
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

/// Check that FOLDER is a folder, find the web app and start the server. Shared by the text and JSON paths.
#[cfg(not(target_arch = "wasm32"))]
fn serve_start(opts: &athena::serve::ServeArgs) -> Result<athena::serve::Served, (Code, String)> {
    // Checked first, so a bad folder is reported even when no web app is installed.
    if !Path::new(&opts.folder).is_dir() {
        return Err((Code::NotFound, format!("not a folder: {}", opts.folder)));
    }
    let exe = std::env::current_exe().ok();
    let candidates = athena::serve::web_candidates(opts.web.as_deref(), exe.as_deref());
    let web = athena::serve::find_web(&candidates).map_err(|msg| (Code::NotFound, msg))?;
    athena::serve::start(Path::new(&opts.folder), &web, opts.port).map_err(|msg| (Code::Io, msg))
}

/// Serve until the process is stopped (Ctrl-C, or kill by PID). The server runs on its own threads.
#[cfg(not(target_arch = "wasm32"))]
fn run_serve(opts: &athena::serve::ServeArgs) -> ExitCode {
    let served = match serve_start(opts) {
        Ok(s) => s,
        Err((_, msg)) => {
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

// ---- `athena --json` ------------------------------------------------------------------------

/// A failed `--json` run: the envelope code, message and hint, and the process exit code.
struct Failure {
    code: Code,
    message: String,
    hint: Option<String>,
    exit: u8,
}

const HINT_DESCRIBE: &str = "run: athena --describe";

fn usage(message: String) -> Failure {
    Failure {
        code: Code::Usage,
        message,
        hint: Some(HINT_DESCRIBE.into()),
        exit: 2,
    }
}

/// Read a source for `--json`; a missing file is `not_found`, the message is the text path's.
fn json_read(path: &str) -> Result<String, Failure> {
    read_source(path).map_err(|message| {
        let code = if path != "-" && !Path::new(path).exists() {
            Code::NotFound
        } else {
            Code::Io
        };
        Failure {
            code,
            message,
            hint: None,
            exit: 1,
        }
    })
}

/// FILE arguments of `check` and `test`: one or more, `-` for stdin.
fn file_args(args: &[String]) -> Result<Vec<String>, Failure> {
    let files: Vec<String> = args.iter().filter(|a| !is_flag(a)).cloned().collect();
    if let Some(flag) = args.iter().find(|a| is_flag(a)) {
        return Err(usage(format!("unknown option '{flag}'")));
    }
    if files.is_empty() {
        return Err(usage("missing FILE.logos argument".into()));
    }
    Ok(files)
}

fn path_json(path: &str) -> Json {
    Json::path(Path::new(path))
}

/// `athena --json check FILE...`: diagnostics are data, so found errors are still `ok: true` (exit 1).
fn json_check(args: &[String]) -> Result<(Json, i32), Failure> {
    let mut diagnostics = Vec::new();
    for file in file_args(args)? {
        let src = json_read(&file)?;
        for d in service::check(&src) {
            diagnostics.push(Json::Obj(vec![
                ("path".into(), path_json(&file)),
                ("line".into(), Json::Int(i64::from(d.line))),
                ("col".into(), Json::Int(i64::from(d.col))),
                ("severity".into(), Json::Str("error".into())),
                ("message".into(), Json::Str(d.message)),
            ]));
        }
    }
    let count = diagnostics.len();
    let data = Json::Obj(vec![
        ("diagnostics".into(), Json::Arr(diagnostics)),
        ("count".into(), Json::Int(count as i64)),
        ("clean".into(), Json::Bool(count == 0)),
    ]);
    Ok((data, if count == 0 { 0 } else { 1 }))
}

/// `athena --json test FILE...`: every test of every file, with the text path's pass and fail counts.
fn json_test(args: &[String]) -> Result<(Json, i32), Failure> {
    let (mut passed, mut failed) = (0i64, 0i64);
    let mut results = Vec::new();
    for file in file_args(args)? {
        let src = json_read(&file)?;
        let outcomes = service::run_tests(&src, DEFAULT_FUEL).map_err(|diags| {
            let message = diags
                .iter()
                .map(|d| format!("{file}:{}:{}: {}", d.line, d.col, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            Failure {
                code: Code::Invalid,
                message,
                hint: None,
                exit: 1,
            }
        })?;
        for r in outcomes {
            let status = if r.passed {
                passed += 1;
                "passed"
            } else {
                failed += 1;
                "failed"
            };
            results.push(Json::Obj(vec![
                ("name".into(), Json::Str(r.name)),
                ("status".into(), Json::Str(status.into())),
                ("message".into(), Json::Str(r.detail)),
            ]));
        }
    }
    let data = Json::Obj(vec![
        ("passed".into(), Json::Int(passed)),
        ("failed".into(), Json::Int(failed)),
        ("results".into(), Json::Arr(results)),
    ]);
    Ok((data, if failed > 0 { 1 } else { 0 }))
}

/// `athena --json fmt [--write] FILE...`: per file whether formatting changes it; `--write` saves it.
fn json_fmt(args: &[String]) -> Result<(Json, i32), Failure> {
    let mut write = false;
    let mut files = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--write" => write = true,
            _ if is_flag(arg) => return Err(usage(format!("unknown option '{arg}'"))),
            _ => files.push(arg.clone()),
        }
    }
    if files.is_empty() {
        return Err(usage("missing FILE.logos argument".into()));
    }
    if write && files.iter().any(|f| f == "-") {
        return Err(usage("cannot use --write with '-'".into()));
    }
    let mut items = Vec::new();
    for file in &files {
        let src = json_read(file)?;
        let formatted = lang::format(&src).map_err(|message| Failure {
            code: Code::Invalid,
            message: format!("{file}: {message}"),
            hint: None,
            exit: 1,
        })?;
        let changed = formatted != src;
        if write {
            fs::write(file, formatted.as_bytes()).map_err(|e| Failure {
                code: Code::Io,
                message: format!("{file}: {e}"),
                hint: None,
                exit: 1,
            })?;
        }
        items.push(Json::Obj(vec![
            ("path".into(), path_json(file)),
            ("changed".into(), Json::Bool(changed)),
        ]));
    }
    let total = items.len();
    Ok((envelope::page(items, total, 0, DEFAULT_LIMIT), 0))
}

/// `athena --json serve FOLDER`: one envelope once the listener is bound, then it keeps serving.
#[cfg(not(target_arch = "wasm32"))]
fn json_serve(args: &[String]) -> ExitCode {
    let opts = match athena::serve::parse_serve_args(args) {
        Ok(Some(opts)) => opts,
        Ok(None) => {
            return finish(
                "serve",
                Err(usage("serve --help is not available under --json".into())),
            )
        }
        Err(msg) => return finish("serve", Err(usage(msg))),
    };
    let served = match serve_start(&opts) {
        Ok(s) => s,
        Err((code, message)) => {
            return finish(
                "serve",
                Err(Failure {
                    code,
                    message,
                    hint: None,
                    exit: 1,
                }),
            )
        }
    };
    let data = Json::Obj(vec![
        ("listen".into(), Json::Str(served.listen.clone())),
        ("url".into(), Json::Str(served.url.clone())),
    ]);
    print!("{}", envelope::ok("athena", "serve", data));
    let _ = io::stdout().flush();
    // The server runs on its own threads; this keeps `served` alive until the process is stopped.
    loop {
        std::thread::park();
    }
}

/// Print the envelope for `cmd` (one line on stdout, the human line on stderr) and return its exit code.
fn finish(cmd: &str, result: Result<(Json, i32), Failure>) -> ExitCode {
    let exit = match result {
        Ok((data, exit)) => {
            print!("{}", envelope::ok("athena", cmd, data));
            u8::try_from(exit).unwrap_or(1)
        }
        Err(f) => {
            eprintln!("athena: {}", f.message);
            print!(
                "{}",
                envelope::err("athena", cmd, f.code, &f.message, f.hint.as_deref())
            );
            f.exit
        }
    };
    let _ = io::stdout().flush();
    ExitCode::from(exit)
}

/// Run `athena --json ARGS...` (`args` are the arguments after `--json`).
fn run_json(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("check") => finish("check", json_check(&args[1..])),
        Some("fmt") => finish("fmt", json_fmt(&args[1..])),
        Some("test") => finish("test", json_test(&args[1..])),
        #[cfg(not(target_arch = "wasm32"))]
        Some("serve") => json_serve(&args[1..]),
        Some(word) => finish("", Err(usage(format!("unknown subcommand '{word}'")))),
        None => finish("", Err(usage("missing subcommand".into()))),
    }
}
