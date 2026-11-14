//! IFS field splitting and pattern prefix/suffix trimming.

use alloc::vec;
use alloc::vec::Vec;

use crate::glob::{glob_match, ExpChar};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExpToken {
    Char(ExpChar),
    EmptyQuoted,
}

pub(crate) fn trim_prefix<'a>(val: &'a str, pat: &str, longest: bool) -> &'a str {
    let mut bounds: Vec<usize> = val.char_indices().map(|(i, _)| i).collect();
    bounds.push(val.len());
    if longest {
        for &idx in bounds.iter().rev() {
            if glob_match(pat, &val[..idx]) {
                return &val[idx..];
            }
        }
    } else {
        for &idx in &bounds {
            if glob_match(pat, &val[..idx]) {
                return &val[idx..];
            }
        }
    }
    val
}

pub(crate) fn trim_suffix<'a>(val: &'a str, pat: &str, longest: bool) -> &'a str {
    let mut bounds: Vec<usize> = val.char_indices().map(|(i, _)| i).collect();
    bounds.push(val.len());
    if longest {
        for &idx in &bounds {
            if glob_match(pat, &val[idx..]) {
                return &val[..idx];
            }
        }
    } else {
        for &idx in bounds.iter().rev() {
            if glob_match(pat, &val[idx..]) {
                return &val[..idx];
            }
        }
    }
    val
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
