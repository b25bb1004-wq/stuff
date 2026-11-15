//! Command-line SQL client for a Iridium server.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{self, BufRead};
use std::net::ToSocketAddrs;
use std::process;

use iridium::net::Client;
use iridium::sql::Value;

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

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let Some(target) = args.next() else {
        return Err("usage: iridium-cli <host:port>".to_owned());
    };
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
    if let Err(msg) = run() {
        eprintln!("{msg}");
        process::exit(1);
    }
}
