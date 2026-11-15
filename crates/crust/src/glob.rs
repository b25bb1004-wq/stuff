//! Glob pattern matching and pathname expansion.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::types::{Env, ExpandError};

const MAX_GLOB_BYTES: usize = 8 * 1024 * 1024;

/// Character with quote-status and field-splitting metadata retained between expansion stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExpChar {
    pub(crate) ch: char,
    pub(crate) quoted: bool,
    pub(crate) splittable: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ClassItem {
    Single(char),
    Range(char, char),
    Posix(PosixClass),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PosixClass {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Xdigit,
}

impl PosixClass {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "alnum" => Some(Self::Alnum),
            "alpha" => Some(Self::Alpha),
            "blank" => Some(Self::Blank),
            "cntrl" => Some(Self::Cntrl),
            "digit" => Some(Self::Digit),
            "graph" => Some(Self::Graph),
            "lower" => Some(Self::Lower),
            "print" => Some(Self::Print),
            "punct" => Some(Self::Punct),
            "space" => Some(Self::Space),
            "upper" => Some(Self::Upper),
            "xdigit" => Some(Self::Xdigit),
            _ => None,
        }
    }

    fn matches(self, c: char) -> bool {
        if !c.is_ascii() {
            return false;
        }
        let b = c as u8;
        match self {
            Self::Alnum => b.is_ascii_alphanumeric(),
            Self::Alpha => b.is_ascii_alphabetic(),
            Self::Blank => b == b' ' || b == b'\t',
            Self::Cntrl => b.is_ascii_control(),
            Self::Digit => b.is_ascii_digit(),
            Self::Graph => b.is_ascii_graphic(),
            Self::Lower => b.is_ascii_lowercase(),
            Self::Print => b.is_ascii_graphic() || b == b' ',
            Self::Punct => b.is_ascii_punctuation(),
            Self::Space => b.is_ascii_whitespace(),
            Self::Upper => b.is_ascii_uppercase(),
            Self::Xdigit => b.is_ascii_hexdigit(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum GlobAtom {
    Star,
    Any,
    Lit(char),
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
}

impl GlobAtom {
    pub(crate) fn matches_char(&self, c: char) -> bool {
        match self {
            Self::Star => true,
            Self::Any => true,
            Self::Lit(expected) => c == *expected,
            Self::Class { negated, items } => {
                let hit = items.iter().any(|item| match item {
                    ClassItem::Single(ch) => c == *ch,
                    ClassItem::Range(lo, hi) => lo <= &c && &c <= hi,
                    ClassItem::Posix(cls) => cls.matches(c),
                });
                if *negated {
                    !hit
                } else {
                    hit
                }
            }
        }
    }
}

fn parse_posix_class(chars: &[char], idx: usize) -> Option<(PosixClass, usize)> {
    // Expect `[:name:]` starting at `idx`, where `chars[idx] == '['` and `chars[idx+1] == ':'`.
    if chars.get(idx) != Some(&'[') || chars.get(idx + 1) != Some(&':') {
        return None;
    }
    let mut j = idx + 2;
    while j + 1 < chars.len() {
        if chars[j] == ':' && chars[j + 1] == ']' {
            let name: String = chars[idx + 2..j].iter().collect();
            let cls = PosixClass::from_name(&name)?;
            return Some((cls, j + 2));
        }
        j += 1;
    }
    None
}

fn parse_char_class(chars: &[char], start: usize) -> Option<(GlobAtom, usize)> {
    // `chars[start]` is `'['`.
    let mut i = start + 1;
    if i >= chars.len() {
        return None;
    }
    let negated = if chars[i] == '!' || chars[i] == '^' {
        i += 1;
        true
    } else {
        false
    };
    if i >= chars.len() {
        return None;
    }

    let mut items = Vec::new();
    let mut first = true;

    while i < chars.len() {
        if chars[i] == ']' && !first {
            return Some((GlobAtom::Class { negated, items }, i + 1));
        }
        first = false;

        if let Some((cls, next_i)) = parse_posix_class(chars, i) {
            items.push(ClassItem::Posix(cls));
            i = next_i;
            continue;
        }

        let ch = if chars[i] == '\\' && i + 1 < chars.len() {
            i += 1;
            let esc = chars[i];
            i += 1;
            esc
        } else {
            let c = chars[i];
            i += 1;
            c
        };

        // Check for range `ch-end` where `-` is not followed immediately by `]`.
        if i + 1 < chars.len() && chars[i] == '-' && chars[i + 1] != ']' {
            i += 1; // skip '-'
            let end_ch = if chars[i] == '\\' && i + 1 < chars.len() {
                i += 1;
                let esc = chars[i];
                i += 1;
                esc
            } else {
                let c = chars[i];
                i += 1;
                c
            };
            items.push(ClassItem::Range(ch, end_ch));
        } else {
            items.push(ClassItem::Single(ch));
        }
    }

    None
}

pub(crate) fn compile_glob(pattern: &str) -> Option<Vec<GlobAtom>> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut atoms = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '*' => {
                if !matches!(atoms.last(), Some(GlobAtom::Star)) {
                    atoms.push(GlobAtom::Star);
                }
                i += 1;
            }
            '?' => {
                atoms.push(GlobAtom::Any);
                i += 1;
            }
            '\\' => {
                if i + 1 >= chars.len() {
                    return None;
                }
                atoms.push(GlobAtom::Lit(chars[i + 1]));
                i += 2;
            }
            '[' => {
                if let Some((atom, next_i)) = parse_char_class(&chars, i) {
                    atoms.push(atom);
                    i = next_i;
                } else {
                    atoms.push(GlobAtom::Lit('['));
                    i += 1;
                }
            }
            c => {
                atoms.push(GlobAtom::Lit(c));
                i += 1;
            }
        }
    }

    Some(atoms)
}

/// Match `name` against a shell glob pattern (`*`, `?`, `[abc]`, `[a-z]`, `[!x]`, `[^x]`, `\`).
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let Some(atoms) = compile_glob(pattern) else {
        return false;
    };
    let text: Vec<char> = name.chars().collect();

    let mut pi = 0usize;
    let mut ti = 0usize;
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;

    while ti < text.len() {
        if pi < atoms.len() && matches!(atoms[pi], GlobAtom::Star) {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if pi < atoms.len() && atoms[pi].matches_char(text[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < atoms.len() && matches!(atoms[pi], GlobAtom::Star) {
        pi += 1;
    }

    pi == atoms.len()
}

/// Build a `glob_match` pattern from `ExpChar`s (escaping quoted/literal special chars).
pub(crate) fn build_glob_pattern(chars: &[ExpChar]) -> String {
    let mut pat = String::new();
    for ec in chars {
        if ec.quoted {
            if matches!(ec.ch, '*' | '?' | '[' | ']' | '\\') {
                pat.push('\\');
            }
            pat.push(ec.ch);
        } else if ec.ch == '\\' {
            // Unquoted backslash from variable expansion is literal, not a glob escape.
            pat.push('\\');
            pat.push('\\');
        } else {
            pat.push(ec.ch);
        }
    }
    pat
}

fn has_unquoted_glob(chars: &[ExpChar]) -> bool {
    let mut i = 0;
    while i < chars.len() {
        let ec = chars[i];
        if !ec.quoted {
            if ec.ch == '*' || ec.ch == '?' {
                return true;
            }
            if ec.ch == '[' {
                // Only treat `[` as a glob trigger if there is a closing `]`.
                let mut j = i + 1;
                if j < chars.len() && (chars[j].ch == '!' || chars[j].ch == '^') {
                    j += 1;
                }
                if j < chars.len() && chars[j].ch == ']' {
                    j += 1;
                }
                while j < chars.len() {
                    if chars[j].ch == ']' {
                        return true;
                    }
                    j += 1;
                }
            }
        }
        i += 1;
    }
    false
}

/// Perform pathname expansion on a single post-split field.
pub(crate) fn expand_pathname(
    field: &[ExpChar],
    env: &mut dyn Env,
    glob_budget: &mut usize,
) -> Result<Vec<String>, ExpandError> {
    let literal: String = field.iter().map(|c| c.ch).collect();
    if !has_unquoted_glob(field) {
        *glob_budget = glob_budget.saturating_add(48 + literal.len());
        if *glob_budget > MAX_GLOB_BYTES {
            return Err(ExpandError::BadSubst(String::from(
                "pathname expansion limit exceeded",
            )));
        }
        return Ok(vec![literal]);
    }

    // Split on '/' into segments.
    let mut segments: Vec<&[ExpChar]> = Vec::new();
    let mut start = 0;
    for (idx, ec) in field.iter().enumerate() {
        if ec.ch == '/' {
            segments.push(&field[start..idx]);
            start = idx + 1;
        }
    }
    segments.push(&field[start..]);

    let mut paths: Vec<String> = vec![String::new()];
    let mut has_leading_slash = false;

    for (seg_idx, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            if seg_idx == 0 {
                has_leading_slash = true;
                for p in &mut paths {
                    p.push('/');
                }
            } else {
                for p in &mut paths {
                    if !p.ends_with('/') || seg_idx + 1 < segments.len() || has_leading_slash {
                        p.push('/');
                    }
                }
            }
            continue;
        }

        if !has_unquoted_glob(seg) {
            let lit: String = seg.iter().map(|c| c.ch).collect();
            let extra = paths.len().saturating_mul(lit.len() + 1);
            *glob_budget = glob_budget.saturating_add(extra);
            if *glob_budget > MAX_GLOB_BYTES {
                return Err(ExpandError::BadSubst(String::from(
                    "pathname expansion limit exceeded",
                )));
            }
            for p in &mut paths {
                if !p.is_empty() && !p.ends_with('/') {
                    p.push('/');
                }
                p.push_str(&lit);
            }
            continue;
        }

        let pat = build_glob_pattern(seg);
        let literal_dot = seg.first().is_some_and(|c| c.ch == '.');
        let mut next_paths = Vec::new();

        for base in &paths {
            let dir_to_list = if base.is_empty() { "." } else { base.as_str() };
            let mut entries = env.list_dir(dir_to_list);
            entries.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            for entry in entries {
                if entry.starts_with('.') && !literal_dot {
                    continue;
                }
                if glob_match(&pat, &entry) {
                    let mut joined = base.clone();
                    if !joined.is_empty() && !joined.ends_with('/') {
                        joined.push('/');
                    }
                    joined.push_str(&entry);
                    *glob_budget = glob_budget.saturating_add(48 + joined.len());
                    if *glob_budget > MAX_GLOB_BYTES {
                        return Err(ExpandError::BadSubst(String::from(
                            "pathname expansion limit exceeded",
                        )));
                    }
                    next_paths.push(joined);
                }
            }
        }

        if next_paths.is_empty() {
            return Ok(vec![literal]);
        }
        paths = next_paths;
    }

    paths.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    if paths.is_empty() {
        Ok(vec![literal])
    } else {
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_table() {
        let cases = [
            ("", "", true),
            ("", "a", false),
            ("*", "", true),
            ("*", "abc", true),
            ("?", "", false),
            ("?", "a", true),
            ("?", "ab", false),
            ("*.rs", "main.rs", true),
            ("*.rs", ".rs", true),
            ("*.rs", "main.c", false),
            ("a*b*c", "axxxbyyyc", true),
            ("a*b*c", "abc", true),
            ("a*b*c", "ab", false),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("[abc]", "b", true),
            ("[abc]", "d", false),
            ("[a-z]", "m", true),
            ("[a-z]", "M", false),
            ("[!a-z]", "M", true),
            ("[!a-z]", "m", false),
            ("[^0-9]", "a", true),
            ("[^0-9]", "5", false),
            ("[]a]", "]", true),
            ("[]a]", "a", true),
            ("[!]a]", "b", true),
            ("[!]a]", "]", false),
            ("[a-]", "-", true),
            ("[-a]", "-", true),
            ("\\*", "*", true),
            ("\\*", "a", false),
            ("\\?", "?", true),
            ("\\?", "a", false),
            ("\\[a\\]", "[a]", true),
            ("\\[a\\]", "a", false),
            ("[[:digit:]]+", "5+", true),
            ("[[:alpha:]]*", "x1", true),
            ("[[:alpha:]]*", "1x", false),
            ("[abc", "[abc", true),
            ("a\\", "a\\", false),
            ("a\\\\", "a\\", true),
            ("a*a*a*a*b", "aaaaaaaaaaaaaaa", false),
        ];
        for (pat, name, expected) in cases {
            assert_eq!(
                glob_match(pat, name),
                expected,
                "glob_match({pat:?}, {name:?})"
            );
        }
    }
}
