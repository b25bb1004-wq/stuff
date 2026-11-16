//! Krypton-powered interactive shell (`sh`) for Titan userland.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use krypton::ast::{AndOr, Command, Pipeline, Redir, RedirOp, Script, Simple};
use krypton::{expand, expand_one, parse, Env, Escapes};

use crate::sys::{self, FdWriter, O_APPEND, O_CREATE, O_READ, O_TRUNC, O_WRITE};

const MAX_LINE_BYTES: usize = 16 * 1024;

struct ShellEnv {
    vars: BTreeMap<String, String>,
    last_status: i32,
    pid: isize,
}

impl ShellEnv {
    fn new() -> Self {
        Self {
            vars: BTreeMap::new(),
            last_status: 0,
            pid: sys::getpid(),
        }
    }
}

impl Env for ShellEnv {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            "?" => Some(format!("{}", self.last_status)),
            "$" => Some(format!("{}", self.pid)),
            _ => self.vars.get(name).cloned(),
        }
    }

    fn set(&mut self, name: &str, value: &str) {
        self.vars.insert(String::from(name), String::from(value));
    }

    fn positional(&self) -> Vec<String> {
        Vec::new()
    }

    fn command(&mut self, _s: &Script) -> String {
        String::new()
    }

    fn home(&self, _user: &str) -> Option<String> {
        None
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        if !dir.starts_with('/') {
            return Vec::new();
        }
        let fd = sys::open(dir, O_READ);
        if fd < 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut raw = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            let n = sys::readdir(fd as usize, &mut buf);
            if n <= 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n as usize]);
            if raw.len() >= 16 * 1024 {
                break;
            }
        }
        let _ = sys::close(fd as usize);
        if let Ok(s) = core::str::from_utf8(&raw) {
            for line in s.lines() {
                if !line.is_empty() {
                    out.push(String::from(line));
                }
            }
        }
        out
    }
}

enum LineRead {
    Line(Vec<u8>),
    TooLong,
    Eof,
}

fn read_line() -> LineRead {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        let n = sys::read(0, &mut b);
        if n < 0 {
            return LineRead::Eof;
        }
        if n == 0 {
            return if line.is_empty() {
                LineRead::Eof
            } else {
                LineRead::Line(line)
            };
        }
        if b[0] == b'\n' {
            return LineRead::Line(line);
        }
        if line.len() < MAX_LINE_BYTES {
            line.push(b[0]);
        } else {
            // Drain until newline or EOF so subsequent lines stay aligned.
            while sys::read(0, &mut b) > 0 && b[0] != b'\n' {}
            return LineRead::TooLong;
        }
    }
}

fn unsupported(env: &mut ShellEnv) {
    let _ = writeln!(FdWriter(2), "sh: unsupported");
    env.last_status = 2;
}

fn close_opt_fd(fd: i64) {
    if fd >= 3 {
        let _ = sys::close(fd as usize);
    }
}

fn open_redirs(redirs: &[Redir], env: &mut ShellEnv) -> Result<(i64, i64), i32> {
    let mut stdin_fd: i64 = -1;
    let mut stdout_fd: i64 = -1;
    for r in redirs {
        let (is_in, flags) = match &r.op {
            RedirOp::In => {
                if r.fd.is_some_and(|f| f != 0) {
                    close_opt_fd(stdin_fd);
                    close_opt_fd(stdout_fd);
                    let _ = writeln!(FdWriter(2), "sh: unsupported");
                    return Err(2);
                }
                (true, O_READ)
            }
            RedirOp::Out | RedirOp::Clobber => {
                if r.fd.is_some_and(|f| f != 1) {
                    close_opt_fd(stdin_fd);
                    close_opt_fd(stdout_fd);
                    let _ = writeln!(FdWriter(2), "sh: unsupported");
                    return Err(2);
                }
                (false, O_WRITE | O_CREATE | O_TRUNC)
            }
            RedirOp::Append => {
                if r.fd.is_some_and(|f| f != 1) {
                    close_opt_fd(stdin_fd);
                    close_opt_fd(stdout_fd);
                    let _ = writeln!(FdWriter(2), "sh: unsupported");
                    return Err(2);
                }
                (false, O_WRITE | O_CREATE | O_APPEND)
            }
            _ => {
                close_opt_fd(stdin_fd);
                close_opt_fd(stdout_fd);
                let _ = writeln!(FdWriter(2), "sh: unsupported");
                return Err(2);
            }
        };
        let Ok(target) = expand_one(&r.target, env) else {
            close_opt_fd(stdin_fd);
            close_opt_fd(stdout_fd);
            let _ = writeln!(FdWriter(2), "sh: expand error");
            return Err(2);
        };
        let fd = sys::open(&target, flags);
        if fd < 0 {
            close_opt_fd(stdin_fd);
            close_opt_fd(stdout_fd);
            let _ = writeln!(FdWriter(2), "sh: {target}: error {fd}");
            return Err(1);
        }
        if is_in {
            close_opt_fd(stdin_fd);
            stdin_fd = fd as i64;
        } else {
            close_opt_fd(stdout_fd);
            stdout_fd = fd as i64;
        }
    }
    Ok((stdin_fd, stdout_fd))
}

fn exec_simple(simple: &Simple, env: &mut ShellEnv) {
    let words = match expand(&simple.words, env) {
        Ok(w) => w,
        Err(_) => {
            let _ = writeln!(FdWriter(2), "sh: expand error");
            env.last_status = 2;
            return;
        }
    };

    if words.is_empty() {
        for (name, val_word) in &simple.assigns {
            let Ok(val) = expand_one(val_word, env) else {
                let _ = writeln!(FdWriter(2), "sh: expand error");
                env.last_status = 2;
                return;
            };
            env.set(name, &val);
        }
        match open_redirs(&simple.redirs, env) {
            Ok((in_fd, out_fd)) => {
                close_opt_fd(in_fd);
                close_opt_fd(out_fd);
                env.last_status = 0;
            }
            Err(code) => env.last_status = code,
        }
        return;
    }

    let cmd = &words[0];
    if cmd == "cd" {
        unsupported(env);
        return;
    }
    if cmd == "exit" {
        let code = match words.get(1) {
            None => env.last_status,
            Some(arg) => match arg.parse::<i32>() {
                Ok(n) => n,
                Err(_) => {
                    let _ = writeln!(FdWriter(2), "sh: exit: bad number");
                    2
                }
            },
        };
        sys::exit(code);
    }

    let (stdin_fd, stdout_fd) = match open_redirs(&simple.redirs, env) {
        Ok(fds) => fds,
        Err(code) => {
            env.last_status = code;
            return;
        }
    };

    let mut c_args: Vec<Vec<u8>> = Vec::with_capacity(words.len());
    for w in &words {
        let mut buf = Vec::with_capacity(w.len() + 1);
        buf.extend_from_slice(w.as_bytes());
        buf.push(0);
        c_args.push(buf);
    }
    let ptrs: Vec<*const u8> = c_args.iter().map(|v| v.as_ptr()).collect();

    let pid = sys::spawn(cmd, &ptrs, stdin_fd, stdout_fd);
    close_opt_fd(stdin_fd);
    close_opt_fd(stdout_fd);

    if pid == -2 {
        let _ = writeln!(FdWriter(2), "sh: {cmd}: not found");
        env.last_status = 127;
    } else if pid < 0 {
        let _ = writeln!(FdWriter(2), "sh: {cmd}: error {pid}");
        env.last_status = 1;
    } else {
        let status = sys::wait(pid as usize);
        env.last_status = status as i32;
    }
}

fn exec_pipeline(pipe: &Pipeline, env: &mut ShellEnv) {
    if pipe.bang || pipe.cmds.len() != 1 {
        unsupported(env);
        return;
    }
    match &pipe.cmds[0] {
        Command::Simple(simple) => exec_simple(simple, env),
        _ => unsupported(env),
    }
}

fn exec_and_or(item: &AndOr, env: &mut ShellEnv) {
    if item.background {
        unsupported(env);
        return;
    }
    exec_pipeline(&item.first, env);
    for (is_and, next) in &item.rest {
        let should_run = (*is_and && env.last_status == 0) || (!*is_and && env.last_status != 0);
        if should_run {
            exec_pipeline(next, env);
        }
    }
}

pub fn run_sh() -> i32 {
    let mut env = ShellEnv::new();
    loop {
        if sys::write_all(1, b"$ ") < 0 {
            return 1;
        }
        let raw = match read_line() {
            LineRead::Eof => return 0,
            LineRead::TooLong => {
                let _ = writeln!(FdWriter(2), "sh: line too long");
                env.last_status = 2;
                continue;
            }
            LineRead::Line(bytes) => bytes,
        };
        let Ok(line) = core::str::from_utf8(&raw) else {
            let _ = writeln!(FdWriter(2), "sh: invalid utf8");
            env.last_status = 2;
            continue;
        };
        let script = match parse(line, Escapes::Posix) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(FdWriter(2), "sh: {}", e.msg);
                env.last_status = 2;
                continue;
            }
        };
        for item in &script.items {
            exec_and_or(item, &mut env);
        }
    }
}
