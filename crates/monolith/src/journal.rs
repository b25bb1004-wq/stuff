//! The journal: every step a computer took, one JSON object per line in `journal.jsonl`.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use aci::json::Json;

use crate::computer::Computer;
use crate::types::{Error, Result};

/// One journal line.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// 1-based position in the journal.
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    /// The session (one agent run) this belongs to.
    pub session: String,
    /// `task`, `model`, `action`, `observation`, `done`, `reflect`, `lesson`, `exec` or `error`.
    pub kind: String,
    /// Kind-specific fields.
    pub data: Json,
}

fn journal_path(c: &Computer) -> PathBuf {
    c.dir.join("journal.jsonl")
}

fn io(context: &str, e: &std::io::Error) -> Error {
    Error::new(Code::Io, format!("{context}: {e}"))
}

/// Appends one entry and returns its `seq`.
pub fn append(c: &Computer, session: &str, kind: &str, data: Json) -> Result<u64> {
    let seq = u64::try_from(read(c)?.len()).unwrap_or(u64::MAX - 1) + 1;
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let line = Json::obj([
        ("seq", Json::Int(i64::try_from(seq).unwrap_or(i64::MAX))),
        ("ts_ms", Json::Int(i64::try_from(ts_ms).unwrap_or(i64::MAX))),
        ("session", Json::Str(session.into())),
        ("kind", Json::Str(kind.into())),
        ("data", data),
    ])
    .write();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path(c))
        .map_err(|e| io("cannot open the journal", &e))?;
    file.write_all(format!("{line}\n").as_bytes())
        .map_err(|e| io("cannot write the journal", &e))?;
    Ok(seq)
}

/// Reads the whole journal, oldest first. A missing file is an empty journal.
/// Lines that do not parse (a torn last write, for example) are skipped.
pub fn read(c: &Computer) -> Result<Vec<Entry>> {
    let bytes = match fs::read(journal_path(c)) {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io("cannot read the journal", &e)),
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = Json::parse(line.trim()) else {
            continue;
        };
        let (Some(seq), Some(ts), Some(session), Some(kind)) = (
            v.get("seq").and_then(Json::as_i64),
            v.get("ts_ms").and_then(Json::as_i64),
            v.get("session").and_then(Json::as_str),
            v.get("kind").and_then(Json::as_str),
        ) else {
            continue;
        };
        out.push(Entry {
            seq: u64::try_from(seq).unwrap_or(0),
            ts_ms: u64::try_from(ts).unwrap_or(0),
            session: session.into(),
            kind: kind.into(),
            data: v.get("data").cloned().unwrap_or(Json::Null),
        });
    }
    Ok(out)
}
