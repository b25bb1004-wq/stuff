//! POSIX shell execution engine (`Shell`).

mod alias;
mod builtins;
mod external;
mod pipeline;
mod printf;
mod redir;
mod test_cmd;

use std::collections::{BTreeMap, BTreeSet};
use std::format;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::prelude::rust_2021::*;
use std::sync::{Arc, Mutex};

use crate::ast::{self, AndOr, Command, Script, Simple};
use crate::expand::{expand, expand_one};
use crate::glob::glob_match;
use crate::types::{Env, ExpandError};

use alias::{expand_alias, Expanded};
use builtins::{is_builtin, run_builtin};
use external::run_external;
use pipeline::{exec_pipeline, spawn_background, Job};
use redir::{FdTable, IoStreams};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlFlow {
    None,
    Break(usize),
    Continue(usize),
    Return(i32),
    Exit(i32),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ShellOpts {
    pub(crate) errexit: bool,
    pub(crate) nounset: bool,
    pub(crate) xtrace: bool,
    pub(crate) noclobber: bool,
    pub(crate) pipefail: bool,
}

impl ShellOpts {
    fn flags_string(&self) -> String {
        let mut s = String::new();
        if self.errexit {
            s.push('e');
        }
        if self.nounset {
            s.push('u');
        }
        if self.xtrace {
            s.push('x');
        }
        if self.noclobber {
            s.push('C');
        }
        s
    }
}

/// POSIX shell execution state and environment.
pub struct Shell {
    pub(crate) vars: BTreeMap<String, String>,
    pub(crate) exported: BTreeSet<String>,
    pub(crate) functions: BTreeMap<String, Command>,
    pub(crate) positional: Vec<String>,
    pub(crate) arg0: String,
    pub(crate) cwd: PathBuf,
    pub(crate) last_status: i32,
    pub(crate) exited: Option<i32>,
    pub(crate) last_bg_pid: Option<u32>,
    pub(crate) pid: u32,
    pub(crate) opts: ShellOpts,
    pub(crate) jobs: Vec<Job>,
    pub(crate) next_job_id: usize,
    pub(crate) local_scopes: Vec<BTreeMap<String, Option<String>>>,
    pub(crate) loop_depth: usize,
    pub(crate) func_depth: usize,
    pub(crate) source_depth: usize,
    pub(crate) flow: ControlFlow,
    pub(crate) suppress_errexit: bool,
    pub(crate) cmd_stderr: Option<Arc<Mutex<Vec<u8>>>>,
    /// When set, external programs inherit the process's real stdin, stdout and stderr.
    pub(crate) inherit_stdio: bool,
    /// Backslash rule used when parsing scripts run by this shell.
    pub(crate) escapes: crate::types::Escapes,
    /// Trap actions by signal name (`EXIT`, `INT`, `TERM`); only `EXIT` runs.
    pub(crate) traps: BTreeMap<String, String>,
    /// Alias definitions by name, from `alias name=text`.
    pub(crate) aliases: BTreeMap<String, String>,
    /// File-creation mask set by `umask` (stored only; Windows has no mode bits to apply it to).
    pub(crate) umask: u32,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Create a new shell importing the current process environment as exported variables.
    pub fn new() -> Shell {
        let mut vars = BTreeMap::new();
        let mut exported = BTreeSet::new();
        // std::env::vars panics on wasm32-unknown-unknown (no process environment there).
        if !cfg!(target_family = "wasm") {
            for (k, v) in std::env::vars() {
                exported.insert(k.clone());
                vars.insert(k, v);
            }
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if !vars.contains_key("PWD") {
            vars.insert("PWD".to_string(), cwd.to_string_lossy().into_owned());
        }
        Shell {
            vars,
            exported,
            functions: BTreeMap::new(),
            positional: Vec::new(),
            arg0: "krypton".to_string(),
            cwd,
            last_status: 0,
            exited: None,
            last_bg_pid: None,
            // std::process::id panics on wasm32-unknown-unknown (the site runs the shell there).
            pid: if cfg!(target_family = "wasm") {
                1
            } else {
                std::process::id()
            },
            opts: ShellOpts::default(),
            jobs: Vec::new(),
            next_job_id: 1,
            local_scopes: Vec::new(),
            loop_depth: 0,
            func_depth: 0,
            source_depth: 0,
            flow: ControlFlow::None,
            suppress_errexit: false,
            cmd_stderr: None,
            inherit_stdio: false,
            escapes: crate::types::Escapes::Posix,
            traps: BTreeMap::new(),
            aliases: BTreeMap::new(),
            umask: 0o022,
        }
    }

    /// Set `$0` (the script's name as given, or the `-c` ARG0).
    pub fn set_arg0(&mut self, name: &str) {
        self.arg0 = name.to_string();
    }

    /// Run the `EXIT` trap, if set, as the shell finishes with `status`. An `exit` inside the trap
    /// replaces the status; otherwise `status` is returned.
    pub fn run_exit_trap(
        &mut self,
        status: i32,
        stdin: &mut dyn Read,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> i32 {
        let Some(action) = self.traps.remove("EXIT") else {
            return status;
        };
        self.exited = None;
        self.flow = ControlFlow::None;
        self.last_status = status;
        self.run(&action, stdin, stdout, stderr);
        self.exited.unwrap_or(status)
    }

    /// Let child programs use the process's real console or terminal streams (interactive use).
    pub fn set_inherit_stdio(&mut self, on: bool) {
        self.inherit_stdio = on;
    }

    /// Set the backslash rule for scripts run by this shell (the binary sets the host rule).
    pub fn set_escapes(&mut self, esc: crate::types::Escapes) {
        self.escapes = esc;
    }

    /// The shell's current working directory.
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// Value of a shell variable (imported environment variables included), if set.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// Set the shell's current working directory.
    pub fn set_cwd(&mut self, dir: PathBuf) {
        self.cwd = dir;
        self.vars
            .insert("PWD".to_string(), self.cwd.to_string_lossy().into_owned());
    }

    /// Return the exit status of the most recently executed command.
    pub fn status(&self) -> i32 {
        self.last_status
    }

    /// Return `Some(code)` after the `exit` builtin (or `set -e` exit) has fired.
    pub fn exited(&self) -> Option<i32> {
        self.exited
    }

    /// Run a whole script; output goes to the given writers. Returns the last command's status.
    pub fn run(
        &mut self,
        src: &str,
        stdin: &mut dyn Read,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> i32 {
        let script = match crate::parse(src, self.escapes) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(stderr, "krypton: {}:{}: {}", e.line, e.col, e.msg);
                self.last_status = 2;
                return 2;
            }
        };

        let prev_err = self.cmd_stderr.take();
        let err_cap = Arc::new(Mutex::new(Vec::new()));
        self.cmd_stderr = Some(Arc::clone(&err_cap));

        let mut fds = FdTable::new_default();
        let mut io = IoStreams {
            stdin,
            stdout,
            stderr,
        };
        let st = self.exec_script(&script, &mut fds, &mut io);

        if let Ok(mut buf) = err_cap.lock() {
            if !buf.is_empty() {
                let _ = io.stderr.write_all(&buf);
                let _ = io.stderr.flush();
                buf.clear();
            }
        }
        self.cmd_stderr = prev_err;
        st
    }

    pub(crate) fn clone_subshell(&self) -> Shell {
        Shell {
            vars: self.vars.clone(),
            exported: self.exported.clone(),
            functions: self.functions.clone(),
            positional: self.positional.clone(),
            arg0: self.arg0.clone(),
            cwd: self.cwd.clone(),
            last_status: self.last_status,
            exited: None,
            last_bg_pid: self.last_bg_pid,
            pid: self.pid,
            opts: self.opts.clone(),
            jobs: Vec::new(),
            next_job_id: 1,
            local_scopes: Vec::new(),
            loop_depth: 0,
            func_depth: 0,
            source_depth: 0,
            flow: ControlFlow::None,
            suppress_errexit: self.suppress_errexit,
            cmd_stderr: self.cmd_stderr.clone(),
            inherit_stdio: self.inherit_stdio,
            escapes: self.escapes,
            // Subshells do not inherit traps (POSIX: traps reset in a subshell).
            traps: BTreeMap::new(),
            aliases: self.aliases.clone(),
            umask: self.umask,
        }
    }

    pub(crate) fn exec_script(
        &mut self,
        script: &Script,
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        for item in &script.items {
            self.exec_and_or(item, fds, io);
            self.flush_cmd_stderr(io);
            if self.exited.is_some() || self.flow != ControlFlow::None {
                break;
            }
        }
        self.last_status
    }

    fn flush_cmd_stderr(&self, io: &mut IoStreams<'_>) {
        if let Some(cap) = &self.cmd_stderr {
            if let Ok(mut buf) = cap.lock() {
                if !buf.is_empty() {
                    let _ = io.stderr.write_all(&buf);
                    let _ = io.stderr.flush();
                    buf.clear();
                }
            }
        }
    }

    pub(crate) fn exec_and_or(
        &mut self,
        and_or: &AndOr,
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        if and_or.background {
            return spawn_background(self, and_or, fds);
        }

        let prev_suppress = self.suppress_errexit;
        let has_rest = !and_or.rest.is_empty();
        self.suppress_errexit = prev_suppress || has_rest;
        exec_pipeline(self, &and_or.first, fds, io);
        self.suppress_errexit = prev_suppress;
        if self.exited.is_some() || self.flow != ControlFlow::None {
            return self.last_status;
        }

        for (idx, (is_and, pipe)) in and_or.rest.iter().enumerate() {
            let is_last = idx + 1 == and_or.rest.len();
            let should_run = if *is_and {
                self.last_status == 0
            } else {
                self.last_status != 0
            };
            if should_run {
                self.suppress_errexit = prev_suppress || !is_last;
                exec_pipeline(self, pipe, fds, io);
                self.suppress_errexit = prev_suppress;
                if self.exited.is_some() || self.flow != ControlFlow::None {
                    break;
                }
            }
        }
        self.last_status
    }

    pub(crate) fn exec_command(
        &mut self,
        cmd: &Command,
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        match cmd {
            Command::Simple(simple) => {
                let st = self.exec_simple(simple, fds, io);
                self.last_status = st;
                if !self.suppress_errexit
                    && self.opts.errexit
                    && st != 0
                    && self.flow == ControlFlow::None
                {
                    self.exited = Some(st);
                    self.flow = ControlFlow::Exit(st);
                }
                st
            }
            Command::Subshell(script, redirs) => {
                let mut cmd_fds = fds.clone();
                if let Err(e) = cmd_fds.apply_redirs(redirs, self) {
                    fds.write_err(io, &format!("krypton: {e}\n"));
                    self.last_status = 1;
                    return 1;
                }
                let mut sub = self.clone_subshell();
                let st = sub.exec_script(script, &mut cmd_fds, io);
                self.last_status = st;
                if !self.suppress_errexit
                    && self.opts.errexit
                    && st != 0
                    && self.flow == ControlFlow::None
                {
                    self.exited = Some(st);
                    self.flow = ControlFlow::Exit(st);
                }
                st
            }
            Command::Group(script, redirs) => {
                let mut cmd_fds = fds.clone();
                if let Err(e) = cmd_fds.apply_redirs(redirs, self) {
                    fds.write_err(io, &format!("krypton: {e}\n"));
                    self.last_status = 1;
                    return 1;
                }
                self.exec_script(script, &mut cmd_fds, io)
            }
            Command::If {
                arms,
                otherwise,
                redirs,
            } => {
                let mut cmd_fds = fds.clone();
                if let Err(e) = cmd_fds.apply_redirs(redirs, self) {
                    fds.write_err(io, &format!("krypton: {e}\n"));
                    self.last_status = 1;
                    return 1;
                }
                for (cond, body) in arms {
                    let prev_suppress = self.suppress_errexit;
                    self.suppress_errexit = true;
                    let st = self.exec_script(cond, &mut cmd_fds, io);
                    self.suppress_errexit = prev_suppress;
                    if self.exited.is_some() || self.flow != ControlFlow::None {
                        return self.last_status;
                    }
                    if st == 0 {
                        return self.exec_script(body, &mut cmd_fds, io);
                    }
                }
                if let Some(else_body) = otherwise {
                    self.exec_script(else_body, &mut cmd_fds, io)
                } else {
                    self.last_status = 0;
                    0
                }
            }
            Command::Loop {
                until,
                cond,
                body,
                redirs,
            } => self.exec_loop(*until, cond, body, redirs, fds, io),
            Command::For {
                var,
                words,
                body,
                redirs,
            } => self.exec_for(var, words.as_deref(), body, redirs, fds, io),
            Command::Case { word, arms, redirs } => self.exec_case(word, arms, redirs, fds, io),
            Command::Func { name, body } => {
                self.functions.insert(name.clone(), (**body).clone());
                self.last_status = 0;
                0
            }
        }
    }

    fn exec_loop(
        &mut self,
        until: bool,
        cond: &Script,
        body: &Script,
        redirs: &[ast::Redir],
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        let mut cmd_fds = fds.clone();
        if let Err(e) = cmd_fds.apply_redirs(redirs, self) {
            fds.write_err(io, &format!("krypton: {e}\n"));
            self.last_status = 1;
            return 1;
        }
        self.loop_depth += 1;
        let mut last_body = 0;
        loop {
            let prev_suppress = self.suppress_errexit;
            self.suppress_errexit = true;
            let cst = self.exec_script(cond, &mut cmd_fds, io);
            self.suppress_errexit = prev_suppress;
            if self.exited.is_some() || self.flow != ControlFlow::None {
                break;
            }
            let ok = if until { cst != 0 } else { cst == 0 };
            if !ok {
                break;
            }
            last_body = self.exec_script(body, &mut cmd_fds, io);
            match self.flow {
                ControlFlow::None => {}
                ControlFlow::Break(n) => {
                    self.flow = if n <= 1 {
                        ControlFlow::None
                    } else {
                        ControlFlow::Break(n - 1)
                    };
                    last_body = 0;
                    break;
                }
                ControlFlow::Continue(n) => {
                    if n <= 1 {
                        self.flow = ControlFlow::None;
                        continue;
                    }
                    self.flow = ControlFlow::Continue(n - 1);
                    break;
                }
                ControlFlow::Return(_) | ControlFlow::Exit(_) => break,
            }
        }
        self.loop_depth -= 1;
        if self.flow == ControlFlow::None {
            self.last_status = last_body;
        }
        self.last_status
    }

    fn exec_for(
        &mut self,
        var: &str,
        words: Option<&[ast::Word]>,
        body: &Script,
        redirs: &[ast::Redir],
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        let mut cmd_fds = fds.clone();
        if let Err(e) = cmd_fds.apply_redirs(redirs, self) {
            fds.write_err(io, &format!("krypton: {e}\n"));
            self.last_status = 1;
            return 1;
        }
        let items = match words {
            Some(ws) => match expand(ws, self) {
                Ok(v) => v,
                Err(e) => return self.handle_expand_err(e, fds, io),
            },
            None => self.positional.clone(),
        };
        self.loop_depth += 1;
        let mut last_body = 0;
        for item in items {
            self.vars.insert(var.to_string(), item);
            last_body = self.exec_script(body, &mut cmd_fds, io);
            match self.flow {
                ControlFlow::None => {}
                ControlFlow::Break(n) => {
                    self.flow = if n <= 1 {
                        ControlFlow::None
                    } else {
                        ControlFlow::Break(n - 1)
                    };
                    last_body = 0;
                    break;
                }
                ControlFlow::Continue(n) => {
                    if n <= 1 {
                        self.flow = ControlFlow::None;
                        continue;
                    }
                    self.flow = ControlFlow::Continue(n - 1);
                    break;
                }
                ControlFlow::Return(_) | ControlFlow::Exit(_) => break,
            }
        }
        self.loop_depth -= 1;
        if self.flow == ControlFlow::None {
            self.last_status = last_body;
        }
        self.last_status
    }

    fn exec_case(
        &mut self,
        word: &ast::Word,
        arms: &[(Vec<ast::Word>, Script)],
        redirs: &[ast::Redir],
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        let mut cmd_fds = fds.clone();
        if let Err(e) = cmd_fds.apply_redirs(redirs, self) {
            fds.write_err(io, &format!("krypton: {e}\n"));
            self.last_status = 1;
            return 1;
        }
        let subject = match expand_one(word, self) {
            Ok(s) => s,
            Err(e) => return self.handle_expand_err(e, fds, io),
        };
        self.last_status = 0;
        for (patterns, body) in arms {
            for pat_word in patterns {
                let pat = match expand_one(pat_word, self) {
                    Ok(p) => p,
                    Err(e) => return self.handle_expand_err(e, fds, io),
                };
                if glob_match(&pat, &subject) {
                    return self.exec_script(body, &mut cmd_fds, io);
                }
            }
        }
        0
    }

    fn handle_expand_err(
        &mut self,
        e: ExpandError,
        fds: &mut FdTable,
        io: &mut IoStreams<'_>,
    ) -> i32 {
        match &e {
            ExpandError::Unset(name, msg) => {
                fds.write_err(io, &format!("krypton: {name}: {msg}\n"));
                self.last_status = 1;
                self.exited = Some(1);
                self.flow = ControlFlow::Exit(1);
            }
            _ => {
                fds.write_err(io, &format!("krypton: {e:?}\n"));
                self.last_status = 1;
            }
        }
        1
    }

    fn exec_simple(&mut self, simple: &Simple, fds: &mut FdTable, io: &mut IoStreams<'_>) -> i32 {
        let rewritten: Simple;
        let simple = match expand_alias(self, simple) {
            Ok(None) => simple,
            Ok(Some(Expanded::Simple(s))) => {
                rewritten = s;
                &rewritten
            }
            Ok(Some(Expanded::Script(script))) => return self.exec_script(&script, fds, io),
            Err(msg) => {
                fds.write_err(io, &format!("krypton: {msg}\n"));
                return 1;
            }
        };
        let args = match expand(&simple.words, self) {
            Ok(a) => a,
            Err(e) => return self.handle_expand_err(e, fds, io),
        };

        let mut expanded_assigns = Vec::with_capacity(simple.assigns.len());
        let mut has_cmd_subst = false;
        for (k, w) in &simple.assigns {
            if word_has_cmd(w) {
                has_cmd_subst = true;
            }
            match expand_one(w, self) {
                Ok(v) => expanded_assigns.push((k.clone(), v)),
                Err(e) => return self.handle_expand_err(e, fds, io),
            }
        }

        // Bare `exec`: its redirections change this shell's own descriptors for later commands.
        if args.len() == 1 && args[0] == "exec" {
            return match fds.apply_redirs(&simple.redirs, self) {
                Ok(()) => 0,
                Err(e) => {
                    fds.write_err(io, &format!("krypton: {e}\n"));
                    1
                }
            };
        }
        // `exec cmd args`: run cmd, then the shell exits with its status.
        let exec_cmd = args.len() > 1 && args[0] == "exec";
        let words: &[String] = if exec_cmd { &args[1..] } else { &args };

        let mut cmd_fds = fds.clone();
        if let Err(e) = cmd_fds.apply_redirs(&simple.redirs, self) {
            fds.write_err(io, &format!("krypton: {e}\n"));
            return 1;
        }

        let Some((cmd_name, cmd_args)) = words.split_first() else {
            for (k, v) in expanded_assigns {
                if self.opts.xtrace {
                    cmd_fds.write_err(io, &format!("+ {k}={v}\n"));
                }
                self.vars.insert(k, v);
            }
            return if has_cmd_subst { self.last_status } else { 0 };
        };

        if self.opts.xtrace {
            let mut trace = String::from("+ ");
            for (k, v) in &expanded_assigns {
                trace.push_str(&format!("{k}={v} "));
            }
            trace.push_str(cmd_name);
            for a in cmd_args {
                trace.push(' ');
                trace.push_str(a);
            }
            trace.push('\n');
            cmd_fds.write_err(io, &trace);
        }

        let st = 'run: {
            if let Some(func_body) = self.functions.get(cmd_name).cloned() {
                let saved = self.push_temp_assigns(&expanded_assigns);
                let old_pos = std::mem::replace(&mut self.positional, cmd_args.to_vec());
                self.local_scopes.push(BTreeMap::new());
                self.func_depth += 1;

                let mut st = self.exec_command(&func_body, &mut cmd_fds, io);

                self.func_depth -= 1;
                if let Some(scope) = self.local_scopes.pop() {
                    for (k, old_val) in scope {
                        if let Some(v) = old_val {
                            self.vars.insert(k, v);
                        } else {
                            self.vars.remove(&k);
                        }
                    }
                }
                self.positional = old_pos;
                if let ControlFlow::Return(code) = self.flow {
                    self.flow = ControlFlow::None;
                    st = code;
                }
                self.pop_temp_assigns(saved);
                break 'run st;
            }

            if is_builtin(cmd_name) {
                let saved = self.push_temp_assigns(&expanded_assigns);
                let st = run_builtin(cmd_name, cmd_args, self, &mut cmd_fds, io);
                self.pop_temp_assigns(saved);
                break 'run st;
            }

            run_external(
                cmd_name,
                cmd_args,
                &expanded_assigns,
                self,
                &mut cmd_fds,
                io,
            )
        };
        if exec_cmd {
            self.exited = Some(st);
            self.flow = ControlFlow::Exit(st);
        }
        st
    }

    fn push_temp_assigns(
        &mut self,
        assigns: &[(String, String)],
    ) -> Vec<(String, Option<String>, bool)> {
        let mut saved = Vec::with_capacity(assigns.len());
        for (k, v) in assigns {
            let old_v = self.vars.insert(k.clone(), v.clone());
            let was_exp = !self.exported.insert(k.clone());
            saved.push((k.clone(), old_v, was_exp));
        }
        saved
    }

    fn pop_temp_assigns(&mut self, saved: Vec<(String, Option<String>, bool)>) {
        for (k, old_v, was_exp) in saved.into_iter().rev() {
            if let Some(v) = old_v {
                self.vars.insert(k.clone(), v);
            } else {
                self.vars.remove(&k);
            }
            if !was_exp {
                self.exported.remove(&k);
            }
        }
    }
}

fn word_has_cmd(word: &ast::Word) -> bool {
    word.iter().any(|p| match p {
        ast::Part::Cmd(_) => true,
        ast::Part::Double(parts) => word_has_cmd(parts),
        _ => false,
    })
}

impl Env for Shell {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            "?" => Some(self.last_status.to_string()),
            "$" => Some(self.pid.to_string()),
            "!" => self.last_bg_pid.map(|p| p.to_string()),
            "-" => Some(self.opts.flags_string()),
            "0" => Some(self.arg0.clone()),
            _ => self.vars.get(name).cloned(),
        }
    }

    fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(name.to_string(), value.to_string());
    }

    fn positional(&self) -> Vec<String> {
        self.positional.clone()
    }

    fn command(&mut self, s: &ast::Script) -> String {
        let mut sub = self.clone_subshell();
        let mut empty_in = io::empty();
        let mut out_buf = Vec::new();
        let mut err_buf = Vec::new();
        let mut fds = FdTable::new_default();
        let mut io = IoStreams {
            stdin: &mut empty_in,
            stdout: &mut out_buf,
            stderr: &mut err_buf,
        };
        let st = sub.exec_script(s, &mut fds, &mut io);
        self.last_status = st;
        if !err_buf.is_empty() {
            if let Some(cap) = &self.cmd_stderr {
                if let Ok(mut guard) = cap.lock() {
                    guard.extend_from_slice(&err_buf);
                }
            } else {
                let _ = io::stderr().write_all(&err_buf);
            }
        }
        while out_buf.last() == Some(&b'\n') {
            out_buf.pop();
        }
        String::from_utf8_lossy(&out_buf).into_owned()
    }

    fn home(&self, user: &str) -> Option<String> {
        if user.is_empty() {
            self.vars
                .get("HOME")
                .cloned()
                .or_else(|| self.vars.get("USERPROFILE").cloned())
        } else {
            None
        }
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        let target = if dir.is_empty() || dir == "." {
            self.cwd.clone()
        } else {
            let p = std::path::Path::new(dir);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.cwd.join(p)
            }
        };
        let Ok(entries) = std::fs::read_dir(target) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| name != "." && name != "..")
            .collect()
    }

    fn nounset(&self) -> bool {
        self.opts.nounset
    }
}
