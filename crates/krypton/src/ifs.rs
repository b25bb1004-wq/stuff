//! IFS field splitting and pattern prefix/suffix trimming.

use alloc::vec;
use alloc::vec::Vec;

use crate::glob::{compile_glob, ExpChar, GlobAtom};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExpToken {
    Char(ExpChar),
    EmptyQuoted,
}

fn close_stars(states: &mut [bool], atoms: &[GlobAtom]) {
    for (i, atom) in atoms.iter().enumerate() {
        if states[i] && matches!(atom, GlobAtom::Star) {
            states[i + 1] = true;
        }
    }
}

fn match_boundary<I>(
    atoms: &[GlobAtom],
    init_boundary: usize,
    steps: I,
    longest: bool,
) -> Option<usize>
where
    I: IntoIterator<Item = (char, usize)>,
{
    let m = atoms.len();
    let mut cur = vec![false; m + 1];
    let mut next = vec![false; m + 1];
    cur[0] = true;
    close_stars(&mut cur, atoms);

    let mut matched = if cur[m] {
        if !longest {
            return Some(init_boundary);
        }
        Some(init_boundary)
    } else {
        None
    };

    for (ch, boundary) in steps {
        next.fill(false);
        let mut any_next = false;
        for (i, atom) in atoms.iter().enumerate() {
            if !cur[i] {
                continue;
            }
            if matches!(atom, GlobAtom::Star) {
                next[i] = true;
                any_next = true;
            } else if atom.matches_char(ch) {
                next[i + 1] = true;
                any_next = true;
            }
        }
        if !any_next {
            break;
        }
        close_stars(&mut next, atoms);
        core::mem::swap(&mut cur, &mut next);
        if cur[m] {
            matched = Some(boundary);
            if !longest {
                break;
            }
        }
    }

    matched
}

pub(crate) fn trim_prefix<'a>(val: &'a str, pat: &str, longest: bool) -> &'a str {
    let Some(atoms) = compile_glob(pat) else {
        return val;
    };
    let chars: Vec<(usize, char)> = val.char_indices().collect();
    let n = chars.len();
    let steps = (0..n).map(|i| {
        let end = if i + 1 < n { chars[i + 1].0 } else { val.len() };
        (chars[i].1, end)
    });
    match match_boundary(&atoms, 0, steps, longest) {
        Some(idx) => &val[idx..],
        None => val,
    }
}

pub(crate) fn trim_suffix<'a>(val: &'a str, pat: &str, longest: bool) -> &'a str {
    let Some(mut atoms) = compile_glob(pat) else {
        return val;
    };
    atoms.reverse();
    let chars: Vec<(usize, char)> = val.char_indices().collect();
    let steps = chars.iter().rev().map(|&(byte_idx, ch)| (ch, byte_idx));
    match match_boundary(&atoms, val.len(), steps, longest) {
        Some(idx) => &val[..idx],
        None => val,
    }
}

fn is_ifs_ws(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n')
}

fn is_ifs_delim(ec: ExpChar, ifs: &str) -> bool {
    ec.splittable && !ec.quoted && ifs.contains(ec.ch)
}

pub(crate) fn split_by_ifs(tokens: &[ExpToken], ifs: &str) -> Vec<Vec<ExpChar>> {
    if tokens.is_empty() {
        return Vec::new();
    }
    if ifs.is_empty() {
        let field: Vec<ExpChar> = tokens
            .iter()
            .filter_map(|t| match t {
                ExpToken::Char(ec) => Some(*ec),
                ExpToken::EmptyQuoted => None,
            })
            .collect();
        return vec![field];
    }

    let mut fields: Vec<Vec<ExpChar>> = Vec::new();
    let mut cur: Vec<ExpChar> = Vec::new();
    let mut cur_has_content = false;
    let mut i = 0;

    while i < tokens.len() {
        match tokens[i] {
            ExpToken::EmptyQuoted => {
                cur_has_content = true;
                i += 1;
            }
            ExpToken::Char(ec) => {
                if is_ifs_delim(ec, ifs) {
                    if is_ifs_ws(ec.ch) {
                        let had_field = cur_has_content;
                        if cur_has_content {
                            fields.push(core::mem::take(&mut cur));
                            cur_has_content = false;
                        }
                        while i < tokens.len() {
                            if let ExpToken::Char(c2) = tokens[i] {
                                if is_ifs_delim(c2, ifs) && is_ifs_ws(c2.ch) {
                                    i += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                        if had_field && i < tokens.len() {
                            if let ExpToken::Char(c2) = tokens[i] {
                                if is_ifs_delim(c2, ifs) && !is_ifs_ws(c2.ch) {
                                    i += 1;
                                    while i < tokens.len() {
                                        if let ExpToken::Char(c3) = tokens[i] {
                                            if is_ifs_delim(c3, ifs) && is_ifs_ws(c3.ch) {
                                                i += 1;
                                                continue;
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        fields.push(core::mem::take(&mut cur));
                        cur_has_content = false;
                        i += 1;
                        while i < tokens.len() {
                            if let ExpToken::Char(c2) = tokens[i] {
                                if is_ifs_delim(c2, ifs) && is_ifs_ws(c2.ch) {
                                    i += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                    }
                } else {
                    cur.push(ec);
                    cur_has_content = true;
                    i += 1;
                }
            }
        }
    }

    if cur_has_content {
        fields.push(cur);
    }
    fields
}
