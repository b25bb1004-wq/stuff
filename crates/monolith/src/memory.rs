//! Memory: facts, lessons and skills kept as JSON lines in a folder, with keyword search.

use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use aci::json::Json;

use crate::computer::{home, Computer};
use crate::types::{CmdOut, Error, Result};

/// The file inside a store folder, one JSON object per line.
const FILE: &str = "items.jsonl";
/// Written first on a rewrite, then renamed over [`FILE`].
const TMP: &str = "items.jsonl.tmp";
/// Longest text one item may hold, in characters.
const MAX_CHARS: usize = 2000;
/// Words that never count for search.
const STOP: &[&str] = &[
    "the", "a", "an", "to", "of", "in", "on", "at", "for", "and", "or", "is", "are", "was", "be",
    "it", "this", "that", "with", "as", "by", "from", "into", "then", "you", "your", "use",
    "using", "do", "not",
];

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

impl Kind {
    /// Parses `fact`, `lesson` or `skill`.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "fact" => Ok(Self::Fact),
            "lesson" => Ok(Self::Lesson),
            "skill" => Ok(Self::Skill),
            _ => Err(
                Error::new(Code::Invalid, format!("unknown memory kind `{s}`"))
                    .hint("use one of: fact, lesson, skill"),
            ),
        }
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
    pub fn add(&self, kind: Kind, text: &str, verified: bool) -> Result<u64> {
        let text = normalize(text);
        if text.is_empty() {
            return Err(Error::new(Code::Invalid, "memory text is empty")
                .hint("write one fact, lesson or skill in words"));
        }
        let length = text.chars().count();
        if length > MAX_CHARS {
            return Err(Error::new(
                Code::Invalid,
                format!("memory text is {length} characters; the limit is {MAX_CHARS}"),
            )
            .hint("keep one idea per item"));
        }
        let raw = self.read_raw()?;
        let mut items = parse_items(&raw);
        let key = text.to_lowercase();
        if let Some(pos) = items
            .iter()
            .position(|i| i.kind == kind && i.text.to_lowercase() == key)
        {
            let id = items[pos].id;
            if verified && !items[pos].verified {
                items[pos].verified = true;
                self.write_all(&items)?;
            }
            return Ok(id);
        }
        let id = items
            .iter()
            .map(|i| i.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let item = Item {
            id,
            kind,
            text,
            verified,
            ts_ms: now_ms(),
        };
        // A last line without a newline (a torn write) must not swallow the new item.
        let mut line = String::new();
        if !raw.is_empty() && !raw.ends_with('\n') {
            line.push('\n');
        }
        line.push_str(&item_json(&item));
        line.push('\n');
        fs::create_dir_all(&self.dir).map_err(|e| io_error("create the memory folder", &e))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.file())
            .map_err(|e| io_error("open the memory file", &e))?;
        file.write_all(line.as_bytes())
            .map_err(|e| io_error("write the memory file", &e))?;
        Ok(id)
    }

    /// Every item, oldest first. A missing store is empty.
    pub fn list(&self) -> Result<Vec<Item>> {
        Ok(parse_items(&self.read_raw()?))
    }

    /// The best `k` items for `query` among `kinds`, best first; items scoring 0 are left out.
    pub fn search(
        &self,
        query: &str,
        k: usize,
        kinds: &[Kind],
        verified_only: bool,
    ) -> Result<Vec<Item>> {
        let query_words = words(query);
        if k == 0 || query_words.is_empty() {
            return Ok(Vec::new());
        }
        let items: Vec<Item> = self
            .list()?
            .into_iter()
            .filter(|i| {
                (kinds.is_empty() || kinds.contains(&i.kind)) && (!verified_only || i.verified)
            })
            .collect();
        let n = items.len() as f64;
        let item_words: Vec<Vec<String>> = items.iter().map(|i| words(&i.text)).collect();
        // Rarer words weigh more: ln(1 + N / df), df counted over the items considered.
        let weights: Vec<f64> = query_words
            .iter()
            .map(|q| {
                let df = item_words.iter().filter(|ws| ws.contains(q)).count();
                if df == 0 {
                    0.0
                } else {
                    (1.0 + n / df as f64).ln()
                }
            })
            .collect();
        let mut hits: Vec<(f64, Item)> = Vec::new();
        for (item, ws) in items.into_iter().zip(&item_words) {
            let mut score = 0.0;
            for (q, w) in query_words.iter().zip(&weights) {
                if ws.contains(q) {
                    score += w;
                }
            }
            if score > 0.0 {
                hits.push((score, item));
            }
        }
        hits.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then(b.1.verified.cmp(&a.1.verified))
                .then(b.1.id.cmp(&a.1.id))
        });
        Ok(hits.into_iter().take(k).map(|(_, item)| item).collect())
    }

    fn file(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    /// The file text, empty when there is no file yet.
    fn read_raw(&self) -> Result<String> {
        match fs::read(self.file()) {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(io_error("read the memory file", &e)),
        }
    }

    /// Replaces the whole file with `items` (through a temporary file, then a rename).
    fn write_all(&self, items: &[Item]) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|e| io_error("create the memory folder", &e))?;
        let mut body = String::new();
        for item in items {
            body.push_str(&item_json(item));
            body.push('\n');
        }
        let tmp = self.dir.join(TMP);
        fs::write(&tmp, body).map_err(|e| io_error("write the memory file", &e))?;
        fs::rename(&tmp, self.file()).map_err(|e| io_error("replace the memory file", &e))
    }
}

/// How well `text` matches `query`: 0.0 for no shared word. Words are lowercased runs of letters
/// and digits; common words (`the`, `a`, `to`, ...) do not count; rarer shared words count more.
pub fn score(query: &str, text: &str) -> f64 {
    let query_words = words(query);
    let text_words = words(text);
    if query_words.is_empty() || text_words.is_empty() {
        return 0.0;
    }
    let shared = query_words
        .iter()
        .filter(|q| text_words.contains(q))
        .count();
    shared as f64 / (query_words.len() as f64).sqrt()
}

/// CLI: `memory NAME add KIND TEXT`, `memory NAME list`, `memory NAME search [--k N] QUERY`.
pub fn cmd(args: &[String]) -> Result<CmdOut> {
    let (name, rest) = args.split_first().ok_or_else(usage)?;
    let computer = Computer::open(&home(), name)?;
    let store = Store::at(&computer.dir.join("memory"));
    let parts: Vec<&str> = rest.iter().map(String::as_str).collect();
    match parts.as_slice() {
        ["add", kind, text @ ..] => {
            let kind = Kind::parse(kind)?;
            let id = store.add(kind, &text.join(" "), false)?;
            let stored = store
                .list()?
                .into_iter()
                .find(|i| i.id == id)
                .map(|i| i.text)
                .unwrap_or_default();
            let data = Json::obj([
                ("id", Json::Int(id as i64)),
                ("kind", Json::Str(kind.as_str().into())),
                ("text", Json::Str(stored)),
            ]);
            Ok(CmdOut {
                data,
                text: format!("remembered #{id}"),
                exit: 0,
            })
        }
        ["list"] => Ok(listing(&store.list()?)),
        ["--k", ..] => Err(usage()),
        ["search", tail @ ..] => {
            let (k, query) = match tail {
                ["--k", n, query @ ..] => {
                    let k = n.parse::<usize>().map_err(|_| {
                        Error::new(Code::Usage, format!("--k needs a number, not `{n}`"))
                    })?;
                    (k, query)
                }
                query => (5, query),
            };
            if query.is_empty() {
                return Err(usage());
            }
            Ok(listing(&store.search(&query.join(" "), k, &[], false)?))
        }
        _ => Err(usage()),
    }
}

/// The usage error for `memory`.
fn usage() -> Error {
    Error::new(Code::Usage, "memory needs a computer name and a form")
        .hint("memory NAME add KIND TEXT | memory NAME list | memory NAME search [--k N] QUERY")
}

/// The `list` and `search` output: the items as data, one line each as text.
fn listing(items: &[Item]) -> CmdOut {
    let data = Json::obj([
        ("items", Json::Arr(items.iter().map(item_value).collect())),
        ("total", Json::Int(items.len() as i64)),
    ]);
    let text = if items.is_empty() {
        "no items".to_string()
    } else {
        items
            .iter()
            .map(|i| {
                let mark = if i.verified { "[v] " } else { "" };
                format!("#{} {} {mark}{}", i.id, i.kind.as_str(), i.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    CmdOut {
        data,
        text,
        exit: 0,
    }
}

/// Collapses every whitespace run (newlines included) to one space and trims the ends.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Distinct search words of `text`, in first-seen order.
pub fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for run in text
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if run.len() < 2 || STOP.contains(&run) {
            continue;
        }
        let word = if run.len() > 3 {
            run.strip_suffix('s').unwrap_or(run)
        } else {
            run
        };
        if !out.iter().any(|w| w == word) {
            out.push(word.to_string());
        }
    }
    out
}

/// Parses the file text, skipping every line that is not a valid item.
fn parse_items(raw: &str) -> Vec<Item> {
    raw.lines().filter_map(parse_item).collect()
}

fn parse_item(line: &str) -> Option<Item> {
    let v = Json::parse(line).ok()?;
    Some(Item {
        id: u64::try_from(v.get("id")?.as_i64()?).ok()?,
        kind: Kind::parse(v.get("kind")?.as_str()?).ok()?,
        text: v.get("text")?.as_str()?.to_string(),
        verified: v.get("verified")?.as_bool()?,
        ts_ms: v
            .get("ts_ms")
            .and_then(Json::as_i64)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0),
    })
}

fn item_json(item: &Item) -> String {
    item_value(item).write()
}

fn item_value(item: &Item) -> Json {
    Json::obj([
        ("id", Json::Int(item.id as i64)),
        ("kind", Json::Str(item.kind.as_str().into())),
        ("text", Json::Str(item.text.clone())),
        ("verified", Json::Bool(item.verified)),
        ("ts_ms", Json::Int(item.ts_ms as i64)),
    ])
}

fn io_error(what: &str, e: &std::io::Error) -> Error {
    Error::new(Code::Io, format!("cannot {what}: {e}"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}
