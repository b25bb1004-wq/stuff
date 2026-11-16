//! The journal: every step a computer took, one JSON object per line in `journal.jsonl`.
//! STUB written by the leader: signatures are pinned, bodies belong to worker A1.

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

/// Appends one entry and returns its `seq`.
pub fn append(_c: &Computer, _session: &str, _kind: &str, _data: Json) -> Result<u64> {
    Err(Error::new(Code::Unsupported, "journal: not built yet"))
}

/// Reads the whole journal, oldest first. A missing file is an empty journal.
pub fn read(_c: &Computer) -> Result<Vec<Entry>> {
    Err(Error::new(Code::Unsupported, "journal: not built yet"))
}
