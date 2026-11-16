//! Pipeline concurrency and background job management.

use std::format;
use std::fs::File;
use std::io::{self};
use std::prelude::rust_2021::*;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::ast::{AndOr, Pipeline};

use super::redir::{FdTable, FdTarget, IoStreams};
use super::{ControlFlow, Shell};

/// Tracked background job.
pub(crate) struct Job {
    pub(crate) id: usize,
    pub(crate) pid: u32,
    pub(crate) desc: String,
    pub(crate) handle: Option<JoinHandle<i32>>,
    pub(crate) status: Option<i32>,
    pub(crate) out_buf: Arc<Mutex<Vec<u8>>>,
    pub(crate) err_buf: Arc<Mutex<Vec<u8>>>,
}

impl Job {
    fn poll_done(&mut self) -> Option<i32> {
        if let Some(st) = self.status {
            return Some(st);
        }
        let finished = self.handle.as_ref().is_some_and(JoinHandle::is_finished);
        if finished {
            if let Some(h) = self.handle.take() {
                let st = h.join().unwrap_or(1);
                self.status = Some(st);
                return Some(st);
            }
        }
        None
    }

    fn wait_finish(&mut self, io: &mut IoStreams<'_>) -> i32 {
        let st = if let Some(st) = self.status {
            st
        } else if let Some(h) = self.handle.take() {
            let s = h.join().unwrap_or(1);
            self.status = Some(s);
            s
        } else {
            0
        };
        if let Ok(mut buf) = self.out_buf.lock() {
            if !buf.is_empty() {
                let _ = io.stdout.write_all(&buf);
                let _ = io.stdout.flush();
                buf.clear();
            }
        }
        if let Ok(mut buf) = self.err_buf.lock() {
            if !buf.is_empty() {
                let _ = io.stderr.write_all(&buf);
                let _ = io.stderr.flush();
                buf.clear();
            }
        }
        st
    }
}

fn sanitize_fds_for_thread(base: &FdTable, err_buf: &Arc<Mutex<Vec<u8>>>) -> FdTable {
    let mut out = base.clone();
    if matches!(out.get(0), Some(FdTarget::ParentStdin) | None) {
        out.insert(0, FdTarget::Closed);
    }
    if matches!(
        out.get(2),
        Some(FdTarget::ParentStderr) | Some(FdTarget::ParentStdout) | None
    ) {
        out.insert(2, FdTarget::BufWrite(Arc::clone(err_buf)));
    }
    out
}

/// OS pipe end as a `File`, so a shell file descriptor can hold it.
#[cfg(unix)]
fn pipe_end(fd: impl Into<std::os::fd::OwnedFd>) -> File {
    File::from(fd.into())
}

/// OS pipe end as a `File`, so a shell file descriptor can hold it.
#[cfg(windows)]
fn pipe_end(fd: impl Into<std::os::windows::io::OwnedHandle>) -> File {
    File::from(fd.into())
}

/// Run a multi-command pipeline: stages 1.. run on threads, stage 0 runs on this thread with the
/// caller's streams. Stages are joined by real OS pipes. Interactive shells let every stage
/// inherit the process streams; otherwise the last stage's output is buffered and written back
/// to the caller once all stages finish.
fn run_stages(
    shell: &mut Shell,
    pipe: &Pipeline,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> io::Result<Vec<i32>> {
    let n = pipe.cmds.len();
    let inherit = shell.inherit_stdio;
    let mut pipes = Vec::with_capacity(n - 1);
    for _ in 1..n {
        let (r, w) = io::pipe()?;
        pipes.push((Arc::new(pipe_end(r)), Arc::new(pipe_end(w))));
    }
    let last_out = Arc::new(Mutex::new(Vec::new()));
    let mut err_bufs = Vec::with_capacity(n);
    let mut handles = Vec::with_capacity(n);

    for idx in 1..n {
        let mut stage_fds = fds.clone();
        stage_fds.insert(0, FdTarget::File(Arc::clone(&pipes[idx - 1].0)));
        if idx + 1 < n {
            stage_fds.insert(1, FdTarget::File(Arc::clone(&pipes[idx].1)));
        } else if !inherit && matches!(fds.get(1), Some(FdTarget::ParentStdout) | None) {
            stage_fds.insert(1, FdTarget::BufWrite(Arc::clone(&last_out)));
        }
        let err_buf = Arc::new(Mutex::new(Vec::new()));
        if !inherit {
            stage_fds = sanitize_fds_for_thread(&stage_fds, &err_buf);
        }
        err_bufs.push(err_buf);

        let mut sub = shell.clone_subshell();
        sub.suppress_errexit = sub.suppress_errexit || pipe.bang;
        let cmd = pipe.cmds[idx].clone();
        handles.push(thread::spawn(move || {
            if inherit {
                let (mut si, mut so, mut se) = (io::stdin(), io::stdout(), io::stderr());
                let mut stage_io = IoStreams {
                    stdin: &mut si,
                    stdout: &mut so,
                    stderr: &mut se,
                };
                sub.exec_command(&cmd, &mut stage_fds, &mut stage_io)
            } else {
                let (mut empty_in, mut sink_out, mut sink_err) =
                    (io::empty(), io::sink(), io::sink());
                let mut stage_io = IoStreams {
                    stdin: &mut empty_in,
                    stdout: &mut sink_out,
                    stderr: &mut sink_err,
                };
                sub.exec_command(&cmd, &mut stage_fds, &mut stage_io)
            }
        }));
    }

    // Stage 0 writes into the first pipe on this thread; its end is dropped when it finishes.
    let mut stage0 = shell.clone_subshell();
    stage0.suppress_errexit = stage0.suppress_errexit || pipe.bang;
    let mut stage0_fds = fds.clone();
    stage0_fds.insert(1, FdTarget::File(Arc::clone(&pipes[0].1)));
    let mut statuses = vec![stage0.exec_command(&pipe.cmds[0], &mut stage0_fds, io)];
    drop(stage0_fds);
    // The coordinator keeps no pipe ends, so EOF and broken pipes reach the stages.
    drop(pipes);

    for h in handles {
        statuses.push(h.join().unwrap_or(1));
    }
    if let Ok(out_data) = last_out.lock() {
        if !out_data.is_empty() {
            let _ = fds.write_fd(1, io, &out_data);
        }
    }
    for eb in err_bufs {
        if let Ok(err_data) = eb.lock() {
            if !err_data.is_empty() {
                let _ = fds.write_fd(2, io, &err_data);
            }
        }
    }
    Ok(statuses)
}

/// Pipeline status from every stage's status: the last one, or with `pipefail` the last non-zero.
fn pipeline_status(statuses: &[i32], pipefail: bool) -> i32 {
    if pipefail {
        statuses
            .iter()
            .rev()
            .find(|&&s| s != 0)
            .copied()
            .unwrap_or(0)
    } else {
        statuses.last().copied().unwrap_or(0)
    }
}

/// Execute a pipeline (`[!] cmd1 | cmd2 ...`), updating `shell.last_status`.
pub(crate) fn exec_pipeline(
    shell: &mut Shell,
    pipe: &Pipeline,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    let n = pipe.cmds.len();
    let raw_status = if n == 0 {
        0
    } else if n == 1 {
        let cmd = &pipe.cmds[0];
        let prev_suppress = shell.suppress_errexit;
        shell.suppress_errexit = prev_suppress || pipe.bang;
        let st = shell.exec_command(cmd, fds, io);
        shell.suppress_errexit = prev_suppress;
        st
    } else {
        match run_stages(shell, pipe, fds, io) {
            Ok(statuses) => pipeline_status(&statuses, shell.opts.pipefail),
            Err(e) => {
                fds.write_err(io, &format!("krypton: pipe: {e}\n"));
                1
            }
        }
    };

    let final_status = if pipe.bang {
        if raw_status == 0 {
            1
        } else {
            0
        }
    } else {
        raw_status
    };
    shell.last_status = final_status;
    if n > 1
        && !shell.suppress_errexit
        && !pipe.bang
        && shell.opts.errexit
        && final_status != 0
        && shell.flow == ControlFlow::None
    {
        shell.exited = Some(final_status);
        shell.flow = ControlFlow::Exit(final_status);
    }
    final_status
}

/// Spawn an `AndOr` list in the background (`&`).
pub(crate) fn spawn_background(shell: &mut Shell, and_or: &AndOr, fds: &FdTable) -> i32 {
    let mut bg_and_or = and_or.clone();
    bg_and_or.background = false;

    let mut sub = shell.clone_subshell();
    let out_buf = Arc::new(Mutex::new(Vec::new()));
    let err_buf = Arc::new(Mutex::new(Vec::new()));
    let mut bg_fds = sanitize_fds_for_thread(fds, &err_buf);
    if matches!(bg_fds.get(1), Some(FdTarget::ParentStdout) | None) {
        bg_fds.insert(1, FdTarget::BufWrite(Arc::clone(&out_buf)));
    }

    let job_id = shell.next_job_id;
    shell.next_job_id += 1;
    let job_pid = shell.pid.saturating_add(job_id as u32);

    let handle = thread::spawn(move || {
        let mut empty_in = io::empty();
        let mut sink_out = io::sink();
        let mut sink_err = io::sink();
        let mut bg_io = IoStreams {
            stdin: &mut empty_in,
            stdout: &mut sink_out,
            stderr: &mut sink_err,
        };
        sub.suppress_errexit = false;
        sub.exec_and_or(&bg_and_or, &mut bg_fds, &mut bg_io)
    });

    shell.jobs.push(Job {
        id: job_id,
        pid: job_pid,
        desc: format!("job {job_id}"),
        handle: Some(handle),
        status: None,
        out_buf,
        err_buf,
    });
    shell.last_bg_pid = Some(job_pid);
    shell.last_status = 0;
    0
}

/// `jobs` builtin.
pub(crate) fn builtin_jobs(shell: &mut Shell, fds: &mut FdTable, io: &mut IoStreams<'_>) -> i32 {
    for job in &mut shell.jobs {
        let state = if job.poll_done().is_some() {
            "Done"
        } else {
            "Running"
        };
        fds.write_out(io, &format!("[{}] {}  {}\n", job.id, state, job.desc));
    }
    0
}

/// `wait [n ...]` builtin.
pub(crate) fn builtin_wait(
    args: &[String],
    shell: &mut Shell,
    fds: &mut FdTable,
    io: &mut IoStreams<'_>,
) -> i32 {
    if args.is_empty() {
        for job in &mut shell.jobs {
            job.wait_finish(io);
        }
        shell.jobs.clear();
        return 0;
    }

    let mut last_status = 0;
    for arg in args {
        let trimmed = arg.strip_prefix('%').unwrap_or(arg.as_str());
        let Ok(num) = trimmed.parse::<usize>() else {
            fds.write_err(io, &format!("krypton: wait: {arg}: no such job\n"));
            last_status = 127;
            continue;
        };
        let found = shell
            .jobs
            .iter_mut()
            .find(|j| j.id == num || j.pid as usize == num);
        if let Some(job) = found {
            last_status = job.wait_finish(io);
        } else {
            fds.write_err(io, &format!("krypton: wait: {arg}: no such job\n"));
            last_status = 127;
        }
    }
    last_status
}
