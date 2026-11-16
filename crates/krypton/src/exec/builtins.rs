//! POSIX shell builtins.

use std::format;
use std::path::{Component, Path, PathBuf};
use std::prelude::rust_2021::*;
use std::vec;

use crate::types::is_valid_name;

use super::external::{run_external, which};
use super::printf::builtin_printf;
use super::redir::{FdTable, IoStreams};
use super::test_cmd::builtin_test;
use super::{ControlFlow, Shell};

/// Check if `name` is a built-in shell command.
pub(crate) fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        ":" | "true"
            | "false"
            | "echo"
            | "printf"
            | "cd"
            | "pwd"
            | "exit"
            | "export"
            | "unset"
            | "set"
            | "shift"
            | "read"
            | "test"
            | "["
            | "return"
            | "break"
            | "continue"
            | "eval"
            | "."
            | "wait"
            | "jobs"
            | "type"
            | "local"
            | "command"
            | "trap"
            | "alias"
            | "unalias"
            | "umask"
            | "hash"
            | "kill"
    )
}

/// Dispatch a built-in command.
pub(crate) fn run_builtin(
    name: &str,
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    match name {
        ":" | "true" => 0,
        "false" => 1,
        "echo" => builtin_echo(args, fds, io),
        "printf" => builtin_printf(args, fds, io),
        "cd" => builtin_cd(args, shell, fds, io),
        "pwd" => builtin_pwd(args, shell, fds, io),
        "exit" => builtin_exit(args, shell, fds, io),
        "export" => builtin_export(args, shell, fds, io),
        "unset" => builtin_unset(args, shell, fds, io),
        "set" => builtin_set(args, shell, fds, io),
        "shift" => builtin_shift(args, shell, fds, io),
        "read" => builtin_read(args, shell, fds, io),
        "test" => builtin_test(args, false, &shell.cwd, fds, io),
        "[" => builtin_test(args, true, &shell.cwd, fds, io),
        "return" => builtin_return(args, shell, fds, io),
        "break" => builtin_break_continue(args, shell, fds, io, true),
        "continue" => builtin_break_continue(args, shell, fds, io, false),
        "eval" => builtin_eval(args, shell, fds, io),
        "." => builtin_dot(args, shell, fds, io),
        "wait" => super::pipeline::builtin_wait(args, shell, fds, io),
        "jobs" => super::pipeline::builtin_jobs(shell, fds, io),
        "type" => builtin_type(args, shell, fds, io),
        "local" => builtin_local(args, shell, fds, io),
        "command" => builtin_command(args, shell, fds, io),
        "trap" => builtin_trap(args, shell, fds, io),
        "alias" => super::alias::builtin_alias(args, shell, fds, io),
        "unalias" => super::alias::builtin_unalias(args, shell, fds, io),
        "umask" => builtin_umask(args, shell, fds, io),
        // Nothing is cached, so there is nothing to forget.
        "hash" => 0,
        "kill" => builtin_kill(args, shell, fds, io),
        _ => 127,
    }
}

fn builtin_echo(args: &[String], fds: &mut FdTable, io: &mut IoStreams<'_>) -> i32 {
    let mut newline = true;
    let mut idx = 0;
    if args.first().map(String::as_str) == Some("-n") {
        newline = false;
        idx = 1;
    }
    let mut out = String::new();
    for (i, arg) in args[idx..].iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(arg);
    }
    if newline {
        out.push('\n');
    }
    if fds.write_fd(1, io, out.as_bytes()).is_err() {
        return 1;
    }
    0
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if can_pop {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn builtin_cd(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let mut idx = 0;
    while let Some(a) = args.get(idx) {
        if a == "-L" || a == "-P" {
            idx += 1;
        } else if a == "--" {
            idx += 1;
            break;
        } else {
            break;
        }
    }
    let target = match args.get(idx).map(String::as_str) {
        None => match shell.vars.get("HOME") {
            Some(h) if !h.is_empty() => h.clone(),
            _ => {
                fds.write_err(io, "krypton: cd: HOME not set\n");
                return 1;
            }
        },
        Some("-") => match shell.vars.get("OLDPWD") {
            Some(old) if !old.is_empty() => {
                fds.write_out(io, &format!("{old}\n"));
                old.clone()
            }
            _ => {
                fds.write_err(io, "krypton: cd: OLDPWD not set\n");
                return 1;
            }
        },
        Some(dir) => dir.to_string(),
    };

    let p = Path::new(&target);
    let candidate = if p.is_absolute() {
        normalize_path(p)
    } else {
        normalize_path(&shell.cwd.join(p))
    };

    if !std::fs::metadata(&candidate).is_ok_and(|m| m.is_dir()) {
        fds.write_err(io, &format!("krypton: cd: {target}: No such directory\n"));
        return 1;
    }

    let old = shell.cwd.to_string_lossy().into_owned();
    shell.vars.insert("OLDPWD".to_string(), old);
    shell.cwd = candidate;
    let new_pwd = shell.cwd.to_string_lossy().into_owned();
    shell.vars.insert("PWD".to_string(), new_pwd);
    0
}

fn builtin_pwd(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let physical = args.iter().any(|a| a == "-P");
    let path_str = if physical {
        let p = std::fs::canonicalize(&shell.cwd).unwrap_or_else(|_| shell.cwd.clone());
        let s = p.to_string_lossy().into_owned();
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    } else {
        shell.cwd.to_string_lossy().into_owned()
    };
    if fds
        .write_fd(1, io, format!("{path_str}\n").as_bytes())
        .is_err()
    {
        return 1;
    }
    0
}

fn builtin_exit(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let code = match args.first() {
        None => shell.last_status,
        Some(s) => match s.trim().parse::<i32>() {
            Ok(n) => n & 0xff,
            Err(_) => {
                fds.write_err(
                    io,
                    &format!("krypton: exit: {s}: numeric argument required\n"),
                );
                2
            }
        },
    };
    shell.exited = Some(code);
    shell.flow = ControlFlow::Exit(code);
    code
}

fn builtin_export(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.is_empty() || (args.len() == 1 && args[0] == "-p") {
        let mut names: Vec<&String> = shell.exported.iter().collect();
        names.sort();
        for name in names {
            if let Some(val) = shell.vars.get(name) {
                let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                fds.write_out(io, &format!("export {name}=\"{escaped}\"\n"));
            } else {
                fds.write_out(io, &format!("export {name}\n"));
            }
        }
        return 0;
    }
    let mut status = 0;
    for arg in args {
        if arg == "-p" {
            continue;
        }
        if let Some((name, val)) = arg.split_once('=') {
            if !is_valid_name(name) {
                fds.write_err(
                    io,
                    &format!("krypton: export: `{arg}': not a valid identifier\n"),
                );
                status = 1;
                continue;
            }
            shell.vars.insert(name.to_string(), val.to_string());
            shell.exported.insert(name.to_string());
        } else {
            if !is_valid_name(arg) {
                fds.write_err(
                    io,
                    &format!("krypton: export: `{arg}': not a valid identifier\n"),
                );
                status = 1;
                continue;
            }
            shell.exported.insert(arg.to_string());
        }
    }
    status
}

fn builtin_unset(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let mut func_mode = false;
    let mut idx = 0;
    while let Some(a) = args.get(idx) {
        if a == "-f" {
            func_mode = true;
            idx += 1;
        } else if a == "-v" {
            func_mode = false;
            idx += 1;
        } else if a == "--" {
            idx += 1;
            break;
        } else {
            break;
        }
    }
    let mut status = 0;
    for name in &args[idx..] {
        if !is_valid_name(name) {
            fds.write_err(
                io,
                &format!("krypton: unset: `{name}': not a valid identifier\n"),
            );
            status = 1;
            continue;
        }
        if func_mode {
            shell.functions.remove(name);
        } else {
            shell.vars.remove(name);
            shell.exported.remove(name);
        }
    }
    status
}

/// The long option names `set -o` accepts, with their current state.
fn set_opt_states(shell: &Shell) -> [(&'static str, bool); 5] {
    [
        ("errexit", shell.opts.errexit),
        ("nounset", shell.opts.nounset),
        ("xtrace", shell.opts.xtrace),
        ("noclobber", shell.opts.noclobber),
        ("pipefail", shell.opts.pipefail),
    ]
}

fn apply_set_opt(shell: &mut Shell, opt: &str, val: bool) -> bool {
    match opt {
        "errexit" => shell.opts.errexit = val,
        "nounset" => shell.opts.nounset = val,
        "xtrace" => shell.opts.xtrace = val,
        "noclobber" => shell.opts.noclobber = val,
        "pipefail" => shell.opts.pipefail = val,
        _ => return false,
    }
    true
}

/// An unknown `set` option is a fatal usage error: the script stops with status 2.
fn set_fatal(shell: &mut Shell, fds: &mut FdTable, io: &mut IoStreams<'_>, msg: &str) -> i32 {
    fds.write_err(io, msg);
    shell.exited = Some(2);
    shell.flow = ControlFlow::Exit(2);
    2
}

fn builtin_set(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.is_empty() {
        for (k, v) in &shell.vars {
            fds.write_out(io, &format!("{k}={v}\n"));
        }
        return 0;
    }
    let mut idx = 0;
    let mut set_positional = false;
    while let Some(arg) = args.get(idx) {
        if arg == "--" {
            idx += 1;
            set_positional = true;
            break;
        }
        if arg == "-" {
            idx += 1;
            set_positional = true;
            break;
        }
        if (arg.starts_with('-') || arg.starts_with('+')) && arg.len() > 1 {
            let enable = arg.starts_with('-');
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut ci = 0;
            while let Some(&c) = chars.get(ci) {
                match c {
                    'e' => shell.opts.errexit = enable,
                    'u' => shell.opts.nounset = enable,
                    'x' => shell.opts.xtrace = enable,
                    'C' => shell.opts.noclobber = enable,
                    'o' => {
                        // `set -o` / `set +o` without a name lists the options.
                        let Some(opt_name) = args.get(idx + 1) else {
                            for (name, on) in set_opt_states(shell) {
                                let state = if on { "on" } else { "off" };
                                fds.write_out(io, &format!("{name:<15}{state}\n"));
                            }
                            return 0;
                        };
                        idx += 1;
                        if !apply_set_opt(shell, opt_name, enable) {
                            let msg = format!("krypton: set: {opt_name}: invalid option\n");
                            return set_fatal(shell, fds, io, &msg);
                        }
                    }
                    other => {
                        let msg = format!("krypton: set: -{other}: invalid option\n");
                        return set_fatal(shell, fds, io, &msg);
                    }
                }
                ci += 1;
            }
            idx += 1;
        } else {
            set_positional = true;
            break;
        }
    }
    if set_positional {
        shell.positional = args[idx..].to_vec();
    }
    0
}

fn builtin_shift(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let n = match args.first() {
        None => 1usize,
        Some(s) => match s.parse::<usize>() {
            Ok(v) => v,
            Err(_) => {
                fds.write_err(
                    io,
                    &format!("krypton: shift: {s}: numeric argument required\n"),
                );
                return 1;
            }
        },
    };
    if n > shell.positional.len() {
        fds.write_err(io, "krypton: shift: shift count out of range\n");
        return 1;
    }
    shell.positional.drain(..n);
    0
}

fn split_read_line(line: &str, ifs: &str, count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    if ifs.is_empty() {
        let mut out = vec![line.to_string()];
        out.resize(count, String::new());
        return out;
    }
    let is_ifs = |c: char| ifs.contains(c);
    let is_ifs_ws = |c: char| matches!(c, ' ' | '\t' | '\n') && ifs.contains(c);

    let chars: Vec<char> = line.chars().collect();
    let mut pos = 0usize;
    while pos < chars.len() && is_ifs_ws(chars[pos]) {
        pos += 1;
    }

    let mut fields = Vec::with_capacity(count);
    for idx in 0..count {
        if pos >= chars.len() {
            fields.push(String::new());
            continue;
        }
        if idx + 1 == count {
            let mut end = chars.len();
            while end > pos && is_ifs_ws(chars[end - 1]) {
                end -= 1;
            }
            let rem: String = chars[pos..end].iter().collect();
            fields.push(rem);
            pos = chars.len();
        } else {
            let start = pos;
            while pos < chars.len() && !is_ifs(chars[pos]) {
                pos += 1;
            }
            let field: String = chars[start..pos].iter().collect();
            fields.push(field);
            while pos < chars.len() && is_ifs_ws(chars[pos]) {
                pos += 1;
            }
            if pos < chars.len() && is_ifs(chars[pos]) && !is_ifs_ws(chars[pos]) {
                pos += 1;
                while pos < chars.len() && is_ifs_ws(chars[pos]) {
                    pos += 1;
                }
            }
        }
    }
    fields
}

fn builtin_read(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let mut raw = false;
    let mut idx = 0;
    while let Some(a) = args.get(idx) {
        if a == "-r" {
            raw = true;
            idx += 1;
        } else if a == "--" {
            idx += 1;
            break;
        } else {
            break;
        }
    }
    let var_names = &args[idx..];
    for name in var_names {
        if !is_valid_name(name) {
            fds.write_err(
                io,
                &format!("krypton: read: `{name}': not a valid identifier\n"),
            );
            return 1;
        }
    }

    let mut bytes = Vec::new();
    let mut one = [0u8; 1];
    let mut eof = false;
    let mut escaped = false;

    loop {
        match fds.read_fd(0, io, &mut one) {
            Ok(0) => {
                eof = true;
                break;
            }
            Ok(_) => {
                let b = one[0];
                if !raw && escaped {
                    escaped = false;
                    if b == b'\n' {
                        continue;
                    }
                    bytes.push(b);
                } else if !raw && b == b'\\' {
                    escaped = true;
                } else if b == b'\n' {
                    break;
                } else {
                    bytes.push(b);
                }
            }
            Err(_) => {
                eof = true;
                break;
            }
        }
    }

    let line = String::from_utf8_lossy(&bytes).into_owned();
    let ifs = shell
        .vars
        .get("IFS")
        .cloned()
        .unwrap_or_else(|| " \t\n".to_string());

    if var_names.is_empty() {
        shell.vars.insert("REPLY".to_string(), line);
    } else {
        let fields = split_read_line(&line, &ifs, var_names.len());
        for (name, val) in var_names.iter().zip(fields) {
            shell.vars.insert(name.clone(), val);
        }
    }

    if eof {
        1
    } else {
        0
    }
}

fn builtin_return(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if shell.func_depth == 0 && shell.source_depth == 0 {
        fds.write_err(
            io,
            "krypton: return: can only `return' from a function or sourced script\n",
        );
        return 1;
    }
    let code = match args.first() {
        None => shell.last_status,
        Some(s) => match s.trim().parse::<i32>() {
            Ok(n) => n & 0xff,
            Err(_) => {
                fds.write_err(
                    io,
                    &format!("krypton: return: {s}: numeric argument required\n"),
                );
                2
            }
        },
    };
    shell.flow = ControlFlow::Return(code);
    code
}

fn builtin_break_continue(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
    is_break: bool,
) -> i32 {
    let cmd = if is_break { "break" } else { "continue" };
    let n = match args.first() {
        None => 1usize,
        Some(s) => match s.trim().parse::<usize>() {
            Ok(v) if v >= 1 => v,
            _ => {
                fds.write_err(
                    io,
                    &format!("krypton: {cmd}: {s}: numeric argument >= 1 required\n"),
                );
                return 1;
            }
        },
    };
    if shell.loop_depth == 0 {
        return 0;
    }
    let clamped = n.min(shell.loop_depth);
    shell.flow = if is_break {
        ControlFlow::Break(clamped)
    } else {
        ControlFlow::Continue(clamped)
    };
    0
}

fn builtin_eval(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let src = args.join(" ");
    if src.trim().is_empty() {
        return 0;
    }
    match crate::parse(&src, shell.escapes) {
        Ok(script) => shell.exec_script(&script, fds, io),
        Err(e) => {
            fds.write_err(
                io,
                &format!("krypton: eval: {}:{}: {}\n", e.line, e.col, e.msg),
            );
            2
        }
    }
}

fn resolve_dot_file(file: &str, shell: &Shell) -> Option<PathBuf> {
    if file.contains('/') || (cfg!(windows) && file.contains('\\')) {
        let p = Path::new(file);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            shell.cwd.join(p)
        };
        return full.is_file().then_some(full);
    }
    if let Some(path_var) = shell.vars.get("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(sep) {
            let base = if dir.is_empty() {
                shell.cwd.clone()
            } else {
                PathBuf::from(dir)
            };
            let cand = base.join(file);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    let local = shell.cwd.join(file);
    local.is_file().then_some(local)
}

fn builtin_dot(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let Some(file) = args.first() else {
        fds.write_err(io, "krypton: .: filename argument required\n");
        return 2;
    };
    let Some(path) = resolve_dot_file(file, shell) else {
        fds.write_err(io, &format!("krypton: .: {file}: file not found\n"));
        return 1;
    };
    let Ok(src) = std::fs::read_to_string(&path) else {
        fds.write_err(io, &format!("krypton: .: {file}: cannot read file\n"));
        return 1;
    };
    let script = match crate::parse(&src, shell.escapes) {
        Ok(s) => s,
        Err(e) => {
            fds.write_err(
                io,
                &format!("krypton: {file}:{}:{}: {}\n", e.line, e.col, e.msg),
            );
            return 2;
        }
    };
    shell.source_depth += 1;
    let st = shell.exec_script(&script, fds, io);
    shell.source_depth -= 1;
    if let ControlFlow::Return(code) = shell.flow {
        shell.flow = ControlFlow::None;
        return code;
    }
    st
}

fn builtin_type(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    // On Windows a path or a non-command operand is a file for cmd's `type`: hand the call over.
    let file_operand = |n: &String| {
        n.contains(['\\', '/'])
            || (!shell.functions.contains_key(n) && !is_builtin(n) && which(n, shell).is_none())
    };
    if cfg!(windows) && args.iter().any(file_operand) {
        return super::external::run_external("type", args, &[], shell, fds, io);
    }
    let mut status = 0;
    for name in args {
        if shell.functions.contains_key(name) {
            fds.write_out(io, &format!("{name} is a function\n"));
        } else if is_builtin(name) {
            fds.write_out(io, &format!("{name} is a shell builtin\n"));
        } else if let Some(p) = which(name, shell) {
            fds.write_out(io, &format!("{name} is {p}\n"));
        } else {
            fds.write_err(io, &format!("krypton: type: {name}: not found\n"));
            status = 1;
        }
    }
    status
}

fn builtin_local(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if shell.local_scopes.is_empty() {
        fds.write_err(io, "krypton: local: can only be used in a function\n");
        return 1;
    }
    let mut status = 0;
    for arg in args {
        let (name, val) = if let Some((k, v)) = arg.split_once('=') {
            (k, Some(v.to_string()))
        } else {
            (arg.as_str(), None)
        };
        if !is_valid_name(name) {
            fds.write_err(
                io,
                &format!("krypton: local: `{arg}': not a valid identifier\n"),
            );
            status = 1;
            continue;
        }
        if let Some(scope) = shell.local_scopes.last_mut() {
            if !scope.contains_key(name) {
                scope.insert(name.to_string(), shell.vars.get(name).cloned());
            }
        }
        if let Some(v) = val {
            shell.vars.insert(name.to_string(), v);
        }
    }
    status
}

/// `command -v NAME...` prints how each name resolves (exit 1 if one does not); `command NAME
/// args` runs NAME as a builtin or program, skipping shell functions.
fn builtin_command(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.first().map(String::as_str) == Some("-v") {
        let mut status = 0;
        for name in &args[1..] {
            let found = if is_builtin(name) || shell.functions.contains_key(name) {
                Some(name.clone())
            } else {
                which(name, shell)
            };
            match found {
                Some(p) => fds.write_out(io, &format!("{p}\n")),
                None => status = 1,
            }
        }
        return status;
    }
    let Some((name, rest)) = args.split_first() else {
        return 0;
    };
    if is_builtin(name) {
        run_builtin(name, rest, shell, fds, io)
    } else {
        run_external(name, rest, &[], shell, fds, io)
    }
}

/// Map a signal operand to its stored name. Only EXIT runs; INT and TERM are stored only.
/// `umask [mode]`: print the mask as four octal digits, or store a new one.
fn builtin_umask(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let Some(value) = args.first() else {
        fds.write_out(io, &format!("{:04o}\n", shell.umask));
        return 0;
    };
    match u32::from_str_radix(value, 8) {
        Ok(mode) if mode <= 0o777 => {
            shell.umask = mode;
            0
        }
        _ => {
            fds.write_err(io, &format!("krypton: umask: {value}: invalid mode\n"));
            1
        }
    }
}

/// `kill -0 PID`: answered here for this shell's own PID; other requests go to the external kill.
fn builtin_kill(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let (signal_zero, pids) = match args {
        [first, rest @ ..] if first.as_str() == "-0" => (true, rest),
        [first, sig, rest @ ..] if first.as_str() == "-s" && sig.as_str() == "0" => (true, rest),
        _ => (false, args),
    };
    let own = shell.pid;
    if signal_zero && !pids.is_empty() && pids.iter().all(|p| p.parse::<u32>().ok() == Some(own)) {
        return 0;
    }
    run_external("kill", args, &[], shell, fds, io)
}

fn trap_signal_name(sig: &str) -> Option<&'static str> {
    let upper = sig.to_ascii_uppercase();
    match upper.strip_prefix("SIG").unwrap_or(&upper) {
        "0" | "EXIT" => Some("EXIT"),
        "INT" => Some("INT"),
        "TERM" => Some("TERM"),
        _ => None,
    }
}

/// `trap ACTION SIGNAL...` sets a trap, `trap - SIGNAL...` clears it, and bare `trap` lists them.
fn builtin_trap(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.is_empty() || args == ["-p"] {
        let lines: Vec<String> = shell
            .traps
            .iter()
            .map(|(sig, act)| format!("trap -- '{}' {sig}\n", act.replace('\'', "'\''")))
            .collect();
        for line in lines {
            fds.write_out(io, &line);
        }
        return 0;
    }
    if args.len() < 2 {
        fds.write_err(io, "krypton: trap: usage: trap [-] ACTION SIGNAL...\n");
        return 2;
    }
    let action = &args[0];
    let mut status = 0;
    for sig in &args[1..] {
        let Some(name) = trap_signal_name(sig) else {
            fds.write_err(
                io,
                &format!("krypton: trap: {sig}: invalid signal specification\n"),
            );
            status = 1;
            continue;
        };
        if action == "-" {
            shell.traps.remove(name);
        } else {
            shell.traps.insert(name.to_string(), action.clone());
        }
    }
    status
}
