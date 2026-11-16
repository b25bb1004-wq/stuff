//! `.gitignore` matching: parses pattern text and answers whether a repository path is ignored.
//! Reading the files is the caller's job, so this module does no I/O.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

/// Ordered `.gitignore` rules from every file loaded so far. Later rules win, as in git.
#[derive(Debug, Default, Clone)]
pub struct Rules {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct Rule {
    /// Directory of the `.gitignore` that held the rule ("" for the repository root).
    base: String,
    /// Pattern split on `/`.
    segs: Vec<String>,
    negate: bool,
    dir_only: bool,
    anchored: bool,
}

impl Rules {
    /// Empty rule set: nothing is ignored.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the lines of a `.gitignore` in directory `base` (repo-relative, "" for the root).
    /// Blank lines and `#` comments are skipped; a leading `!` re-includes a path.
    pub fn add(&mut self, base: &str, text: &str) {
        for raw in text.lines() {
            let line = raw.trim_end_matches(['\r', ' ', '\t']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negate, body) = if let Some(rest) = line.strip_prefix('!') {
                (true, rest)
            } else if line.starts_with("\\!") || line.starts_with("\\#") {
                (false, &line[1..])
            } else {
                (false, line)
            };
            let (dir_only, body) = match body.strip_suffix('/') {
                Some(b) => (true, b),
                None => (false, body),
            };
            let (lead_slash, body) = match body.strip_prefix('/') {
                Some(b) => (true, b),
                None => (false, body),
            };
            if body.is_empty() {
                continue;
            }
            self.rules.push(Rule {
                base: base.to_string(),
                segs: body
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                negate,
                dir_only,
                anchored: lead_slash || body.contains('/'),
            });
        }
    }

    /// True if the repo-relative path `path` is ignored. A path inside an ignored directory is
    /// ignored too, so callers never need to descend into one.
    pub fn is_ignored(&self, path: &str, is_dir: bool) -> bool {
        if path
            .match_indices('/')
            .any(|(i, _)| self.verdict(&path[..i], true))
        {
            return true;
        }
        self.verdict(path, is_dir)
    }

    fn verdict(&self, path: &str, is_dir: bool) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|r| matches(r, path, is_dir))
            .is_some_and(|r| !r.negate)
    }
}

fn matches(rule: &Rule, path: &str, is_dir: bool) -> bool {
    if rule.dir_only && !is_dir {
        return false;
    }
    let sub = if rule.base.is_empty() {
        path
    } else {
        match path
            .strip_prefix(rule.base.as_str())
            .and_then(|r| r.strip_prefix('/'))
        {
            Some(s) => s,
            None => return false,
        }
    };
    let comps: Vec<&str> = sub.split('/').collect();
    if rule.anchored {
        glob(&rule.segs, &comps)
    } else {
        comps.last().is_some_and(|last| glob(&rule.segs, &[*last]))
    }
}

/// Match pattern segments against path components; `**` may span any number of directories.
fn glob(pat: &[String], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((seg, rest)) if seg.as_str() == "**" => {
            if rest.is_empty() {
                !path.is_empty()
            } else {
                (0..=path.len()).any(|i| glob(rest, &path[i..]))
            }
        }
        Some((seg, rest)) => match path.split_first() {
            Some((comp, tail)) => {
                let p: Vec<char> = seg.chars().collect();
                let s: Vec<char> = comp.chars().collect();
                seg_match(&p, &s) && glob(rest, tail)
            }
            None => false,
        },
    }
}

/// Glob within one path component: `*`, `?`, `[set]`, `[!set]`, and `\x` for a literal x.
fn seg_match(p: &[char], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') => (0..=s.len()).any(|i| seg_match(&p[1..], &s[i..])),
        Some('?') => !s.is_empty() && seg_match(&p[1..], &s[1..]),
        Some('\\') if p.len() > 1 => s.first() == Some(&p[1]) && seg_match(&p[2..], &s[1..]),
        Some('[') => match p[1..].iter().position(|&c| c == ']') {
            // shortcut: an unclosed '[' is taken literally, a ']' first in a set is not special
            Some(j) => {
                let end = j + 1;
                let Some(&c) = s.first() else { return false };
                let body = &p[1..end];
                let (neg, body) = match body.first() {
                    Some('!') | Some('^') => (true, &body[1..]),
                    _ => (false, body),
                };
                let mut hit = false;
                let mut i = 0;
                while i < body.len() {
                    if i + 2 < body.len() && body[i + 1] == '-' {
                        hit |= body[i] <= c && c <= body[i + 2];
                        i += 3;
                    } else {
                        hit |= body[i] == c;
                        i += 1;
                    }
                }
                hit != neg && seg_match(&p[end + 1..], &s[1..])
            }
            None => s.first() == Some(&'[') && seg_match(&p[1..], &s[1..]),
        },
        Some(&c) => s.first() == Some(&c) && seg_match(&p[1..], &s[1..]),
    }
}
