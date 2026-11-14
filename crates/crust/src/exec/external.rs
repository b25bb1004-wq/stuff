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

fn pathext_candidates(name: &str, pathext: Option<&str>) -> Vec<String> {
    let mut out = vec![name.to_string()];
    if cfg!(windows) {
        let exts = pathext.unwrap_or(".COM;.EXE;.BAT;.CMD");
        for ext in exts.split(';') {
            if !ext.is_empty() {
                out.push(format!("{name}{ext}"));
            }
        }
    }
    out
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
    let candidates = pathext_candidates(name, pathext);
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

fn stdio_for_fd(target: Option<&FdTarget>) -> Stdio {
    match target {
        Some(FdTarget::File(f)) => f
            .try_clone()
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null()),
        Some(FdTarget::Closed) | None => Stdio::null(),
        _ => Stdio::piped(),
    }
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
        Some(FdTarget::PipeWrite(pw)) => Some(thread::spawn(move || {
            if let Ok(mut guard) = pw.lock() {
                copy_stream(&mut reader, &mut *guard);
            }
            Vec::new()
        })),
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
                fds.write_err(io, &format!("crust: {program}: command not found\n"));
            } else {
                fds.write_err(io, &format!("crust: {program}: Permission denied\n"));
            }
            return code;
        }
    };

    let fd0 = fds.get(0).cloned();
    let fd1 = fds.get(1).cloned();
    let fd2 = fds.get(2).cloned();

    let mut cmd = Command::new(&exe_path);
    cmd.args(args);
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

    cmd.stdin(stdio_for_fd(fd0.as_ref()));
    cmd.stdout(stdio_for_fd(fd1.as_ref()));
    cmd.stderr(stdio_for_fd(fd2.as_ref()));

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            fds.write_err(io, &format!("crust: {program}: {e}\n"));
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
            Some(FdTarget::PipeRead(pr)) => Some(thread::spawn(move || {
                if let Ok(mut guard) = pr.lock() {
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
