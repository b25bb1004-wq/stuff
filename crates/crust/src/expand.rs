//! POSIX word expansion (`expand` and `expand_one`).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::arith::arith;
use crate::ast::{Param, ParamOp, Part, Word};
use crate::glob::{build_glob_pattern, expand_pathname, ExpChar};
use crate::ifs::{split_by_ifs, trim_prefix, trim_suffix, ExpToken};
use crate::types::{is_assignable_name, Env, ExpandError};
use crate::word::parse_arith_template;

struct Stage1 {
    fields: Vec<Vec<ExpToken>>,
    depth: usize,
    saw_empty_at: bool,
}

impl Stage1 {
    fn new() -> Self {
        Self {
            fields: vec![Vec::new()],
            depth: 0,
            saw_empty_at: false,
        }
    }

    fn total_tokens(&self) -> usize {
        self.fields.iter().map(Vec::len).sum()
    }

    fn push_str(&mut self, s: &str, quoted: bool, splittable: bool) {
        let Some(cur) = self.fields.last_mut() else {
            return;
        };
        if s.is_empty() && quoted {
            cur.push(ExpToken::EmptyQuoted);
            return;
        }
        for ch in s.chars() {
            cur.push(ExpToken::Char(ExpChar {
                ch,
                quoted,
                splittable,
            }));
        }
    }

    fn push_char(&mut self, ch: char, quoted: bool, splittable: bool) {
        if let Some(cur) = self.fields.last_mut() {
            cur.push(ExpToken::Char(ExpChar {
                ch,
                quoted,
                splittable,
            }));
        }
    }

    fn push_empty_quoted(&mut self) {
        if let Some(cur) = self.fields.last_mut() {
            cur.push(ExpToken::EmptyQuoted);
        }
    }

    fn split_field(&mut self) {
        self.fields.push(Vec::new());
    }
}

fn ifs_string(env: &dyn Env) -> String {
    match env.get("IFS") {
        None => String::from(" \t\n"),
        Some(s) => s,
    }
}

fn ifs_first_is_non_ws(env: &dyn Env) -> bool {
    first_ifs_sep(env).is_some_and(|c| !matches!(c, ' ' | '\t' | '\n'))
}

fn first_ifs_sep(env: &dyn Env) -> Option<char> {
    match env.get("IFS") {
        None => Some(' '),
        Some(s) => s.chars().next(),
    }
}

fn join_positional(pos: &[String], sep: Option<char>) -> String {
    let mut out = String::new();
    for (idx, item) in pos.iter().enumerate() {
        if idx > 0 {
            if let Some(ch) = sep {
                out.push(ch);
            }
        }
        out.push_str(item);
    }
    out
}

fn lookup_param(name: &str, env: &dyn Env) -> Option<String> {
    match name {
        "?" | "$" | "!" | "-" | "0" => env.get(name),
        "#" => Some(env.positional().len().to_string()),
        "@" => {
            let pos = env.positional();
            if pos.is_empty() {
                None
            } else {
                Some(join_positional(&pos, Some(' ')))
            }
        }
        "*" => {
            let pos = env.positional();
            if pos.is_empty() {
                None
            } else {
                Some(join_positional(&pos, first_ifs_sep(env)))
            }
        }
        _ if name.bytes().all(|b| b.is_ascii_digit()) => {
            let idx = name.parse::<usize>().ok()?;
            if idx == 0 {
                env.get("0")
            } else {
                env.positional().get(idx - 1).cloned()
            }
        }
        _ => env.get(name),
    }
}

fn expand_word_stage1(
    word: &[Part],
    in_double: bool,
    split_lit: bool,
    env: &mut dyn Env,
    st: &mut Stage1,
) -> Result<(), ExpandError> {
    st.depth += 1;
    if st.depth > 128 {
        st.depth -= 1;
        return Err(ExpandError::BadSubst(String::from(
            "expansion nesting too deep",
        )));
    }
    let res = (|| {
        for part in word {
            match part {
                Part::Lit(s) => st.push_str(s, in_double, !in_double && split_lit),
                Part::Single(s) => {
                    if s.is_empty() {
                        st.push_empty_quoted();
                    } else {
                        st.push_str(s, true, false);
                    }
                }
                Part::Escaped(ch) => st.push_char(*ch, true, false),
                Part::Double(parts) => {
                    if parts.is_empty() {
                        st.push_empty_quoted();
                    } else {
                        let before = st.total_tokens();
                        let prev_empty_at = st.saw_empty_at;
                        st.saw_empty_at = false;
                        expand_word_stage1(parts, true, false, env, st)?;
                        if st.total_tokens() == before && !st.saw_empty_at {
                            st.push_empty_quoted();
                        }
                        st.saw_empty_at = prev_empty_at;
                    }
                }
                Part::Tilde(user) => {
                    if let Some(home) = env.home(user) {
                        if home.is_empty() {
                            st.push_empty_quoted();
                        } else {
                            st.push_str(&home, true, false);
                        }
                    } else {
                        st.push_char('~', false, split_lit);
                        st.push_str(user, false, split_lit);
                    }
                }
                Part::Cmd(script) => {
                    let mut out = env.command(script);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    st.push_str(&out, in_double, !in_double);
                }
                Part::Arith(raw) => {
                    let tpl =
                        parse_arith_template(raw).map_err(|e| ExpandError::BadSubst(e.msg))?;
                    let expanded = expand_one_depth(&tpl, env, st.depth)?;
                    let val = arith(&expanded, env)?;
                    st.push_str(&val.to_string(), in_double, !in_double);
                }
                Part::Param(p) => expand_param(p, in_double, env, st)?,
            }
        }
        Ok(())
    })();
    st.depth -= 1;
    res
}

fn expand_param(
    p: &Param,
    in_double: bool,
    env: &mut dyn Env,
    st: &mut Stage1,
) -> Result<(), ExpandError> {
    let splittable = !in_double;
    match &p.op {
        ParamOp::Plain => {
            if p.name == "@" {
                let pos = env.positional();
                if pos.is_empty() {
                    st.saw_empty_at = true;
                }
                let keep_empty = in_double || ifs_first_is_non_ws(env);
                for (idx, item) in pos.iter().enumerate() {
                    if idx > 0 {
                        st.split_field();
                    }
                    if item.is_empty() && keep_empty {
                        st.push_empty_quoted();
                    } else {
                        st.push_str(item, in_double, splittable);
                    }
                }
                return Ok(());
            }
            if p.name == "*" {
                let pos = env.positional();
                if in_double {
                    let joined = join_positional(&pos, first_ifs_sep(env));
                    st.push_str(&joined, true, false);
                } else {
                    let keep_empty = ifs_first_is_non_ws(env);
                    for (idx, item) in pos.iter().enumerate() {
                        if idx > 0 {
                            st.split_field();
                        }
                        if item.is_empty() && keep_empty {
                            st.push_empty_quoted();
                        } else {
                            st.push_str(item, false, true);
                        }
                    }
                }
                return Ok(());
            }
            if let Some(val) = lookup_param(&p.name, env) {
                st.push_str(&val, in_double, splittable);
            }
            Ok(())
        }
        ParamOp::Length => {
            let len = if p.name == "@" || p.name == "*" {
                env.positional().len()
            } else {
                lookup_param(&p.name, env)
                    .map(|s| s.chars().count())
                    .unwrap_or(0)
            };
            st.push_str(&len.to_string(), in_double, splittable);
            Ok(())
        }
        ParamOp::Default(colon, w) => {
            let val = lookup_param(&p.name, env);
            let use_val = match &val {
                Some(s) => !*colon || !s.is_empty(),
                None => false,
            };
            if use_val {
                if p.name == "@" && in_double {
                    return expand_param(
                        &Param {
                            name: String::from("@"),
                            op: ParamOp::Plain,
                        },
                        true,
                        env,
                        st,
                    );
                }
                if let Some(s) = val {
                    st.push_str(&s, in_double, splittable);
                }
            } else {
                expand_word_stage1(w, in_double, !in_double, env, st)?;
            }
            Ok(())
        }
        ParamOp::Assign(colon, w) => {
            if !is_assignable_name(&p.name) {
                return Err(ExpandError::BadSubst(p.name.clone()));
            }
            let val = lookup_param(&p.name, env);
            let use_val = match &val {
                Some(s) => !*colon || !s.is_empty(),
                None => false,
            };
            if use_val {
                if let Some(s) = val {
                    st.push_str(&s, in_double, splittable);
                }
            } else {
                let assigned = expand_one_depth(w, env, st.depth)?;
                env.set(&p.name, &assigned);
                st.push_str(&assigned, in_double, splittable);
            }
            Ok(())
        }
        ParamOp::Error(colon, w) => {
            let val = lookup_param(&p.name, env);
            let use_val = match &val {
                Some(s) => !*colon || !s.is_empty(),
                None => false,
            };
            if use_val {
                if let Some(s) = val {
                    st.push_str(&s, in_double, splittable);
                }
                Ok(())
            } else {
                let msg = if w.is_empty() {
                    String::from("parameter null or not set")
                } else {
                    let expanded = expand_one_depth(w, env, st.depth)?;
                    if expanded.is_empty() {
                        String::from("parameter null or not set")
                    } else {
                        expanded
                    }
                };
                Err(ExpandError::Unset(p.name.clone(), msg))
            }
        }
        ParamOp::Alt(colon, w) => {
            let val = lookup_param(&p.name, env);
            let use_val = match &val {
                Some(s) => !*colon || !s.is_empty(),
                None => false,
            };
            if use_val {
                expand_word_stage1(w, in_double, !in_double, env, st)?;
            }
            Ok(())
        }
        ParamOp::TrimPrefix(longest, w) => {
            let val = lookup_param(&p.name, env).unwrap_or_default();
            let pat = expand_to_glob_pattern(w, env, st.depth)?;
            let trimmed = trim_prefix(&val, &pat, *longest);
            st.push_str(trimmed, in_double, splittable);
            Ok(())
        }
        ParamOp::TrimSuffix(longest, w) => {
            let val = lookup_param(&p.name, env).unwrap_or_default();
            let pat = expand_to_glob_pattern(w, env, st.depth)?;
            let trimmed = trim_suffix(&val, &pat, *longest);
            st.push_str(trimmed, in_double, splittable);
            Ok(())
        }
    }
}

fn expand_to_glob_pattern(
    w: &[Part],
    env: &mut dyn Env,
    depth: usize,
) -> Result<String, ExpandError> {
    let mut st = Stage1 {
        fields: vec![Vec::new()],
        depth,
        saw_empty_at: false,
    };
    expand_word_stage1(w, false, false, env, &mut st)?;
    let mut chars = Vec::new();
    for (idx, field) in st.fields.into_iter().enumerate() {
        if idx > 0 {
            chars.push(ExpChar {
                ch: ' ',
                quoted: false,
                splittable: false,
            });
        }
        for tok in field {
            if let ExpToken::Char(ec) = tok {
                chars.push(ec);
            }
        }
    }
    Ok(build_glob_pattern(&chars))
}

/// Expand a slice of shell words (tilde, param, cmd, arith, IFS split, glob, quote removal).
pub fn expand(words: &[Word], env: &mut dyn Env) -> Result<Vec<String>, ExpandError> {
    let mut out = Vec::new();
    for word in words {
        let mut st = Stage1::new();
        expand_word_stage1(word, false, false, env, &mut st)?;
        let ifs = ifs_string(env);
        for pre in st.fields {
            for field in split_by_ifs(&pre, &ifs) {
                out.extend(expand_pathname(&field, env));
            }
        }
    }
    Ok(out)
}

fn expand_one_depth(word: &Word, env: &mut dyn Env, depth: usize) -> Result<String, ExpandError> {
    let mut st = Stage1 {
        fields: vec![Vec::new()],
        depth,
        saw_empty_at: false,
    };
    expand_word_stage1(word, false, false, env, &mut st)?;
    let mut out = String::new();
    for (idx, field) in st.fields.into_iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        for tok in field {
            if let ExpToken::Char(ec) = tok {
                out.push(ec.ch);
            }
        }
    }
    Ok(out)
}

/// Expand a single word without field splitting or pathname expansion.
pub fn expand_one(word: &Word, env: &mut dyn Env) -> Result<String, ExpandError> {
    expand_one_depth(word, env, 0)
}
