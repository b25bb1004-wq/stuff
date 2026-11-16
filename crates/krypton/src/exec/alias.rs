//! `alias` and `unalias` builtins, and alias expansion of a simple command's first word.

use std::format;
use std::prelude::rust_2021::*;

use crate::ast::{Command, Script, Simple};
use crate::lexer::is_word_single_lit;

use super::redir::{FdTable, IoStreams};
use super::Shell;

/// Most alias bodies followed for one command. The name guard already stops cycles.
const MAX_CHAIN: usize = 64;

/// What an alias rewrite turns a simple command into.
pub(crate) enum Expanded {
    /// The command with its first word replaced by the alias body's command.
    Simple(Simple),
    /// A whole alias body with a pipe or list, used when the command has no other words.
    Script(Script),
}

/// Expand aliases on the first word of `simple`, following chains without repeating a name.
/// `Ok(None)` means no alias applies.
pub(crate) fn expand_alias(shell: &Shell, simple: &Simple) -> Result<Option<Expanded>, String> {
    let mut cur: Option<Simple> = None;
    let mut seen: Vec<String> = Vec::new();
    loop {
        let view = cur.as_ref().unwrap_or(simple);
        let Some(name) = view.words.first().and_then(is_word_single_lit) else {
            break;
        };
        if seen.len() >= MAX_CHAIN || seen.iter().any(|s| s == name) {
            break;
        }
        let Some(body) = shell.aliases.get(name) else {
            break;
        };
        seen.push(name.to_string());
        let mut script =
            crate::parse(body, shell.escapes).map_err(|e| format!("alias {name}: {}", e.msg))?;
        let only_simple = match (script.items.len(), script.items.first()) {
            (1, Some(item))
                if item.rest.is_empty()
                    && !item.background
                    && !item.first.bang
                    && item.first.cmds.len() == 1 =>
            {
                matches!(item.first.cmds.first(), Some(Command::Simple(_)))
            }
            _ => false,
        };
        if !only_simple {
            if view.words.len() == 1 && view.assigns.is_empty() && view.redirs.is_empty() {
                return Ok(Some(Expanded::Script(script)));
            }
            return Err(format!(
                "alias {name}: a body with a pipe or list takes no arguments"
            ));
        }
        let Some(item) = script.items.pop() else {
            break;
        };
        let Some(Command::Simple(inner)) = item.first.cmds.into_iter().next() else {
            break;
        };
        let mut words = inner.words;
        words.extend(view.words.iter().skip(1).cloned());
        let mut assigns = inner.assigns;
        assigns.extend(view.assigns.iter().cloned());
        let mut redirs = inner.redirs;
        redirs.extend(view.redirs.iter().cloned());
        cur = Some(Simple {
            assigns,
            words,
            redirs,
        });
    }
    Ok(cur.map(Expanded::Simple))
}

/// Quote an alias value for `alias` listing, as `'...'` with embedded quotes escaped.
fn alias_line(name: &str, value: &str) -> String {
    format!("alias {name}='{}'\n", value.replace('\'', "'\\''"))
}

/// Execute `alias [name[=text] ...]`.
pub(crate) fn builtin_alias(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.is_empty() {
        let mut out = String::new();
        for (name, value) in &shell.aliases {
            out.push_str(&alias_line(name, value));
        }
        fds.write_out(io, &out);
        return 0;
    }
    let mut status = 0;
    let mut out = String::new();
    for arg in args {
        match arg.split_once('=') {
            Some(("", _)) => {
                fds.write_err(io, "krypton: alias: invalid alias name\n");
                status = 1;
            }
            Some((name, value)) => {
                shell.aliases.insert(name.to_string(), value.to_string());
            }
            None => match shell.aliases.get(arg) {
                Some(value) => out.push_str(&alias_line(arg, value)),
                None => {
                    fds.write_err(io, &format!("krypton: alias: {arg}: not found\n"));
                    status = 1;
                }
            },
        }
    }
    fds.write_out(io, &out);
    status
}

/// Execute `unalias name ...` or `unalias -a`.
pub(crate) fn builtin_unalias(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.is_empty() {
        fds.write_err(
            io,
            "krypton: unalias: usage: unalias [-a] name [name ...]\n",
        );
        return 2;
    }
    if args.iter().any(|a| a == "-a") {
        shell.aliases.clear();
        return 0;
    }
    let mut status = 0;
    for name in args {
        if shell.aliases.remove(name).is_none() {
            fds.write_err(io, &format!("krypton: unalias: {name}: not found\n"));
            status = 1;
        }
    }
    status
}
