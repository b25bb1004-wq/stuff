//! Command-line tool for local and networked Iridium SQL databases.
#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, BufRead, BufWriter, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process;

use iridium::net::Client;
use iridium::sql::{Db, Rows, Value};
use iridium::FileStorage;

const VERSION: &str = "iridium 0.1.0";

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

fn wal_path(db_path: &Path) -> PathBuf {
    let mut os: OsString = db_path.as_os_str().to_os_string();
    os.push("-wal");
    PathBuf::from(os)
}

fn split_line_statements(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut stmts = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_block_comment {
            if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_single {
            if b == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                } else {
                    in_single = false;
                    i += 1;
                }
            } else {
                i += 1;
            }
            continue;
        }
        if in_double {
            if b == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 2;
                } else {
                    in_double = false;
                    i += 1;
                }
            } else {
                i += 1;
            }
            continue;
        }
        if b == b'-' && bytes.get(i + 1) == Some(&b'-') {
            break;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if b == b'\'' {
            in_single = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double = true;
            i += 1;
            continue;
        }
        if b == b';' {
            let chunk = line[start..i].trim();
            if !chunk.is_empty() {
                stmts.push(chunk);
            }
            i += 1;
            start = i;
            continue;
        }
        i += 1;
    }

    if start < line.len() {
        let tail = line[start..].trim();
        if !tail.is_empty() && !tail.starts_with("--") {
            stmts.push(tail);
        }
    }
    stmts
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

    let db_path = Path::new(db_arg);
    let main_store = FileStorage::open(db_path)
        .map_err(|e| CliError::Op(format!("cannot open '{db_arg}': {e}")))?;
    let log_path = wal_path(db_path);
    let log_store = FileStorage::open(&log_path)
        .map_err(|e| CliError::Op(format!("cannot open '{}': {e}", log_path.display())))?;
    let mut db = Db::open(main_store, log_store)
        .map_err(|e| CliError::Op(format!("cannot open '{db_arg}': {e}")))?;

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let exec_res = match query_arg {
        Some(q) if q != "-" => match db.execute(q) {
            Ok(rows) => write_tsv(&mut out, &rows),
            Err(e) => Err(CliError::Op(e.to_string())),
        },
        _ => {
            let stdin = io::stdin();
            let mut res = Ok(());
            for line_res in stdin.lock().lines() {
                let line = match line_res {
                    Ok(l) => l,
                    Err(e) => {
                        res = Err(CliError::Op(format!("stdin read failed: {e}")));
                        break;
                    }
                };
                for stmt in split_line_statements(&line) {
                    match db.execute(stmt) {
                        Ok(rows) => {
                            if let Err(e) = write_tsv(&mut out, &rows) {
                                res = Err(e);
                                break;
                            }
                        }
                        Err(e) => {
                            res = Err(CliError::Op(e.to_string()));
                            break;
                        }
                    }
                }
                if res.is_err() {
                    break;
                }
            }
            res
        }
    };

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

fn main() {
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
