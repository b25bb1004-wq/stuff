//! Command-line tool for local and networked Iridium SQL databases.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{self, BufRead, BufWriter, Write};
use std::net::ToSocketAddrs;
use std::process;

use aci::envelope::Code;
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use iridium::net::Client;
use iridium::report::{self, split_line_statements, Failure};
use iridium::sql::{Db, Rows, Value};
use iridium::FileStorage;

const VERSION: &str = "iridium 0.1.0";

const SQL_ARGS: &[Arg] = &[
    Arg {
        name: "DB",
        kind: Kind::Path,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "QUERY",
        kind: Kind::Text,
        required: false,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--limit",
        kind: Kind::Int,
        required: false,
        default: Some("100"),
        choices: &[],
    },
    Arg {
        name: "--offset",
        kind: Kind::Int,
        required: false,
        default: Some("0"),
        choices: &[],
    },
];

const CONNECT_ARGS: &[Arg] = &[
    Arg {
        name: "ADDR",
        kind: Kind::Text,
        required: true,
        default: None,
        choices: &[],
    },
    Arg {
        name: "--limit",
        kind: Kind::Int,
        required: false,
        default: Some("100"),
        choices: &[],
    },
    Arg {
        name: "--offset",
        kind: Kind::Int,
        required: false,
        default: Some("0"),
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[
    Command {
        name: "sql",
        summary: "Open or create DB and run QUERY (or statements from stdin if omitted or '-')",
        effect: Effect::Write,
        args: SQL_ARGS,
        returns: "statements: one entry per statement in order; a SELECT is {kind, columns: [{name, type}], rows, total, more, next_offset (when more)}, anything else {kind, count}",
        example: "iridium --json sql app.idb \"SELECT 1 + 2 AS sum\"",
    },
    Command {
        name: "connect",
        summary: "Run statements from stdin on the iridium server at host:port",
        effect: Effect::Net,
        args: CONNECT_ARGS,
        returns: "statements: as for sql; column types are \"\" over the network",
        example: "iridium --json connect 127.0.0.1:7432",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "iridium",
    version: "0.1.0",
    summary: "SQL database engine CLI",
    commands: COMMANDS,
};

const HELP_TEXT: &str = "\
iridium - SQL database engine CLI

USAGE:
    iridium <COMMAND> [ARGS]

COMMANDS:
    sql DB [QUERY]    Open or create DB and run QUERY (or statements from stdin if omitted or '-')
    connect ADDR      Connect to an iridium server at host:port and run statements from stdin

OPTIONS:
    -h, --help        Print help information
    -V, --version     Print version information

EXAMPLES:
    iridium sql app.idb \"SELECT 1 + 2 AS sum\"
    iridium connect 127.0.0.1:7432";

enum CliError {
    Usage(String),
    Op(String),
}

fn format_value(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_owned(),
        Value::Int(n) => n.to_string(),
        Value::Real(f) => {
            let s = format!("{f}");
            if f.is_finite() && !s.contains(['.', 'e', 'E']) {
                format!("{s}.0")
            } else {
                s
            }
        }
        Value::Text(s) => s.clone(),
        Value::Blob(bytes) => {
            let mut out = String::with_capacity(3 + bytes.len() * 2);
            out.push_str("x'");
            for b in bytes {
                let _ = write!(out, "{b:02x}");
            }
            out.push('\'');
            out
        }
    }
}

fn write_tsv(out: &mut impl Write, rows: &Rows) -> Result<(), CliError> {
    if rows.columns.is_empty() {
        return Ok(());
    }
    writeln!(out, "{}", rows.columns.join("\t"))
        .map_err(|e| CliError::Op(format!("stdout write failed: {e}")))?;
    for row in &rows.rows {
        let formatted: Vec<String> = row.iter().map(format_value).collect();
        writeln!(out, "{}", formatted.join("\t"))
            .map_err(|e| CliError::Op(format!("stdout write failed: {e}")))?;
    }
    Ok(())
}

/// Run every `;`-separated statement in `text`, printing each result; stops at the first error.
fn run_text(db: &mut Db<FileStorage>, out: &mut impl Write, text: &str) -> Result<(), CliError> {
    for stmt in split_line_statements(text) {
        let rows = db.execute(stmt).map_err(|e| CliError::Op(e.to_string()))?;
        write_tsv(out, &rows)?;
    }
    Ok(())
}

fn run_stdin(db: &mut Db<FileStorage>, out: &mut impl Write) -> Result<(), CliError> {
    for line_res in io::stdin().lock().lines() {
        let line = line_res.map_err(|e| CliError::Op(format!("stdin read failed: {e}")))?;
        run_text(db, out, &line)?;
    }
    Ok(())
}

fn run_sql(args: &[String]) -> Result<(), CliError> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{HELP_TEXT}");
        return Ok(());
    }
    if args.is_empty() || args.len() > 2 {
        return Err(CliError::Usage("usage: iridium sql DB [QUERY]".to_owned()));
    }
    let db_arg = &args[0];
    if db_arg == "-" {
        return Err(CliError::Op("database path cannot be '-'".to_owned()));
    }
    let query_arg = args.get(1).map(String::as_str);

    // The lock is held until this function returns, so no other process can touch the files meanwhile.
    let (_lock, mut db) = report::open_db(db_arg).map_err(|(_, msg)| CliError::Op(msg))?;

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let exec_res = match query_arg {
        Some(q) if q != "-" => run_text(&mut db, &mut out, q),
        _ => run_stdin(&mut db, &mut out),
    };
    // close() would silently roll back a BEGIN left open; report it so exit 0 means all committed.
    let exec_res = exec_res.and_then(|()| {
        if db.in_transaction() {
            Err(CliError::Op(
                "transaction not committed, rolled back (end it with COMMIT)".to_owned(),
            ))
        } else {
            Ok(())
        }
    });

    let flush_res = out
        .flush()
        .map_err(|e| CliError::Op(format!("stdout flush failed: {e}")));
    let close_res = db
        .close()
        .map_err(|e| CliError::Op(format!("close failed: {e}")));

    exec_res?;
    flush_res?;
    close_res?;
    Ok(())
}

fn run_connect(args: &[String]) -> Result<(), CliError> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{HELP_TEXT}");
        return Ok(());
    }
    if args.len() != 1 {
        return Err(CliError::Usage("usage: iridium connect ADDR".to_owned()));
    }
    let target = &args[0];

    let addr = target
        .to_socket_addrs()
        .map_err(|e| CliError::Op(format!("invalid address '{target}': {e}")))?
        .next()
        .ok_or_else(|| CliError::Op(format!("could not resolve '{target}'")))?;

    let mut client =
        Client::connect(addr).map_err(|e| CliError::Op(format!("connect failed: {e}")))?;

    let stdin = io::stdin();
    for line_res in stdin.lock().lines() {
        let line = line_res.map_err(|e| CliError::Op(format!("stdin read failed: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        match client.execute(&line) {
            Ok(Ok(rows)) => {
                for row in &rows.rows {
                    let formatted: Vec<String> = row.iter().map(format_value).collect();
                    println!("{}", formatted.join("\t"));
                }
                println!("ok {}", rows.changes);
            }
            Ok(Err(msg)) => {
                println!("error: {msg}");
            }
            Err(e) => {
                return Err(CliError::Op(format!("connection error: {e}")));
            }
        }
    }
    Ok(())
}

fn run() -> Result<(), CliError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        return Err(CliError::Usage(
            "missing subcommand (try --help)".to_owned(),
        ));
    };

    match cmd.as_str() {
        "-h" | "--help" => {
            println!("{HELP_TEXT}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        "sql" => run_sql(&args[1..]),
        "connect" => run_connect(&args[1..]),
        other => Err(CliError::Usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

fn statements(entries: Vec<Json>) -> Json {
    Json::obj([("statements", Json::Arr(entries))])
}

fn sql_json(args: &[String]) -> Result<Json, Failure> {
    let (pos, paging) = report::split_paging(args)?;
    if pos.is_empty() || pos.len() > 2 {
        return Err(Failure::new(
            Code::Usage,
            "usage: iridium --json sql DB [QUERY] [--limit N] [--offset N]",
        )
        .hint("run: iridium --describe"));
    }
    let db_arg = &pos[0];
    if db_arg == "-" {
        return Err(Failure::new(Code::Invalid, "database path cannot be '-'"));
    }
    let stmts = match pos.get(1).map(String::as_str) {
        Some(query) if query != "-" => split_line_statements(query)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        _ => report::read_statements(io::stdin().lock())?,
    };
    let (_lock, mut db) = report::open_db(db_arg).map_err(|(code, msg)| Failure::new(code, msg))?;
    let run = report::run_local(&mut db, &stmts, &paging);
    // Closed even after a failure, as text mode does, so the log is checkpointed.
    let closed = db
        .close()
        .map_err(|e| Failure::new(report::engine_code(&e), format!("close failed: {e}")));
    let entries = run?;
    closed?;
    Ok(statements(entries))
}

fn connect_json(args: &[String]) -> Result<Json, Failure> {
    let (pos, paging) = report::split_paging(args)?;
    if pos.len() != 1 {
        return Err(Failure::new(
            Code::Usage,
            "usage: iridium --json connect ADDR [--limit N] [--offset N]",
        )
        .hint("run: iridium --describe"));
    }
    let stmts = report::read_statements(io::stdin().lock())?;
    Ok(statements(report::run_remote(&pos[0], &stmts, &paging)?))
}

/// Run one `--json` invocation (the arguments after `--json`) and return the exit code.
fn run_json(args: &[String]) -> i32 {
    let (cmd, result) = match args.first().map(String::as_str) {
        None => (
            "",
            Err(Failure::new(Code::Usage, "missing subcommand (try --help)")
                .hint("run: iridium --describe")),
        ),
        Some("sql") => ("sql", sql_json(&args[1..])),
        Some("connect") => ("connect", connect_json(&args[1..])),
        Some("-V" | "--version") => (
            "version",
            Ok(Json::obj([("version", Json::Str("0.1.0".into()))])),
        ),
        Some("-h" | "--help") => (
            "help",
            Ok(Json::obj([("help", Json::Str(HELP_TEXT.into()))])),
        ),
        Some(other) => (
            "",
            Err(Failure::new(
                Code::Usage,
                format!("unknown subcommand '{other}' (try --help)"),
            )
            .hint("run: iridium --describe")),
        ),
    };
    report::finish("iridium", cmd, result)
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.len() == 1 && raw[0] == "--describe" {
        println!("{}", MANIFEST.to_json().write());
        return;
    }
    if raw.first().map(String::as_str) == Some("--json") {
        let code = run_json(&raw[1..]);
        let _ = io::stdout().flush();
        process::exit(code);
    }
    if let Err(err) = run() {
        match err {
            CliError::Usage(msg) => {
                eprintln!("iridium: {msg}");
                process::exit(2);
            }
            CliError::Op(msg) => {
                eprintln!("iridium: {msg}");
                process::exit(1);
            }
        }
    }
}
