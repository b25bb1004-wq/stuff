//! External command resolution and process execution.

use std::format;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::prelude::rust_2021::*;
use std::process::{Command, Stdio};
use std::thread;
use std::vec;

use super::redir::{FdTable, FdTarget, IoStreams};
use super::Shell;

fn is_explicit_path(name: &str) -> bool {
    name.contains('/') || (cfg!(windows) && name.contains('\\'))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        (meta.permissions().mode() & 0o111) != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// File names to try for `name`. Windows never runs a bare extensionless file: it appends each
/// `PATHEXT` extension unless the name already ends in one.
fn pathext_candidates(name: &str, pathext: Option<&str>, windows: bool) -> Vec<String> {
    if !windows {
        return vec![name.to_string()];
    }
    let exts: Vec<&str> = pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter(|e| !e.is_empty())
        .collect();
    let lower = name.to_ascii_lowercase();
    let has_ext = exts
        .iter()
        .any(|e| lower.len() > e.len() && lower.ends_with(&e.to_ascii_lowercase()));
    if has_ext {
        return vec![name.to_string()];
    }
    exts.iter().map(|e| format!("{name}{e}")).collect()
}

/// Resolve an external command name using `PATH` (and `PATHEXT` on Windows).
pub(crate) fn find_in_path(
    name: &str,
    path_var: Option<&str>,
    pathext: Option<&str>,
    cwd: &Path,
) -> Result<PathBuf, i32> {
    if name.is_empty() {
        return Err(127);
    }
    if is_explicit_path(name) {
        let p = Path::new(name);
        let resolved = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if let Ok(meta) = std::fs::metadata(&resolved) {
            if meta.is_dir() || !is_executable_file(&resolved) {
                return Err(126);
            }
            return Ok(resolved);
        }
        return Err(127);
    }

    let path_str = path_var.unwrap_or("");
    let sep = if cfg!(windows) { ';' } else { ':' };
    let candidates = pathext_candidates(name, pathext, cfg!(windows));
    let mut saw_non_exec = false;

    for dir in path_str.split(sep) {
        let base = if dir.is_empty() {
            cwd.to_path_buf()
        } else {
            let p = Path::new(dir);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        };
        for cand in &candidates {
            let full = base.join(cand);
            if let Ok(meta) = std::fs::metadata(&full) {
                if meta.is_file() {
                    if is_executable_file(&full) {
                        return Ok(full);
                    }
                    saw_non_exec = true;
                }
            }
        }
    }

    if saw_non_exec {
        Err(126)
    } else {
        Err(127)
    }
}

fn stdio_for_fd(target: Option<&FdTarget>, inherit: bool) -> Stdio {
    match target {
        Some(FdTarget::File(f)) => f
            .try_clone()
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null()),
        Some(FdTarget::Closed) | None => Stdio::null(),
        Some(FdTarget::ParentStdin | FdTarget::ParentStdout | FdTarget::ParentStderr)
            if inherit =>
        {
            Stdio::inherit()
        }
        _ => Stdio::piped(),
    }
}

/// Quote one argument for a Windows command line so CommandLineToArgvW reads it back unchanged.
#[cfg(any(windows, test))]
pub(crate) fn win_quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

fn is_batch_file(path: &Path) -> bool {
    cfg!(windows)
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

/// Build the command for `exe`. A `.cmd`/`.bat` file runs through `cmd /d /s /c` on Windows.
fn command_for(exe: &Path, args: &[String]) -> Result<Command, String> {
    if is_batch_file(exe) {
        return batch_command(exe, args);
    }
    let mut cmd = Command::new(exe);
    cmd.args(args);
    Ok(cmd)
}

#[cfg(windows)]
fn batch_command(exe: &Path, args: &[String]) -> Result<Command, String> {
    use std::os::windows::process::CommandExt;
    // cmd.exe cannot quote % or " safely, and ! expands only with delayed expansion: refuse them.
    if let Some(bad) = args
        .iter()
        .find(|a| a.contains(['%', '"', '!', '\r', '\n', '\0']))
    {
        return Err(format!("argument not safe for a batch file: {bad:?}"));
    }
    let mut line = win_quote_arg(&exe.to_string_lossy());
    for arg in args {
        line.push(' ');
        line.push_str(&win_quote_arg(arg));
    }
    let comspec = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut cmd = Command::new(comspec);
    // /s keeps the quoting of the whole line, so only the outer pair of quotes is stripped.
    cmd.raw_arg(format!("/d /s /c \"{line}\""));
    Ok(cmd)
}

#[cfg(not(windows))]
fn batch_command(_exe: &Path, _args: &[String]) -> Result<Command, String> {
    Err("batch files only run on Windows".into())
}

fn copy_stream(reader: &mut dyn Read, writer: &mut dyn Write) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Some(slice) = buf.get(..n) {
                    if writer.write_all(slice).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
            }
        }
    }
}

fn spawn_out_thread(
    mut reader: impl Read + Send + 'static,
    target: Option<FdTarget>,
) -> Option<thread::JoinHandle<Vec<u8>>> {
    match target {
        Some(FdTarget::BufWrite(bw)) => Some(thread::spawn(move || {
            let mut local = Vec::new();
            copy_stream(&mut reader, &mut local);
            if let Ok(mut guard) = bw.lock() {
                guard.extend_from_slice(&local);
            }
            Vec::new()
        })),
        Some(FdTarget::ParentStdout) | Some(FdTarget::ParentStderr) => {
            Some(thread::spawn(move || {
                let mut local = Vec::new();
                copy_stream(&mut reader, &mut local);
                local
            }))
        }
        _ => None,
    }
}

/// Execute an external program, returning its exit status (127/126/128+sig on failure).
pub(crate) fn run_external(
    program: &str,
    args: &[String],
    extra_env: &[(String, String)],
    shell: &Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let path_var = shell.vars.get("PATH").map(String::as_str);
    let pathext = shell.vars.get("PATHEXT").map(String::as_str);
    let exe_path = match find_in_path(program, path_var, pathext, &shell.cwd) {
        Ok(p) => p,
        Err(code) => {
            if code == 127 {
                fds.write_err(io, &format!("krypton: {program}: command not found\n"));
            } else {
                fds.write_err(io, &format!("krypton: {program}: Permission denied\n"));
            }
            return code;
        }
    };

    let fd0 = fds.get(0).cloned();
    let fd1 = fds.get(1).cloned();
    let fd2 = fds.get(2).cloned();

    let inherit = shell.inherit_stdio;
    let mut cmd = match command_for(&exe_path, args) {
        Ok(c) => c,
        Err(msg) => {
            fds.write_err(io, &format!("krypton: {program}: {msg}\n"));
            return 126;
        }
    };
    cmd.current_dir(&shell.cwd);
    cmd.env_clear();
    for name in &shell.exported {
        if let Some(val) = shell.vars.get(name) {
            cmd.env(name, val);
        }
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    cmd.stdin(stdio_for_fd(fd0.as_ref(), inherit));
    cmd.stdout(stdio_for_fd(fd1.as_ref(), inherit));
    cmd.stderr(stdio_for_fd(fd2.as_ref(), inherit));

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            fds.write_err(io, &format!("krypton: {program}: {e}\n"));
            return if e.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            };
        }
    };

    let out_handle = child
        .stdout
        .take()
        .and_then(|s| spawn_out_thread(s, fd1.clone()));
    let err_handle = child
        .stderr
        .take()
        .and_then(|s| spawn_out_thread(s, fd2.clone()));

    let in_handle = if let Some(mut cstdin) = child.stdin.take() {
        match fd0 {
            Some(FdTarget::MemRead(cur)) => Some(thread::spawn(move || {
                if let Ok(mut guard) = cur.lock() {
                    copy_stream(&mut *guard, &mut cstdin);
                }
            })),
            Some(FdTarget::ParentStdin) => {
                copy_stream(io.stdin, &mut cstdin);
                drop(cstdin);
                None
            }
            _ => None,
        }
    } else {
        None
    };

    let status = child.wait();
    if let Some(h) = in_handle {
        let _ = h.join();
    }
    if let Some(h) = out_handle {
        if let Ok(data) = h.join() {
            if !data.is_empty() {
                if matches!(fd1, Some(FdTarget::ParentStdout)) {
                    let _ = io.stdout.write_all(&data);
                    let _ = io.stdout.flush();
                } else if matches!(fd1, Some(FdTarget::ParentStderr)) {
                    let _ = io.stderr.write_all(&data);
                    let _ = io.stderr.flush();
                }
            }
        }
    }
    if let Some(h) = err_handle {
        if let Ok(data) = h.join() {
            if !data.is_empty() {
                if matches!(fd2, Some(FdTarget::ParentStdout)) {
                    let _ = io.stdout.write_all(&data);
                    let _ = io.stdout.flush();
                } else if matches!(fd2, Some(FdTarget::ParentStderr)) {
                    let _ = io.stderr.write_all(&data);
                    let _ = io.stderr.flush();
                }
            }
        }
    }

    match status {
        Ok(st) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                st.code()
                    .unwrap_or_else(|| st.signal().map(|s| 128 + s).unwrap_or(1))
            }
            #[cfg(not(unix))]
            {
                st.code().unwrap_or(1)
            }
        }
        Err(_) => 1,
    }
}

/// Format an external path for `type` builtin if found.
pub(crate) fn which(name: &str, shell: &Shell) -> Option<String> {
    let path_var = shell.vars.get("PATH").map(String::as_str);
    let pathext = shell.vars.get("PATHEXT").map(String::as_str);
    find_in_path(name, path_var, pathext, &shell.cwd)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{find_in_path, pathext_candidates, win_quote_arg};
    use std::format;
    use std::path::Path;
    use std::prelude::rust_2021::*;
    use std::string::String;
    use std::vec;
    use std::vec::Vec;

    /// Split a command line by the documented CommandLineToArgvW rules (MS C runtime).
    fn argv_split(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            if i >= chars.len() {
                return out;
            }
            let mut cur = String::new();
            let mut in_quotes = false;
            while i < chars.len() {
                let c = chars[i];
                if c == '\\' {
                    let mut n = 0;
                    while i < chars.len() && chars[i] == '\\' {
                        n += 1;
                        i += 1;
                    }
                    if i < chars.len() && chars[i] == '"' {
                        cur.extend(std::iter::repeat_n('\\', n / 2));
                        if n % 2 == 1 {
                            cur.push('"');
                        } else {
                            in_quotes = !in_quotes;
                        }
                        i += 1;
                    } else {
                        cur.extend(std::iter::repeat_n('\\', n));
                    }
                } else if c == '"' {
                    in_quotes = !in_quotes;
                    i += 1;
                } else if (c == ' ' || c == '\t') && !in_quotes {
                    break;
                } else {
                    cur.push(c);
                    i += 1;
                }
            }
            out.push(cur);
        }
    }

    #[test]
    fn quoting_matches_documented_examples() {
        assert_eq!(win_quote_arg("plain"), "plain");
        assert_eq!(win_quote_arg(""), "\"\"");
        assert_eq!(win_quote_arg("a b"), "\"a b\"");
        assert_eq!(win_quote_arg("a\"b"), "\"a\\\"b\"");
        assert_eq!(win_quote_arg("a b\\"), "\"a b\\\\\"");
        assert_eq!(win_quote_arg("C:\\dir\\"), "C:\\dir\\");
    }

    #[test]
    fn quoted_arguments_round_trip_through_argv_rules() {
        let cases = [
            "",
            "plain",
            "two words",
            "a\"b",
            "trailing\\",
            "dir with space\\",
            "back\\\\\"slash",
            "q\"",
            "\"",
            "\\\"",
            "tab\tin",
            "x\\\\",
            "a b\\\\\\\"c d",
            "\\",
            " lead",
            "end ",
            "C:\\Program Files\\App\\",
            "%PATH% & | < > ^",
        ];
        for case in cases {
            let line = win_quote_arg(case);
            assert_eq!(
                argv_split(&line),
                vec![case.to_string()],
                "{case:?} as {line}"
            );
        }
        let args = ["one", "two words", "q\"x", "end\\"];
        let line: Vec<String> = args.iter().map(|a| win_quote_arg(a)).collect();
        let back = argv_split(&line.join(" "));
        assert_eq!(back, args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn windows_candidates_use_pathext_and_never_a_bare_name() {
        let exts = Some(".EXE;.CMD");
        assert_eq!(
            pathext_candidates("foo", exts, true),
            ["foo.EXE", "foo.CMD"]
        );
        assert_eq!(pathext_candidates("foo.cmd", exts, true), ["foo.cmd"]);
        assert_eq!(pathext_candidates("FOO.exe", exts, true), ["FOO.exe"]);
        assert_eq!(pathext_candidates("foo", None, true).len(), 4);
        assert_eq!(pathext_candidates("foo", exts, false), ["foo"]);
    }

    #[test]
    fn path_lookup_finds_real_files_in_a_temp_dir() {
        let dir = std::env::temp_dir().join(format!("krypton-path-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let tool = if cfg!(windows) { "tool.cmd" } else { "tool" };
        let plain = if cfg!(windows) { "bare" } else { "notexec" };
        let _ = std::fs::write(dir.join(tool), "@echo off\r\n");
        let _ = std::fs::write(dir.join(plain), "data");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(dir.join(tool), std::fs::Permissions::from_mode(0o755));
        }
        let path = dir.to_string_lossy().into_owned();
        let found = find_in_path("tool", Some(&path), Some(".CMD"), Path::new("."));
        let missing = find_in_path(plain, Some(&path), Some(".CMD"), Path::new("."));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(found.is_ok(), "tool should resolve: {found:?}");
        assert_eq!(missing.err(), Some(if cfg!(windows) { 127 } else { 126 }));
    }
}
