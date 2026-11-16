//! AI computers: their folders, the path fence, and running Monolith tools inside them.

use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use aci::envelope::{self, Code};
use aci::json::Json;

use crate::journal;
use crate::types::{CmdOut, Error, Result};

/// The Monolith tool names a computer may run. `monolith` itself is deliberately not one of them.
pub const TOOLS: [&str; 28] = [
    "aci",
    "argo",
    "athena",
    "aurum",
    "bench",
    "calliope",
    "chronos",
    "cobalt",
    "elysium",
    "hades",
    "helios",
    "indium",
    "iridium",
    "iridium-cli",
    "iris",
    "kairos",
    "krypton",
    "lithium",
    "logos",
    "mercury",
    "midas",
    "odyssey",
    "pandora",
    "silicon",
    "sphinx",
    "titan",
    "xenon",
    "zeusctl",
];

/// Most bytes kept from each output stream of a tool run.
const OUTPUT_CAP: usize = 256 * 1024;

/// A computer: `dir` is `$MONOLITH_HOME/computers/NAME`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Computer {
    /// Its name (`a-z`, `0-9`, `-`, `_`; 1 to 40 characters).
    pub name: String,
    /// Its folder.
    pub dir: PathBuf,
    /// Where its tool binaries are ([`tool_dir`] unless a test points it elsewhere).
    pub tools: PathBuf,
}

/// What a tool run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    /// Exit code (-1 when killed).
    pub exit: i32,
    /// Standard output, capped.
    pub stdout: Vec<u8>,
    /// Standard error, capped.
    pub stderr: Vec<u8>,
    /// True when the timeout killed it.
    pub timed_out: bool,
    /// Wall time in milliseconds.
    pub ms: u64,
}

/// `$MONOLITH_HOME`, or `~/.monolith`.
pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("MONOLITH_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(h);
    }
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let user = std::env::var_os(key).map_or_else(|| PathBuf::from("."), PathBuf::from);
    user.join(".monolith")
}

/// Where the Monolith tool binaries are: `$MONOLITH_BIN`, or the folder of the running program.
pub fn tool_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MONOLITH_BIN").filter(|v| !v.is_empty()) {
        return PathBuf::from(d);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Splits a command line into arguments without a shell: spaces separate, single and double
/// quotes group, a backslash escapes the next character outside single quotes.
///
/// Outside quotes a backslash escapes only a space, a quote or a backslash; any other backslash
/// is kept, so Windows-style paths such as `C:\dir` survive. Inside double quotes only `\"` and
/// `\\` are escapes. An unclosed quote or an empty line is `invalid`.
pub fn split_command_line(line: &str) -> Result<Vec<String>> {
    let unclosed = || Error::new(Code::Invalid, "unclosed quote");
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => cur.push(x),
                        None => return Err(unclosed()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek().copied() {
                            Some(n @ ('"' | '\\')) => {
                                cur.push(n);
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        Some(x) => cur.push(x),
                        None => return Err(unclosed()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.peek().copied() {
                    Some(n @ (' ' | '"' | '\\')) => {
                        cur.push(n);
                        chars.next();
                    }
                    _ => cur.push('\\'),
                }
            }
            other => {
                in_word = true;
                cur.push(other);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    if out.is_empty() {
        return Err(Error::new(Code::Invalid, "empty command")
            .hint("give a tool name, for example: chronos --describe"));
    }
    Ok(out)
}

/// The computer folder names must satisfy: 1 to 40 characters from `a-z`, `0-9`, `-` and `_`.
fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(
            Error::new(Code::Invalid, format!("bad computer name `{name}`"))
                .hint("use 1 to 40 characters from a-z, 0-9, - and _"),
        )
    }
}

fn computers_dir(home: &Path) -> PathBuf {
    home.join("computers")
}

fn io_error(context: &str, e: &std::io::Error) -> Error {
    Error::new(Code::Io, format!("{context}: {e}"))
}

fn denied_path(rel: &str, why: &str) -> Error {
    Error::new(Code::Denied, format!("path `{rel}` {why}"))
        .hint("paths are relative to the computer's root and must stay inside it")
}

/// Why a tool argument word is outside the computer, or `None` when it is fine.
fn outside_reason(word: &str) -> Option<String> {
    if word.split(['/', '\\']).any(|c| c == "..") {
        return Some("has a `..` part that climbs out of the computer".into());
    }
    let b = word.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some("starts with a drive letter, a path on the host".into());
    }
    if word.starts_with("\\\\") || word.starts_with("//") || word.starts_with('~') {
        return Some("names a path on the host".into());
    }
    if let Some(rest) = word.strip_prefix('/') {
        let first = rest.split(['/', '\\']).next().unwrap_or("");
        // `/c/Users` is how Unix-style shells on Windows spell the C drive.
        if first.len() == 1 && first.as_bytes()[0].is_ascii_alphabetic() {
            return Some("looks like a drive path on the host".into());
        }
        // A bare `/` is the root of a disk image or a division sign in SQL, not a host path.
        if !first.is_empty() && Path::new("/").join(first).exists() {
            return Some("names a folder of the host, not the computer".into());
        }
    }
    if let Some(idx) = word.find("://") {
        let after = &word[idx + 3..];
        let host = if let Some(inner) = after.strip_prefix('[') {
            inner.split(']').next().unwrap_or("")
        } else {
            after.split(['/', ':', '?']).next().unwrap_or("")
        };
        let loopback =
            host.eq_ignore_ascii_case("localhost") || host == "::1" || host.starts_with("127.");
        if !loopback {
            return Some(format!("reaches the host `{host}`, which is not loopback"));
        }
    }
    if let Some((host, port)) = word.split_once(':') {
        let ipv4 = host.split('.').count() == 4
            && host
                .split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        let port_ok = !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit());
        if ipv4 && port_ok && !host.starts_with("127.") {
            return Some(format!("reaches the host `{host}`, which is not loopback"));
        }
    }
    None
}

/// Reads one output stream on its own thread, keeping at most [`OUTPUT_CAP`] bytes but draining
/// the rest so the child never blocks on a full pipe.
fn drain<R: Read + Send + 'static>(mut r: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let room = OUTPUT_CAP.saturating_sub(kept.len());
                    kept.extend_from_slice(&buf[..room.min(n)]);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        kept
    })
}

/// Runs a program with cwd, extra env vars, piped and capped output, and a timeout.
fn run_program(
    program: &Path,
    args: &[String],
    cwd: &Path,
    envs: &[(&str, &OsStr)],
    timeout: Duration,
) -> Result<ExecResult> {
    let start = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => Error::new(
                Code::NotFound,
                format!("cannot find `{}`", program.display()),
            ),
            _ => io_error(&format!("cannot start `{}`", program.display()), &e),
        })?;
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let (exit, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(-1), false),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (-1, true);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(io_error("cannot wait for the tool", &e)),
        }
    };
    let stdout = out
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = err
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    Ok(ExecResult {
        exit,
        stdout,
        stderr,
        timed_out,
        ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Runs a program the way [`Computer::exec_in`] does, without the fence. Tests use it to check
/// the timeout and the output cap with programs that are not Monolith tools.
#[doc(hidden)]
pub fn run_program_for_test(
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<ExecResult> {
    run_program(program, args, cwd, &[], timeout)
}

impl Computer {
    /// Makes a new computer under `home`. `exists` if the name is taken, `invalid` for a bad name.
    pub fn create(home: &Path, name: &str) -> Result<Self> {
        check_name(name)?;
        let parent = computers_dir(home);
        fs::create_dir_all(&parent)
            .map_err(|e| io_error("cannot create the computers folder", &e))?;
        let dir = parent.join(name);
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                return Err(Error::new(
                    Code::Exists,
                    format!("a computer named `{name}` already exists"),
                )
                .hint("open it, or pick another name"));
            }
            Err(e) => return Err(io_error("cannot make the computer", &e)),
        }
        for sub in ["root", "memory"] {
            fs::create_dir_all(dir.join(sub))
                .map_err(|e| io_error("cannot make the computer", &e))?;
        }
        Ok(Self {
            name: name.into(),
            dir,
            tools: tool_dir(),
        })
    }

    /// Opens an existing computer. `not_found` if there is none.
    pub fn open(home: &Path, name: &str) -> Result<Self> {
        check_name(name)?;
        let dir = computers_dir(home).join(name);
        if !dir.is_dir() {
            return Err(
                Error::new(Code::NotFound, format!("no computer named `{name}`"))
                    .hint(format!("make it with: monolith new {name}")),
            );
        }
        Ok(Self {
            name: name.into(),
            dir,
            tools: tool_dir(),
        })
    }

    /// Names of all computers under `home`, sorted.
    pub fn list(home: &Path) -> Result<Vec<String>> {
        let parent = computers_dir(home);
        if !parent.is_dir() {
            return Ok(Vec::new());
        }
        let entries =
            fs::read_dir(&parent).map_err(|e| io_error("cannot list the computers", &e))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io_error("cannot list the computers", &e))?;
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Deletes a computer's folder.
    pub fn remove(home: &Path, name: &str) -> Result<()> {
        check_name(name)?;
        let dir = computers_dir(home).join(name);
        if !dir.is_dir() {
            return Err(Error::new(
                Code::NotFound,
                format!("no computer named `{name}`"),
            ));
        }
        fs::remove_dir_all(&dir).map_err(|e| io_error("cannot delete the computer", &e))
    }

    /// Its `root/` folder (the tools' working directory).
    pub fn root(&self) -> PathBuf {
        self.dir.join("root")
    }

    /// Turns a path relative to `root/` into a host path, or `denied` if it leaves `root/`.
    /// Checks the text first (no absolute paths, no `~`, no `..`), then the deepest existing
    /// ancestor, so a symlink inside `root/` cannot point the path outside it.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        if rel.is_empty() {
            return Err(denied_path(rel, "is empty"));
        }
        let b = rel.as_bytes();
        if rel.starts_with(['/', '\\'])
            || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        {
            return Err(denied_path(rel, "is an absolute path"));
        }
        if rel.starts_with('~') {
            return Err(denied_path(rel, "starts with `~`"));
        }
        if rel.split(['/', '\\']).any(|c| c == "..") {
            return Err(denied_path(rel, "has a `..` part"));
        }
        let root = self
            .root()
            .canonicalize()
            .map_err(|e| io_error("cannot open the computer's root", &e))?;
        let path = self.root().join(rel);
        let mut ancestor = path.as_path();
        while ancestor.symlink_metadata().is_err() {
            match ancestor.parent() {
                Some(p) => ancestor = p,
                None => break,
            }
        }
        let real = ancestor
            .canonicalize()
            .map_err(|_| denied_path(rel, "cannot be checked safely"))?;
        if !real.starts_with(&root) {
            return Err(denied_path(
                rel,
                "leads outside the computer through a link",
            ));
        }
        Ok(path)
    }

    /// Checks a tool's arguments against the fence: the tool must be a Monolith tool, and no
    /// word may leave `root/` or name a host outside loopback. `denied` says which word and why.
    ///
    /// This is a lexical check. It stops mistakes and careless paths; it is not a security
    /// sandbox, and a tool that builds its own paths is not limited by it.
    pub fn check_args(&self, argv: &[String]) -> Result<()> {
        let Some(tool) = argv.first() else {
            return Err(Error::new(Code::Usage, "no tool given")
                .hint("give a tool name, for example: chronos --describe"));
        };
        if !TOOLS.contains(&tool.as_str()) {
            return Err(
                Error::new(Code::Denied, format!("`{tool}` is not a Monolith tool"))
                    .hint(format!("the tools are: {}", TOOLS.join(" "))),
            );
        }
        for arg in &argv[1..] {
            for word in arg.split(|c: char| c.is_whitespace() || c == '=') {
                if let Some(why) = outside_reason(word) {
                    return Err(Error::new(Code::Denied, format!("`{word}` {why}"))
                        .hint("this is outside the computer; only the operator can do that"));
                }
            }
        }
        Ok(())
    }

    /// Runs `argv[0]` (a Monolith tool) with the rest as arguments, cwd `root/`, after
    /// [`Computer::check_args`]. Kills it at `timeout`. Output is capped at 256 KiB per stream.
    pub fn exec(&self, argv: &[String], timeout: Duration) -> Result<ExecResult> {
        self.exec_in(&self.tools, argv, timeout)
    }

    /// Like [`Computer::exec`], but the tool binaries are looked for in `tool_dir`.
    pub fn exec_in(
        &self,
        tool_dir: &Path,
        argv: &[String],
        timeout: Duration,
    ) -> Result<ExecResult> {
        self.check_args(argv)?;
        let Some(tool) = argv.first() else {
            return Err(Error::new(Code::Usage, "no tool given"));
        };
        let program = tool_dir.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX));
        if !program.is_file() {
            return Err(
                Error::new(Code::NotFound, format!("tool `{tool}` is not installed")).hint(
                    format!(
                        "looked in {}; set MONOLITH_BIN to the folder that holds the tools",
                        tool_dir.display()
                    ),
                ),
            );
        }
        let root = self.root();
        let envs = [
            ("HOME", root.as_os_str()),
            ("USERPROFILE", root.as_os_str()),
            // Only the Monolith tools are on the search path, so a tool that starts other
            // programs (the krypton shell) cannot reach the host's.
            ("PATH", tool_dir.as_os_str()),
            ("MONOLITH_COMPUTER", OsStr::new(self.name.as_str())),
        ];
        run_program(&program, &argv[1..], &root, &envs, timeout)
    }
}

fn exec_usage() -> Error {
    Error::new(Code::Usage, "exec needs a computer, then `--`, then a tool")
        .hint("monolith exec NAME [--timeout SECS] -- TOOL ARGS...")
}

fn one_name(args: &[String], form: &str) -> Result<String> {
    match args {
        [name] => Ok(name.clone()),
        _ => Err(Error::new(Code::Usage, "expected one computer name").hint(form.to_string())),
    }
}

fn cmd_exec(home: &Path, args: &[String]) -> Result<CmdOut> {
    let name = args.first().ok_or_else(exec_usage)?;
    let mut timeout = 60u64;
    let mut i = 1;
    loop {
        match args.get(i).map(String::as_str) {
            Some("--") => break,
            Some("--timeout") => {
                let v = args.get(i + 1).ok_or_else(exec_usage)?;
                timeout = v.parse().map_err(|_| exec_usage())?;
                i += 2;
            }
            _ => return Err(exec_usage()),
        }
    }
    let argv = &args[i + 1..];
    if argv.is_empty() {
        return Err(exec_usage());
    }
    let c = Computer::open(home, name)?;
    let res = c.exec(argv, Duration::from_secs(timeout))?;
    let argv_json = Json::Arr(argv.iter().cloned().map(Json::Str).collect());
    let ms = Json::Int(i64::try_from(res.ms).unwrap_or(i64::MAX));
    journal::append(
        &c,
        "cli",
        "exec",
        Json::obj([
            ("argv", argv_json),
            ("exit", Json::Int(i64::from(res.exit))),
            ("ms", ms.clone()),
            ("timed_out", Json::Bool(res.timed_out)),
        ]),
    )?;
    let stdout = String::from_utf8_lossy(&res.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&res.stderr).into_owned();
    let text = format!("{stdout}{stderr}");
    let exit = if (0..=255).contains(&res.exit) {
        u8::try_from(res.exit).unwrap_or(1)
    } else {
        1
    };
    let data = Json::obj([
        ("exit", Json::Int(i64::from(res.exit))),
        ("stdout", Json::Str(stdout)),
        ("stderr", Json::Str(stderr)),
        ("timed_out", Json::Bool(res.timed_out)),
        ("duration_ms", ms),
    ]);
    Ok(CmdOut { data, text, exit })
}

fn cmd_log(home: &Path, args: &[String]) -> Result<CmdOut> {
    const FORM: &str = "monolith log NAME [--limit N] [--offset N]";
    let usage = || Error::new(Code::Usage, "log needs a computer name").hint(FORM);
    let name = args.first().ok_or_else(usage)?;
    let mut limit = 50usize;
    let mut offset = 0usize;
    let mut i = 1;
    while i < args.len() {
        let value = args.get(i + 1).ok_or_else(usage)?;
        let n: usize = value.parse().map_err(|_| {
            Error::new(Code::Usage, format!("`{value}` is not a number")).hint(FORM)
        })?;
        match args[i].as_str() {
            "--limit" => limit = n,
            "--offset" => offset = n,
            _ => return Err(usage()),
        }
        i += 2;
    }
    let c = Computer::open(home, name)?;
    let entries = journal::read(&c)?;
    let total = entries.len();
    let shown: Vec<_> = entries.iter().skip(offset).take(limit).collect();
    let mut text = String::new();
    let mut items = Vec::new();
    for e in &shown {
        let mut data = e.data.write();
        if data.chars().count() > 160 {
            data = format!("{}...", data.chars().take(160).collect::<String>());
        }
        text.push_str(&format!("{} {} {} {data}\n", e.seq, e.kind, e.session));
        items.push(Json::obj([
            ("seq", Json::Int(i64::try_from(e.seq).unwrap_or(i64::MAX))),
            (
                "ts_ms",
                Json::Int(i64::try_from(e.ts_ms).unwrap_or(i64::MAX)),
            ),
            ("session", Json::Str(e.session.clone())),
            ("kind", Json::Str(e.kind.clone())),
            ("data", e.data.clone()),
        ]));
    }
    Ok(CmdOut {
        data: envelope::page(items, total, offset, limit),
        text,
        exit: 0,
    })
}

/// CLI: `new NAME`, `list`, `rm NAME`, `exec NAME -- TOOL ARGS...`, `log NAME [--limit N] [--offset N]`.
pub fn cmd(sub: &str, args: &[String]) -> Result<CmdOut> {
    let home = home();
    match sub {
        "new" => {
            let name = one_name(args, "monolith new NAME")?;
            let c = Computer::create(&home, &name)?;
            Ok(CmdOut {
                data: Json::obj([
                    ("name", Json::Str(c.name.clone())),
                    ("dir", Json::path(&c.dir)),
                ]),
                text: format!("created {} at {}", c.name, c.dir.display()),
                exit: 0,
            })
        }
        "list" => {
            if !args.is_empty() {
                return Err(
                    Error::new(Code::Usage, "list takes no arguments").hint("monolith list")
                );
            }
            let names = Computer::list(&home)?;
            let items: Vec<Json> = names
                .iter()
                .map(|n| {
                    Json::obj([
                        ("name", Json::Str(n.clone())),
                        ("dir", Json::path(&computers_dir(&home).join(n))),
                    ])
                })
                .collect();
            let total = items.len();
            let text = if names.is_empty() {
                "no computers".to_string()
            } else {
                names.join("\n")
            };
            Ok(CmdOut {
                data: Json::obj([
                    ("items", Json::Arr(items)),
                    ("total", Json::Int(i64::try_from(total).unwrap_or(i64::MAX))),
                ]),
                text,
                exit: 0,
            })
        }
        "rm" => {
            let name = one_name(args, "monolith rm NAME")?;
            Computer::remove(&home, &name)?;
            Ok(CmdOut {
                data: Json::obj([
                    ("name", Json::Str(name.clone())),
                    ("removed", Json::Bool(true)),
                ]),
                text: format!("removed {name}"),
                exit: 0,
            })
        }
        "exec" => cmd_exec(&home, args),
        "log" => cmd_log(&home, args),
        other => Err(Error::new(
            Code::Usage,
            format!("unknown computer command `{other}`"),
        )),
    }
}
