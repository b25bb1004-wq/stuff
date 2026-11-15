//! Krypton POSIX shell parser and word expander demo.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use krypton::ast::{AndOr, Command, Part, Pipeline, RedirOp, Script, Word};
use krypton::{expand, parse, Env};

use super::{check_input, Demo, Output};

const MAX_AST_DEPTH: usize = 24;
const MAX_AST_LINES: usize = 256;
const MAX_VARS: usize = 128;

const DEFAULT_SHELL: &str = "echo $X \"$HOME/notes\" $((X * 6)) ${UNSET:-fallback}";

/// Registered Krypton shell parser and expander demo.
pub const DEMO: Demo = Demo {
    id: "krypton",
    title: "Krypton Shell",
    default_input: DEFAULT_SHELL,
    run,
};

struct DemoEnv {
    vars: BTreeMap<String, String>,
}

impl DemoEnv {
    fn new() -> Self {
        let mut vars = BTreeMap::new();
        vars.insert(String::from("X"), String::from("7"));
        vars.insert(String::from("HOME"), String::from("/home/demo"));
        Self { vars }
    }
}

impl Env for DemoEnv {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            "?" => Some(String::from("0")),
            "$" => Some(String::from("100")),
            "0" => Some(String::from("krypton")),
            _ => self.vars.get(name).cloned(),
        }
    }

    fn set(&mut self, name: &str, value: &str) {
        if self.vars.len() < MAX_VARS || self.vars.contains_key(name) {
            self.vars.insert(name.to_owned(), value.to_owned());
        }
    }

    fn positional(&self) -> Vec<String> {
        Vec::new()
    }

    fn command(&mut self, _s: &Script) -> String {
        String::new()
    }

    fn home(&self, user: &str) -> Option<String> {
        if user.is_empty() {
            self.vars.get("HOME").cloned()
        } else {
            None
        }
    }

    fn list_dir(&self, _dir: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Parse one shell line, print its indented AST, and expand words with `X=7` and `HOME=/home/demo`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    let line = input.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() {
        return Err(String::from("empty shell line"));
    }
    if line.contains('\n') {
        return Err(String::from("expected one shell line"));
    }

    let script =
        parse(line).map_err(|e| format!("parse error at {}:{}: {}", e.line, e.col, e.msg))?;

    let mut out = String::new();
    out.push_str("=== parse tree ===\n");
    let mut lines_left = MAX_AST_LINES;
    fmt_script(&mut out, &script, 0, &mut lines_left);

    let mut env = DemoEnv::new();
    let mut words = Vec::new();
    collect_words(&script, &mut words, 0);

    out.push_str("\n=== expanded words (X=7, HOME=/home/demo) ===\n");
    let expanded = expand(&words, &mut env).map_err(|e| format!("expand error: {e:?}"))?;
    if expanded.is_empty() {
        out.push_str("  (no words)\n");
    } else {
        for (idx, word) in expanded.iter().enumerate() {
            let _ = writeln!(out, "  [{idx}] {word}");
        }
    }

    Ok(Output::Text(out))
}

fn push_indent(out: &mut String, depth: usize, lines_left: &mut usize, text: &str) -> bool {
    if *lines_left == 0 {
        return false;
    }
    *lines_left -= 1;
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(text);
    out.push('\n');
    true
}

fn fmt_script(out: &mut String, script: &Script, depth: usize, lines_left: &mut usize) {
    if depth > MAX_AST_DEPTH {
        let _ = push_indent(out, depth, lines_left, "... (depth limit)");
        return;
    }
    if !push_indent(out, depth, lines_left, "Script") {
        return;
    }
    for ao in &script.items {
        fmt_and_or(out, ao, depth + 1, lines_left);
    }
}

fn fmt_and_or(out: &mut String, ao: &AndOr, depth: usize, lines_left: &mut usize) {
    let label = if ao.background { "AndOr (&)" } else { "AndOr" };
    if !push_indent(out, depth, lines_left, label) {
        return;
    }
    fmt_pipeline(out, &ao.first, depth + 1, lines_left);
    for (is_and, pipe) in &ao.rest {
        let op = if *is_and { "&&" } else { "||" };
        if !push_indent(out, depth + 1, lines_left, op) {
            return;
        }
        fmt_pipeline(out, pipe, depth + 1, lines_left);
    }
}

fn fmt_pipeline(out: &mut String, pipe: &Pipeline, depth: usize, lines_left: &mut usize) {
    let label = if pipe.bang {
        "Pipeline (!)"
    } else {
        "Pipeline"
    };
    if !push_indent(out, depth, lines_left, label) {
        return;
    }
    for cmd in &pipe.cmds {
        fmt_cmd(out, cmd, depth + 1, lines_left);
    }
}

fn fmt_cmd(out: &mut String, cmd: &Command, depth: usize, lines_left: &mut usize) {
    if depth > MAX_AST_DEPTH {
        let _ = push_indent(out, depth, lines_left, "... (depth limit)");
        return;
    }
    match cmd {
        Command::Simple(s) => {
            if !push_indent(out, depth, lines_left, "Simple") {
                return;
            }
            for (k, w) in &s.assigns {
                if !push_indent(out, depth + 1, lines_left, &format!("Assign {k}=")) {
                    return;
                }
                fmt_word(out, w, depth + 2, lines_left);
            }
            for (i, w) in s.words.iter().enumerate() {
                if !push_indent(out, depth + 1, lines_left, &format!("Word[{i}]")) {
                    return;
                }
                fmt_word(out, w, depth + 2, lines_left);
            }
            for r in &s.redirs {
                let fd_str = r.fd.map_or_else(String::new, |f| f.to_string());
                let op_str = match &r.op {
                    RedirOp::In => "<",
                    RedirOp::Out => ">",
                    RedirOp::Append => ">>",
                    RedirOp::Clobber => ">|",
                    RedirOp::ReadWrite => "<>",
                    RedirOp::DupIn => "<&",
                    RedirOp::DupOut => ">&",
                    RedirOp::HereDoc { .. } => "<<",
                };
                if !push_indent(
                    out,
                    depth + 1,
                    lines_left,
                    &format!("Redir {fd_str}{op_str}"),
                ) {
                    return;
                }
                fmt_word(out, &r.target, depth + 2, lines_left);
            }
        }
        Command::Subshell(s, _) => {
            if push_indent(out, depth, lines_left, "Subshell") {
                fmt_script(out, s, depth + 1, lines_left);
            }
        }
        Command::Group(s, _) => {
            if push_indent(out, depth, lines_left, "Group") {
                fmt_script(out, s, depth + 1, lines_left);
            }
        }
        Command::If {
            arms, otherwise, ..
        } => {
            if !push_indent(out, depth, lines_left, "If") {
                return;
            }
            for (cond, body) in arms {
                if push_indent(out, depth + 1, lines_left, "Cond") {
                    fmt_script(out, cond, depth + 2, lines_left);
                }
                if push_indent(out, depth + 1, lines_left, "Then") {
                    fmt_script(out, body, depth + 2, lines_left);
                }
            }
            if let Some(els) = otherwise {
                if push_indent(out, depth + 1, lines_left, "Else") {
                    fmt_script(out, els, depth + 2, lines_left);
                }
            }
        }
        Command::Loop {
            until, cond, body, ..
        } => {
            let kind = if *until { "Until" } else { "While" };
            if push_indent(out, depth, lines_left, kind) {
                fmt_script(out, cond, depth + 1, lines_left);
                fmt_script(out, body, depth + 1, lines_left);
            }
        }
        Command::For { var, body, .. } => {
            if push_indent(out, depth, lines_left, &format!("For {var}")) {
                fmt_script(out, body, depth + 1, lines_left);
            }
        }
        Command::Case { word, arms, .. } => {
            if push_indent(
                out,
                depth,
                lines_left,
                &format!("Case ({} arms)", arms.len()),
            ) {
                fmt_word(out, word, depth + 1, lines_left);
            }
        }
        Command::Func { name, body } => {
            if push_indent(out, depth, lines_left, &format!("Func {name}")) {
                fmt_cmd(out, body, depth + 1, lines_left);
            }
        }
    }
}

fn fmt_word(out: &mut String, word: &Word, depth: usize, lines_left: &mut usize) {
    for part in word {
        fmt_part(out, part, depth, lines_left);
    }
}

fn fmt_part(out: &mut String, part: &Part, depth: usize, lines_left: &mut usize) {
    if depth > MAX_AST_DEPTH {
        let _ = push_indent(out, depth, lines_left, "... (depth limit)");
        return;
    }
    match part {
        Part::Lit(s) => {
            let _ = push_indent(out, depth, lines_left, &format!("Lit({s:?})"));
        }
        Part::Single(s) => {
            let _ = push_indent(out, depth, lines_left, &format!("Single({s:?})"));
        }
        Part::Escaped(c) => {
            let _ = push_indent(out, depth, lines_left, &format!("Escaped({c:?})"));
        }
        Part::Double(inner) => {
            if push_indent(out, depth, lines_left, "Double") {
                for p in inner {
                    fmt_part(out, p, depth + 1, lines_left);
                }
            }
        }
        Part::Param(p) => {
            let _ = push_indent(
                out,
                depth,
                lines_left,
                &format!("Param(${}, op={:?})", p.name, p.op),
            );
        }
        Part::Cmd(s) => {
            if push_indent(out, depth, lines_left, "CmdSubst") {
                fmt_script(out, s, depth + 1, lines_left);
            }
        }
        Part::Arith(expr) => {
            let _ = push_indent(out, depth, lines_left, &format!("Arith({expr:?})"));
        }
        Part::Tilde(u) => {
            let _ = push_indent(out, depth, lines_left, &format!("Tilde({u:?})"));
        }
    }
}

fn collect_words(script: &Script, out: &mut Vec<Word>, depth: usize) {
    if depth > MAX_AST_DEPTH {
        return;
    }
    for ao in &script.items {
        collect_pipe(&ao.first, out, depth);
        for (_, p) in &ao.rest {
            collect_pipe(p, out, depth);
        }
    }
}

fn collect_pipe(pipe: &Pipeline, out: &mut Vec<Word>, depth: usize) {
    for cmd in &pipe.cmds {
        collect_cmd(cmd, out, depth);
    }
}

fn collect_cmd(cmd: &Command, out: &mut Vec<Word>, depth: usize) {
    if depth > MAX_AST_DEPTH {
        return;
    }
    match cmd {
        Command::Simple(s) => out.extend(s.words.iter().cloned()),
        Command::Subshell(s, _) | Command::Group(s, _) => collect_words(s, out, depth + 1),
        Command::If {
            arms, otherwise, ..
        } => {
            for (c, b) in arms {
                collect_words(c, out, depth + 1);
                collect_words(b, out, depth + 1);
            }
            if let Some(els) = otherwise {
                collect_words(els, out, depth + 1);
            }
        }
        Command::Loop { cond, body, .. } => {
            collect_words(cond, out, depth + 1);
            collect_words(body, out, depth + 1);
        }
        Command::For { words, body, .. } => {
            if let Some(ws) = words {
                out.extend(ws.iter().cloned());
            }
            collect_words(body, out, depth + 1);
        }
        Command::Case { word, arms, .. } => {
            out.push(word.clone());
            for (_, b) in arms {
                collect_words(b, out, depth + 1);
            }
        }
        Command::Func { body, .. } => collect_cmd(body, out, depth + 1),
    }
}
