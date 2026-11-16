//! Command history: consecutive duplicates collapse, the newest 5000 entries are kept, and the
//! file stores one entry per line with `\` and newline escaped.

use std::fs;
use std::io;
use std::path::Path;
use std::string::String;
use std::vec::Vec;

/// Maximum number of entries kept in memory and on disk.
pub(crate) const CAP: usize = 5000;

#[derive(Default)]
pub(crate) struct History {
    items: Vec<String>,
}

impl History {
    /// Number of entries.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// True when there are no entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Entry at `index` (0 is the oldest).
    pub(crate) fn get(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(String::as_str)
    }

    /// Record a finished command line. Blank lines and an immediate repeat are skipped.
    pub(crate) fn push(&mut self, line: &str) {
        if line.trim().is_empty() || self.items.last().is_some_and(|last| last == line) {
            return;
        }
        self.items.push(line.to_string());
        if self.items.len() > CAP {
            let excess = self.items.len() - CAP;
            self.items.drain(..excess);
        }
    }

    /// Newest entry before `before` that contains `query`, searching backwards.
    pub(crate) fn search_back(&self, query: &str, before: usize) -> Option<usize> {
        let end = before.min(self.items.len());
        (0..end)
            .rev()
            .find(|&i| self.items.get(i).is_some_and(|s| s.contains(query)))
    }

    /// Read a history file. A missing or unreadable file gives an empty history.
    pub(crate) fn load(path: &Path) -> History {
        let mut history = History::default();
        if let Ok(text) = fs::read_to_string(path) {
            for line in text.lines() {
                history.push(&unescape(line));
            }
        }
        history
    }

    /// Write the history file, replacing its previous contents.
    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        let mut text = String::new();
        for item in &self.items {
            text.push_str(&escape(item));
            text.push('\n');
        }
        fs::write(path, text)
    }
}

fn escape(entry: &str) -> String {
    let mut out = String::with_capacity(entry.len());
    for c in entry.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{History, CAP};
    use std::format;
    use std::path::PathBuf;
    use std::string::String;
    use std::vec::Vec;

    fn temp_file(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("krypton-hist-{tag}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn round_trip_keeps_order_escapes_and_collapses_repeats() {
        let path = temp_file("rt");
        let mut h = History::default();
        for line in [
            "echo one",
            "echo one",
            "",
            "   ",
            "echo two\\n",
            "for x\nin a; do\ndone",
            "ls",
        ] {
            h.push(line);
        }
        h.save(&path).expect("save history");
        let back = History::load(&path);
        let _ = std::fs::remove_file(&path);
        let items: Vec<String> = (0..back.len())
            .filter_map(|i| back.get(i).map(String::from))
            .collect();
        assert_eq!(
            items,
            ["echo one", "echo two\\n", "for x\nin a; do\ndone", "ls"]
        );
    }

    #[test]
    fn cap_keeps_only_the_newest_entries() {
        let mut h = History::default();
        for i in 0..(CAP + 10) {
            h.push(&format!("cmd {i}"));
        }
        assert_eq!(h.len(), CAP);
        assert_eq!(h.get(0), Some("cmd 10"));
        assert_eq!(h.get(CAP - 1), Some(format!("cmd {}", CAP + 9).as_str()));
    }

    #[test]
    fn reverse_search_finds_the_newest_match_before_a_point() {
        let mut h = History::default();
        for line in ["git status", "cargo test", "git log"] {
            h.push(line);
        }
        assert_eq!(h.search_back("git", h.len()), Some(2));
        assert_eq!(h.search_back("git", 2), Some(0));
        assert_eq!(h.search_back("nothing", h.len()), None);
    }

    #[test]
    fn missing_file_loads_empty() {
        let h = History::load(&temp_file("missing"));
        assert_eq!(h.len(), 0);
    }
}
