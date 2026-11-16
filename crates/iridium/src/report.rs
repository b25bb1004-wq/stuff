//! The `--json` side of the iridium binaries: statement results as ACI envelope data (docs/ai-computer/ACI.md).
//!
//! Local runs and network runs both build their entries with `statement_entry`, so a SELECT looks the same
//! wherever it ran. The text printers in the binaries do not use this module.

use std::fmt::Write as _;
use std::io::BufRead;
use std::net::ToSocketAddrs;
use std::path::Path;

use aci::envelope::{self, Code, DEFAULT_LIMIT, MAX_LIMIT};
use aci::json::Json;

use crate::net::Client;
use crate::sql::{statement_kind, Db, Rows, Value};
use crate::{lock_database, DbLock, Error, FileStorage};

/// A failed `--json` run: the envelope code, the message and an optional hint.
pub struct Failure {
    /// The error code of the envelope.
    pub code: Code,
    /// What went wrong, with the statement number when a statement failed.
    pub message: String,
    /// What to run or change next.
    pub hint: Option<String>,
}

impl Failure {
    /// A failure with no hint.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    /// The same failure with a hint attached.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Paging for SELECT results: `rows` holds at most `limit` rows starting at `offset`.
pub struct Paging {
    /// Most rows per SELECT.
    pub limit: usize,
    /// First row returned from each SELECT.
    pub offset: usize,
}

impl Default for Paging {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

/// Split the arguments into positional ones and the `--limit N` and `--offset N` flags, which may come anywhere.
pub fn split_paging(args: &[String]) -> Result<(Vec<String>, Paging), Failure> {
    let usage = |msg: String| Failure::new(Code::Usage, msg).hint("run: iridium --describe");
    let mut rest = Vec::new();
    let mut paging = Paging::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg != "--limit" && arg != "--offset" {
            rest.push(arg.clone());
            continue;
        }
        let value = it
            .next()
            .ok_or_else(|| usage(format!("{arg} needs a number")))?;
        let n: usize = value
            .parse()
            .map_err(|_| usage(format!("{arg} needs a whole number, got '{value}'")))?;
        if arg == "--limit" {
            if n == 0 || n > MAX_LIMIT {
                return Err(usage(format!("--limit must be from 1 to {MAX_LIMIT}")));
            }
            paging.limit = n;
        } else {
            paging.offset = n;
        }
    }
    Ok((rest, paging))
}

/// Split one line of SQL at its top-level `;`, ignoring quotes and comments. A chunk of only comments is dropped.
pub fn split_line_statements(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut stmts = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_block_comment = false;
    let mut has_code = false;

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
            // A line comment ends at the newline, so later lines of a QUERY argument still run.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if !b.is_ascii_whitespace() && b != b';' {
            has_code = true;
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
            // A chunk of only comments is not a statement; it is dropped, not sent to the parser.
            if has_code {
                stmts.push(line[start..i].trim());
            }
            has_code = false;
            i += 1;
            start = i;
            continue;
        }
        i += 1;
    }

    if has_code {
        stmts.push(line[start..].trim());
    }
    stmts
}

/// Every statement read from `input`, line by line, in order.
pub fn read_statements(input: impl BufRead) -> Result<Vec<String>, Failure> {
    let mut out = Vec::new();
    for line in input.lines() {
        let line = line.map_err(|e| Failure::new(Code::Io, format!("stdin read failed: {e}")))?;
        out.extend(split_line_statements(&line).into_iter().map(str::to_owned));
    }
    Ok(out)
}

/// The error code for an engine error.
pub fn engine_code(e: &Error) -> Code {
    match e {
        Error::Io | Error::Corrupt => Code::Io,
        Error::Locked | Error::Busy => Code::Locked,
        Error::NoSuchTable | Error::NoSuchColumn => Code::NotFound,
        Error::Exists => Code::Exists,
        _ => Code::Invalid,
    }
}

/// The error code for an error that arrived over the wire as text.
fn remote_code(msg: &str) -> Code {
    // shortcut: the wire carries the error text, not its code; upgrade by sending the code in net/codec.rs.
    if msg.starts_with("no such") {
        Code::NotFound
    } else if msg.contains("locked") {
        Code::Locked
    } else {
        Code::Invalid
    }
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Int(n) => Json::Int(*n),
        Value::Real(f) => Json::Num(*f),
        Value::Text(s) => Json::Str(s.clone()),
        Value::Blob(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for b in bytes {
                let _ = write!(hex, "{b:02x}");
            }
            Json::Str(hex)
        }
    }
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn object(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// One entry of `data.statements`. `kind` is the statement's kind word; `types` gives each column's declared type.
pub fn statement_entry(kind: &str, rows: &Rows, types: &[&str], paging: &Paging) -> Json {
    if kind != "select" && kind != "explain" {
        return object(vec![
            ("kind", Json::Str(kind.to_owned())),
            (
                "count",
                int(usize::try_from(rows.changes).unwrap_or(usize::MAX)),
            ),
        ]);
    }
    let total = rows.rows.len();
    let start = paging.offset.min(total);
    let end = start.saturating_add(paging.limit).min(total);
    let more = end < total;
    let columns = rows
        .columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let ty = types.get(i).copied().unwrap_or("");
            object(vec![
                ("name", Json::Str(name.clone())),
                ("type", Json::Str(ty.to_owned())),
            ])
        })
        .collect();
    let page = rows.rows[start..end]
        .iter()
        .map(|row| Json::Arr(row.iter().map(value_json).collect()))
        .collect();
    let mut pairs = vec![
        ("kind", Json::Str(kind.to_owned())),
        ("columns", Json::Arr(columns)),
        ("rows", Json::Arr(page)),
        ("total", int(total)),
        ("more", Json::Bool(more)),
    ];
    if more {
        pairs.push(("next_offset", int(end)));
    }
    object(pairs)
}

/// Run each statement against `db` in order and stop at the first failure. Statements before it have run.
pub fn run_local(
    db: &mut Db<FileStorage>,
    stmts: &[String],
    paging: &Paging,
) -> Result<Vec<Json>, Failure> {
    let total = stmts.len();
    let mut out = Vec::with_capacity(total);
    for (i, stmt) in stmts.iter().enumerate() {
        let fail = |e: Error| {
            Failure::new(
                engine_code(&e),
                format!("statement {} of {total}: {e}", i + 1),
            )
        };
        let described = db.describe(stmt).map_err(fail)?;
        let rows = db.execute(stmt).map_err(fail)?;
        out.push(statement_entry(
            described.kind,
            &rows,
            &described.types,
            paging,
        ));
    }
    // close() would silently roll back a BEGIN left open; report it so ok means all committed.
    if db.in_transaction() {
        return Err(Failure::new(
            Code::Invalid,
            "transaction not committed, rolled back (end it with COMMIT)",
        ));
    }
    Ok(out)
}

/// Open the database at `db_arg` under its lock. The error pairs an envelope code with the text-mode message.
pub fn open_db(db_arg: &str) -> Result<(DbLock, Db<FileStorage>), (Code, String)> {
    let db_path = Path::new(db_arg);
    // Held by the caller until it returns, so no other process can touch the files meanwhile.
    let lock = lock_database(db_path).map_err(|e| (engine_code(&e), e.to_string()))?;
    let main_store = FileStorage::open(db_path)
        .map_err(|e| (engine_code(&e), format!("cannot open '{db_arg}': {e}")))?;
    let log_path = {
        let mut os = db_path.as_os_str().to_os_string();
        os.push("-wal");
        std::path::PathBuf::from(os)
    };
    let log_store = FileStorage::open(&log_path).map_err(|e| {
        (
            engine_code(&e),
            format!("cannot open '{}': {e}", log_path.display()),
        )
    })?;
    let db = Db::open(main_store, log_store)
        .map_err(|e| (engine_code(&e), format!("cannot open '{db_arg}': {e}")))?;
    Ok((lock, db))
}

/// Send each statement to the iridium server at `target` in order. Statements before a failure have run.
pub fn run_remote(target: &str, stmts: &[String], paging: &Paging) -> Result<Vec<Json>, Failure> {
    let addr = target
        .to_socket_addrs()
        .map_err(|e| Failure::new(Code::Invalid, format!("invalid address '{target}': {e}")))?
        .next()
        .ok_or_else(|| Failure::new(Code::Invalid, format!("could not resolve '{target}'")))?;
    let mut client = Client::connect(addr).map_err(|e| {
        Failure::new(Code::Io, format!("connect failed: {e}")).hint(format!(
            "no iridium server answered at {target}: start one or check the address"
        ))
    })?;
    let total = stmts.len();
    let mut out = Vec::with_capacity(total);
    for (i, stmt) in stmts.iter().enumerate() {
        match client.execute(stmt) {
            Ok(Ok(rows)) => {
                // The wire does not carry column types; they stay "" over the network.
                let kind = statement_kind(stmt).unwrap_or("");
                let types = vec![""; rows.columns.len()];
                out.push(statement_entry(kind, &rows, &types, paging));
            }
            Ok(Err(msg)) => {
                return Err(Failure::new(
                    remote_code(&msg),
                    format!("statement {} of {total}: {msg}", i + 1),
                ));
            }
            Err(e) => return Err(Failure::new(Code::Io, format!("connection error: {e}"))),
        }
    }
    Ok(out)
}

/// Print the envelope for `result` on stdout and return the exit code: 0 ok, 1 failed, 2 usage.
/// A failure also gets the human `<tool>: message` line on stderr.
pub fn finish(tool: &str, cmd: &str, result: Result<Json, Failure>) -> i32 {
    match result {
        Ok(data) => {
            print!("{}", envelope::ok(tool, cmd, data));
            0
        }
        Err(f) => {
            eprintln!("{tool}: {}", f.message);
            print!(
                "{}",
                envelope::err(tool, cmd, f.code, &f.message, f.hint.as_deref())
            );
            if f.code == Code::Usage {
                2
            } else {
                1
            }
        }
    }
}
