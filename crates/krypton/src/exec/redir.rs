//! File descriptor table and POSIX redirections.

use std::collections::BTreeMap;
use std::format;
use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::PathBuf;
use std::prelude::rust_2021::*;
use std::sync::{Arc, Mutex};

use crate::ast::{Part, Redir, RedirOp};
use crate::expand::expand_one;
use crate::types::{Env, ExpandError};

use super::{ControlFlow, Shell};

/// Borrowed standard streams passed into [`Shell::run`].
pub(crate) struct IoStreams<'a> {
    pub(crate) stdin: &'a mut dyn Read,
    pub(crate) stdout: &'a mut dyn Write,
    pub(crate) stderr: &'a mut dyn Write,
}

/// Destination or source backing a shell file descriptor.
#[derive(Clone)]
pub(crate) enum FdTarget {
    ParentStdin,
    ParentStdout,
    ParentStderr,
    File(Arc<File>),
    MemRead(Arc<Mutex<Cursor<Vec<u8>>>>),
    BufWrite(Arc<Mutex<Vec<u8>>>),
    Closed,
}

/// Per-command file descriptor table (0 = stdin, 1 = stdout, 2 = stderr, ...).
#[derive(Clone)]
pub(crate) struct FdTable {
    fds: BTreeMap<u32, FdTarget>,
}

impl FdTable {
    /// Create a default table mapping 0/1/2 to the caller's streams.
    pub(crate) fn new_default() -> Self {
        let mut fds = BTreeMap::new();
        fds.insert(0, FdTarget::ParentStdin);
        fds.insert(1, FdTarget::ParentStdout);
        fds.insert(2, FdTarget::ParentStderr);
        Self { fds }
    }

    /// Look up a file descriptor entry.
    pub(crate) fn get(&self, fd: u32) -> Option<&FdTarget> {
        self.fds.get(&fd)
    }

    /// Insert or replace a file descriptor entry.
    pub(crate) fn insert(&mut self, fd: u32, target: FdTarget) {
        self.fds.insert(fd, target);
    }

    /// Resolve `path` relative to `cwd`.
    fn resolve_path(cwd: &std::path::Path, target: &str) -> PathBuf {
        if cfg!(windows) && target == "/dev/null" {
            return PathBuf::from("NUL");
        }
        let p = std::path::Path::new(target);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    }

    /// Apply a list of redirections left-to-right.
    pub(crate) fn apply_redirs(
        &mut self,
        redirs: &[Redir],
        shell: &mut Shell,
    ) -> Result<(), String> {
        for redir in redirs {
            self.apply_one(redir, shell)?;
        }
        Ok(())
    }

    fn apply_one(&mut self, redir: &Redir, shell: &mut Shell) -> Result<(), String> {
        if let RedirOp::HereDoc {
            strip_tabs: _,
            quoted,
            body,
        } = &redir.op
        {
            let fd = redir.fd.unwrap_or(0);
            let text = if *quoted {
                let mut s = String::new();
                for part in body {
                    if let Part::Lit(lit) = part {
                        s.push_str(lit);
                    }
                }
                s
            } else {
                expand_one(body, shell).map_err(|e| {
                    if let ExpandError::Unset(name, msg) = &e {
                        shell.exited = Some(1);
                        shell.flow = ControlFlow::Exit(1);
                        return format!("{name}: {msg}");
                    }
                    format!("{e:?}")
                })?
            };
            self.fds.insert(
                fd,
                FdTarget::MemRead(Arc::new(Mutex::new(Cursor::new(text.into_bytes())))),
            );
            return Ok(());
        }

        let target_str = expand_one(&redir.target, shell).map_err(|e| {
            if let ExpandError::Unset(name, msg) = &e {
                shell.exited = Some(1);
                shell.flow = ControlFlow::Exit(1);
                return format!("{name}: {msg}");
            }
            format!("{e:?}")
        })?;
        let default_fd = match redir.op {
            RedirOp::In | RedirOp::ReadWrite | RedirOp::DupIn | RedirOp::HereDoc { .. } => 0,
            RedirOp::Out | RedirOp::Append | RedirOp::Clobber | RedirOp::DupOut => 1,
        };
        let fd = redir.fd.unwrap_or(default_fd);

        match redir.op {
            RedirOp::In => {
                let path = Self::resolve_path(&shell.cwd, &target_str);
                let file = File::open(&path).map_err(|e| format!("{target_str}: {e}"))?;
                self.fds.insert(fd, FdTarget::File(Arc::new(file)));
            }
            RedirOp::Out => {
                let path = Self::resolve_path(&shell.cwd, &target_str);
                let mut opts = OpenOptions::new();
                opts.write(true);
                if shell.opts.noclobber {
                    if path.exists() && path.is_file() {
                        return Err(format!("{target_str}: cannot overwrite existing file"));
                    }
                    opts.create(true).truncate(true);
                } else {
                    opts.create(true).truncate(true);
                }
                let file = opts.open(&path).map_err(|e| format!("{target_str}: {e}"))?;
                self.fds.insert(fd, FdTarget::File(Arc::new(file)));
            }
            RedirOp::Clobber => {
                let path = Self::resolve_path(&shell.cwd, &target_str);
                let file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&path)
                    .map_err(|e| format!("{target_str}: {e}"))?;
                self.fds.insert(fd, FdTarget::File(Arc::new(file)));
            }
            RedirOp::Append => {
                let path = Self::resolve_path(&shell.cwd, &target_str);
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|e| format!("{target_str}: {e}"))?;
                self.fds.insert(fd, FdTarget::File(Arc::new(file)));
            }
            RedirOp::ReadWrite => {
                let path = Self::resolve_path(&shell.cwd, &target_str);
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&path)
                    .map_err(|e| format!("{target_str}: {e}"))?;
                self.fds.insert(fd, FdTarget::File(Arc::new(file)));
            }
            RedirOp::DupIn | RedirOp::DupOut => {
                if target_str == "-" {
                    self.fds.insert(fd, FdTarget::Closed);
                } else {
                    let src_fd: u32 = target_str
                        .parse()
                        .map_err(|_| format!("{target_str}: ambiguous redirect"))?;
                    let cloned = match self.fds.get(&src_fd) {
                        Some(FdTarget::Closed) | None => {
                            return Err(format!("{src_fd}: Bad file descriptor"));
                        }
                        Some(other) => other.clone(),
                    };
                    self.fds.insert(fd, cloned);
                }
            }
            RedirOp::HereDoc { .. } => {}
        }
        let _ = shell.get("?");
        Ok(())
    }

    /// Read bytes from `fd` into `buf`.
    pub(crate) fn read_fd(
        &mut self,
        fd: u32,
        io: &mut IoStreams<'_>,
        buf: &mut [u8],
    ) -> io::Result<usize> {
        let Some(target) = self.fds.get(&fd) else {
            return Err(io::Error::other("bad file descriptor"));
        };
        match target {
            FdTarget::ParentStdin => io.stdin.read(buf),
            FdTarget::ParentStdout | FdTarget::ParentStderr | FdTarget::BufWrite(_) => {
                Err(io::Error::other("file descriptor not open for reading"))
            }
            FdTarget::File(file) => {
                let mut f: &File = file;
                f.read(buf)
            }
            FdTarget::MemRead(cur) => {
                let mut guard = cur.lock().map_err(|_| io::Error::other("mutex poisoned"))?;
                guard.read(buf)
            }
            FdTarget::Closed => Ok(0),
        }
    }

    /// Write all bytes of `data` to `fd`.
    pub(crate) fn write_fd(
        &mut self,
        fd: u32,
        io: &mut IoStreams<'_>,
        data: &[u8],
    ) -> io::Result<()> {
        let Some(target) = self.fds.get(&fd) else {
            return Err(io::Error::other("bad file descriptor"));
        };
        match target {
            FdTarget::ParentStdout => {
                io.stdout.write_all(data)?;
                io.stdout.flush()
            }
            FdTarget::ParentStderr => {
                io.stderr.write_all(data)?;
                io.stderr.flush()
            }
            FdTarget::ParentStdin | FdTarget::MemRead(_) => {
                Err(io::Error::other("file descriptor not open for writing"))
            }
            FdTarget::File(file) => {
                let mut f: &File = file;
                f.write_all(data)
            }
            FdTarget::BufWrite(buf) => {
                let mut guard = buf.lock().map_err(|_| io::Error::other("mutex poisoned"))?;
                guard.extend_from_slice(data);
                Ok(())
            }
            FdTarget::Closed => Err(io::Error::other("bad file descriptor")),
        }
    }

    /// Write a string message to fd 1 (ignoring broken pipe errors if desired or propagating).
    pub(crate) fn write_out(&mut self, io: &mut IoStreams<'_>, s: &str) {
        let _ = self.write_fd(1, io, s.as_bytes());
    }

    /// Write a string message to fd 2.
    pub(crate) fn write_err(&mut self, io: &mut IoStreams<'_>, s: &str) {
        let _ = self.write_fd(2, io, s.as_bytes());
    }
}
