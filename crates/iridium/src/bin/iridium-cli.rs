//! Command-line SQL client for a Iridium server.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{self, BufRead, Write};
use std::net::ToSocketAddrs;
use std::process;

use aci::envelope::Code;
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind, Manifest};
use iridium::net::Client;
use iridium::report::{self, Failure, Paging};
use iridium::sql::Value;

const ADDR_ARGS: &[Arg] = &[Arg {
    name: "HOST:PORT",
    kind: Kind::Text,
    required: true,
    default: None,
    choices: &[],
}];

const COMMANDS: &[Command] = &[Command {
    name: "connect",
    summary: "Run statements from stdin on the iridium server at HOST:PORT",
    effect: Effect::Net,
    args: ADDR_ARGS,
    returns: "statements: one entry per statement in order; a SELECT is {kind, columns: [{name, type}], rows, total, more, next_offset (when more)}, anything else {kind, count}",
    example: "iridium-cli --json 127.0.0.1:7432",
}];

const MANIFEST: Manifest = Manifest {
    tool: "iridium-cli",
    version: "0.1.0",
    summary: "Command-line SQL client for an iridium server",
    commands: COMMANDS,
};

/// `iridium-cli --json HOST:PORT`: the same statements shape as `iridium --json connect`.
fn run_json(args: &[String]) -> i32 {
    let result = match args {
        [target] => report::read_statements(io::stdin().lock())
            .and_then(|stmts| report::run_remote(target, &stmts, &Paging::default()))
            .map(|entries| Json::obj([("statements", Json::Arr(entries))])),
        _ => Err(
            Failure::new(Code::Usage, "usage: iridium-cli --json <host:port>")
                .hint("run: iridium-cli --describe"),
        ),
    };
    report::finish("iridium-cli", "connect", result)
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

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let Some(target) = args.next() else {
        return Err("usage: iridium-cli <host:port>".to_owned());
    };
    if target == "--help" || target == "-h" {
        println!("usage: iridium-cli <host:port>");
        return Ok(());
    }
    if args.next().is_some() {
        return Err("usage: iridium-cli <host:port>".to_owned());
    }

    let addr = target
        .to_socket_addrs()
        .map_err(|e| format!("invalid address '{target}': {e}"))?
        .next()
        .ok_or_else(|| format!("could not resolve '{target}'"))?;

    let mut client = Client::connect(addr).map_err(|e| format!("connect failed: {e}"))?;

    let stdin = io::stdin();
    for line_res in stdin.lock().lines() {
        let line = line_res.map_err(|e| format!("stdin read failed: {e}"))?;
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
                return Err(format!("connection error: {e}"));
            }
        }
    }
    Ok(())
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
    if let Err(msg) = run() {
        eprintln!("{msg}");
        process::exit(1);
    }
}
