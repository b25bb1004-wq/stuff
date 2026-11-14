//! Pipeline concurrency and background job management.

use std::format;
use std::io;
use std::prelude::rust_2021::*;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::ast::{AndOr, Pipeline};

use super::pipe::mem_pipe;
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
        let mut readers = Vec::with_capacity(n - 1);
        let mut writers = Vec::with_capacity(n - 1);
        for _ in 0..(n - 1) {
            let (r, w) = mem_pipe(65536);
            readers.push(Arc::new(Mutex::new(r)));
            writers.push(Arc::new(Mutex::new(w)));
        }

        let last_out_buf = Arc::new(Mutex::new(Vec::new()));
        let mut err_bufs = Vec::with_capacity(n - 1);
        let mut handles = Vec::with_capacity(n - 1);

        for idx in 1..n {
            let cmd = pipe.cmds[idx].clone();
            let mut sub = shell.clone_subshell();
            if pipe.bang {
                sub.suppress_errexit = true;
            }
            let err_buf = Arc::new(Mutex::new(Vec::new()));
            let mut stage_fds = sanitize_fds_for_thread(fds, &err_buf);
            stage_fds.insert(0, FdTarget::PipeRead(Arc::clone(&readers[idx - 1])));
            if idx + 1 < n {
                stage_fds.insert(1, FdTarget::PipeWrite(Arc::clone(&writers[idx])));
            } else if matches!(stage_fds.get(1), Some(FdTarget::ParentStdout) | None) {
                stage_fds.insert(1, FdTarget::BufWrite(Arc::clone(&last_out_buf)));
            } else if matches!(stage_fds.get(1), Some(FdTarget::ParentStderr)) {
                stage_fds.insert(1, FdTarget::BufWrite(Arc::clone(&err_buf)));
            }
            err_bufs.push(err_buf);

            let h = thread::spawn(move || {
                let mut empty_in = io::empty();
                let mut sink_out = io::sink();
                let mut sink_err = io::sink();
                let mut stage_io = IoStreams {
                    stdin: &mut empty_in,
                    stdout: &mut sink_out,
                    stderr: &mut sink_err,
                };
                sub.exec_command(&cmd, &mut stage_fds, &mut stage_io)
            });
            handles.push(h);
        }

        // Drop unused pipe ends in the coordinator so EOF / BrokenPipe propagate.
        drop(readers);
        let first_writer = writers.remove(0);
        drop(writers);

        let mut stage0 = shell.clone_subshell();
        if pipe.bang {
            stage0.suppress_errexit = true;
        }
        let mut stage0_fds = fds.clone();
        stage0_fds.insert(1, FdTarget::PipeWrite(first_writer));
        let _ = stage0.exec_command(&pipe.cmds[0], &mut stage0_fds, io);
        drop(stage0_fds);

        let mut last_st = 0;
        for (i, h) in handles.into_iter().enumerate() {
            let st = h.join().unwrap_or(1);
            if i + 2 == n {
                last_st = st;
            }
        }

        if let Ok(out_data) = last_out_buf.lock() {
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

        last_st
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
            fds.write_err(io, &format!("crust: wait: {arg}: no such job\n"));
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
            fds.write_err(io, &format!("crust: wait: {arg}: no such job\n"));
            last_status = 127;
        }
    }
    last_status
}
