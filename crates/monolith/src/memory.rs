//! Memory: facts, lessons and skills kept as JSON lines in a folder, with keyword search.
//! STUB written by the leader: signatures are pinned, bodies belong to worker A3.

use std::path::{Path, PathBuf};

use aci::envelope::Code;

use crate::types::{CmdOut, Error, Result};

/// What kind of thing is remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Something true about this computer or task ("the server uses port 8473").
    Fact,
    /// What to do differently next time, learned from a failure.
    Lesson,
    /// A command sequence that solved a kind of task.
    Skill,
}

/// One remembered item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 1-based id, unique in the store.
    pub id: u64,
    /// Its kind.
    pub kind: Kind,
    /// The text.
    pub text: String,
    /// True when a check confirmed it (a retry passed with this lesson, or the task passed).
    pub verified: bool,
    /// Milliseconds since the Unix epoch when it was added.
    pub ts_ms: u64,
}

/// A memory store: the folder that holds `items.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// The folder.
    pub dir: PathBuf,
}

fn stub<T>() -> Result<T> {
    Err(Error::new(Code::Unsupported, "memory: not built yet"))
}

impl Kind {
    /// Parses `fact`, `lesson` or `skill`.
    pub fn parse(_s: &str) -> Result<Self> {
        stub()
    }

    /// `fact`, `lesson` or `skill`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Lesson => "lesson",
            Self::Skill => "skill",
        }
    }
}

impl Store {
    /// A store in `dir` (created on first add).
    pub fn at(dir: &Path) -> Self {
        Self { dir: dir.into() }
    }

    /// Adds an item and returns its id. An item with the same kind and text is not added twice
    /// (its id is returned, and `verified` becomes true if either is).
    pub fn add(&self, _kind: Kind, _text: &str, _verified: bool) -> Result<u64> {
        stub()
    }

    /// Every item, oldest first. A missing store is empty.
    pub fn list(&self) -> Result<Vec<Item>> {
        stub()
    }

    /// The best `k` items for `query` among `kinds`, best first; items scoring 0 are left out.
    pub fn search(
        &self,
        _query: &str,
        _k: usize,
        _kinds: &[Kind],
        _verified_only: bool,
    ) -> Result<Vec<Item>> {
        stub()
    }
}

/// How well `text` matches `query`: 0.0 for no shared word. Words are lowercased runs of letters
/// and digits; common words (`the`, `a`, `to`, ...) do not count; rarer shared words count more.
pub fn score(_query: &str, _text: &str) -> f64 {
    0.0
}

/// CLI: `memory NAME add KIND TEXT`, `memory NAME list`, `memory NAME search QUERY`.
pub fn cmd(_args: &[String]) -> Result<CmdOut> {
    stub()
}
