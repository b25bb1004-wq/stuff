//! U-mode syscall dispatcher matching `docs/TITAN.md` numbers (`0..=15`) and `Errno` codes.

use alloc::vec::Vec;
use core::fmt::Write;
use core::ptr::read_volatile;
use core::sync::atomic::Ordering;

use titan::frames::PAGE_SIZE;
use titan::proc::{Pid, WaitStatus};
use titan::uaccess::Errno;

use crate::proc::KernelState;
use crate::progs;
use crate::vfs::{self, OpenFd};
use crate::vm;
use crate::{TrapFrame, Uart, TICKS};

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 2;
const SYS_OPEN: u64 = 3;
const SYS_CLOSE: u64 = 4;
const SYS_SPAWN: u64 = 5;
const SYS_WAIT: u64 = 6;
const SYS_KILL: u64 = 7;
const SYS_GETPID: u64 = 8;
const SYS_YIELD: u64 = 9;
const SYS_SBRK: u64 = 10;
const SYS_MKDIR: u64 = 11;
const SYS_UNLINK: u64 = 12;
const SYS_READDIR: u64 = 13;
const SYS_PS: u64 = 14;
const SYS_UPTIME: u64 = 15;

pub fn handle_syscall(ks: &mut KernelState, tf: &mut TrapFrame) -> *mut TrapFrame {
    let pid = match ks.current {
        Some(p) => p,
        None => crate::fail("syscall without current proc"),
    };
    let pt_root = ks.procs[&pid].pt_root;
    let num = tf.regs[17]; // a7
    let a0 = tf.regs[10];
    let a1 = tf.regs[11];
    let a2 = tf.regs[12];
    let a3 = tf.regs[13];
    let a4 = tf.regs[14];
    let a5 = tf.regs[15];

    match num {
        SYS_EXIT => {
            let code = a0 as i32;
            ks.exit_current(code)
        }
        SYS_WRITE => {
            let res = if a0 == 1 && ks.procs.get(&pid).is_some_and(|p| p.fds.has_stdout()) {
                vfs::sys_write_fd(ks, pid, pt_root, a0, a1, a2)
            } else if a0 == 1 || a0 == 2 {
                sys_write_uart(pt_root, a1, a2)
            } else {
                vfs::sys_write_fd(ks, pid, pt_root, a0, a1, a2)
            };
            finish_syscall(tf, res)
        }
        SYS_READ => {
            if a0 == 0 && !ks.procs.get(&pid).is_some_and(|p| p.fds.has_stdin()) {
                match sys_read_stdin(ks, pt_root, a1, a2) {
                    StdinRead::Done(res) => finish_syscall(tf, res),
                    StdinRead::WouldBlock => {
                        // Keep `sepc` pointing at `ecall` so wakeup retries `read`.
                        ks.schedule_next()
                    }
                }
            } else {
                let res = vfs::sys_read_fd(ks, pid, pt_root, a0, a1, a2);
                finish_syscall(tf, res)
            }
        }
        SYS_OPEN => {
            let res = vfs::sys_open(ks, pid, pt_root, a0, a1, a2);
            finish_syscall(tf, res)
        }
        SYS_CLOSE => {
            let res = vfs::sys_close(ks, pid, a0);
            finish_syscall(tf, res)
        }
        SYS_MKDIR => {
            let res = vfs::sys_mkdir(ks, pt_root, a0, a1);
            finish_syscall(tf, res)
        }
        SYS_UNLINK => {
            let res = vfs::sys_unlink(ks, pt_root, a0, a1);
            finish_syscall(tf, res)
        }
        SYS_READDIR => {
            let res = vfs::sys_readdir(ks, pid, pt_root, a0, a1, a2);
            finish_syscall(tf, res)
        }
        SYS_SPAWN => {
            let res = sys_spawn(ks, pid, pt_root, a0, a1, a2, a3, a4, a5);
            finish_syscall(tf, res)
        }
        SYS_WAIT => {
            let Ok(child_pid) = Pid::try_from(a0) else {
                return finish_syscall(tf, Errno::BadArg.as_isize());
            };
            if child_pid == 0 {
                return finish_syscall(tf, Errno::BadArg.as_isize());
            }
            match ks.wait_child(pid, child_pid) {
                Ok(WaitStatus::Exited(code)) => finish_syscall(tf, code as isize),
                Ok(WaitStatus::WouldBlock) => {
                    // Keep `sepc` pointing at `ecall` so wakeup retries `wait`.
                    ks.schedule_next()
                }
                Err(e) => finish_syscall(tf, e.as_isize()),
            }
        }
        SYS_KILL => {
            let res = match Pid::try_from(a0) {
                Ok(target) if target != 0 => match ks.kill_child(pid, target) {
                    Ok(()) => 0,
                    Err(e) => e.as_isize(),
                },
                _ => Errno::BadArg.as_isize(),
            };
            finish_syscall(tf, res)
        }
        SYS_GETPID => finish_syscall(tf, pid as isize),
        SYS_YIELD => {
            tf.sepc = tf.sepc.wrapping_add(4);
            tf.regs[10] = 0;
            ks.schedule_next()
        }
        SYS_SBRK => {
            let delta = a0 as i64;
            let res = match ks.sbrk(pid, delta) {
                Ok(old_brk) => old_brk as isize,
                Err(e) => e.as_isize(),
            };
            finish_syscall(tf, res)
        }
        SYS_PS => {
            let res = sys_ps(ks, pt_root, a0, a1);
            finish_syscall(tf, res)
        }
        SYS_UPTIME => {
            let ticks = TICKS.load(Ordering::Relaxed) as isize;
            finish_syscall(tf, ticks)
        }
        _ => finish_syscall(tf, Errno::BadArg.as_isize()),
    }
}

fn finish_syscall(tf: &mut TrapFrame, ret: isize) -> *mut TrapFrame {
    tf.regs[10] = ret as u64;
    tf.sepc = tf.sepc.wrapping_add(4);
    tf as *mut TrapFrame
}

fn sys_write_uart(pt_root: u64, ptr: u64, len: u64) -> isize {
    if let Err(e) = vm::validate_user_read(pt_root, ptr, len) {
        return e.as_isize();
    }
    let mut off = 0u64;
    let mut buf = [0u8; 256];
    while off < len {
        let chunk = (len - off).min(buf.len() as u64) as usize;
        if let Err(e) = vm::copy_from_user(pt_root, ptr + off, &mut buf[..chunk]) {
            return e.as_isize();
        }
        for &b in &buf[..chunk] {
            let _ = Uart.write_char(b as char);
        }
        off += chunk as u64;
    }
    len as isize
}

enum StdinRead {
    Done(isize),
    WouldBlock,
}

fn uart_rx_ready() -> bool {
    // SAFETY: `UART_BASE + 5` is the 1-byte 16550 LSR register mapped in kernel/user page tables.
    let lsr = unsafe { read_volatile((vm::UART_BASE + 5) as *const u8) };
    (lsr & 1) != 0
}

fn uart_rx_byte() -> u8 {
    // SAFETY: `UART_BASE` is the 1-byte 16550 RBR register mapped in kernel/user page tables.
    unsafe { read_volatile(vm::UART_BASE as *const u8) }
}

fn sys_read_stdin(ks: &mut KernelState, pt_root: u64, ptr: u64, len: u64) -> StdinRead {
    if let Err(e) = vm::validate_user_write(pt_root, ptr, len) {
        return StdinRead::Done(e.as_isize());
    }
    if len == 0 {
        return StdinRead::Done(0);
    }
    if uart_rx_ready() {
        ks.uart_empty_since = None;
        let target = len.min(4096) as usize;
        let mut total = 0usize;
        let mut buf = [0u8; 256];
        while total < target && uart_rx_ready() {
            let mut chunk = 0usize;
            let limit = (target - total).min(buf.len());
            while chunk < limit && uart_rx_ready() {
                buf[chunk] = uart_rx_byte();
                chunk += 1;
            }
            if chunk == 0 {
                break;
            }
            if let Err(e) = vm::copy_to_user(pt_root, ptr + total as u64, &buf[..chunk]) {
                return StdinRead::Done(e.as_isize());
            }
            total += chunk;
        }
        return StdinRead::Done(total as isize);
    }

    let now = TICKS.load(Ordering::Relaxed);
    match ks.uart_empty_since {
        None => {
            ks.uart_empty_since = Some(now);
            StdinRead::WouldBlock
        }
        Some(since) if now.wrapping_sub(since) < 1 => StdinRead::WouldBlock,
        Some(_) => {
            ks.uart_empty_since = None;
            StdinRead::Done(0)
        }
    }
}

const MAX_SPAWN_ARGC: usize = 16;
const MAX_ARG_LEN: usize = 255;
const MAX_ARGV_BYTES: usize = 4096;

fn read_user_cstr(pt_root: u64, ptr: u64, max_len: usize) -> Result<Vec<u8>, Errno> {
    let mut out = Vec::new();
    let mut va = ptr;
    let max_with_nul = max_len + 1;
    let mut chunk = [0u8; 64];
    while out.len() < max_with_nul {
        let page_rem = (PAGE_SIZE - (va & (PAGE_SIZE - 1))) as usize;
        let want = (max_with_nul - out.len()).min(page_rem).min(chunk.len());
        vm::copy_from_user(pt_root, va, &mut chunk[..want])?;
        if let Some(nul_pos) = chunk[..want].iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..nul_pos]);
            return Ok(out);
        }
        out.extend_from_slice(&chunk[..want]);
        va = va.checked_add(want as u64).ok_or(Errno::BadArg)?;
    }
    Err(Errno::BadArg)
}

fn dup_parent_fd(ks: &KernelState, parent: Pid, fd: u64) -> Result<Option<OpenFd>, Errno> {
    if !(3..=15).contains(&fd) {
        return Ok(None);
    }
    let Some(proc) = ks.procs.get(&parent) else {
        return Err(Errno::BadArg);
    };
    let Some(open_fd) = proc.fds.get(fd) else {
        return Err(Errno::BadFd);
    };
    Ok(Some(open_fd.dup_for_child()))
}

#[allow(clippy::too_many_arguments)]
fn sys_spawn(
    ks: &mut KernelState,
    parent: Pid,
    pt_root: u64,
    path_ptr: u64,
    path_len: u64,
    argv_ptr: u64,
    argc: u64,
    stdin_fd: u64,
    stdout_fd: u64,
) -> isize {
    if path_len == 0 || path_len > 256 {
        let _ = vm::validate_user_read(pt_root, path_ptr, path_len);
        return Errno::BadArg.as_isize();
    }
    let mut buf = [0u8; 256];
    let len = path_len as usize;
    if let Err(e) = vm::copy_from_user(pt_root, path_ptr, &mut buf[..len]) {
        return e.as_isize();
    }
    let Ok(name) = core::str::from_utf8(&buf[..len]) else {
        return Errno::BadArg.as_isize();
    };
    if argc > MAX_SPAWN_ARGC as u64 {
        return Errno::BadArg.as_isize();
    }
    let mut args = Vec::new();
    if argc > 0 {
        let argc_usize = argc as usize;
        let mut ptrs_bytes = alloc::vec![0u8; argc_usize * 8];
        if let Err(e) = vm::copy_from_user(pt_root, argv_ptr, &mut ptrs_bytes) {
            return e.as_isize();
        }
        let mut total_bytes = 0usize;
        for chunk in ptrs_bytes.chunks_exact(8) {
            let arg_ptr = u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            let s = match read_user_cstr(pt_root, arg_ptr, MAX_ARG_LEN) {
                Ok(s) => s,
                Err(e) => return e.as_isize(),
            };
            total_bytes = total_bytes.saturating_add(s.len() + 1);
            if total_bytes > MAX_ARGV_BYTES {
                return Errno::BadArg.as_isize();
            }
            args.push(s);
        }
    }
    let stdin = match dup_parent_fd(ks, parent, stdin_fd) {
        Ok(fd) => fd,
        Err(e) => return e.as_isize(),
    };
    let stdout = match dup_parent_fd(ks, parent, stdout_fd) {
        Ok(fd) => fd,
        Err(e) => return e.as_isize(),
    };
    let Some(elf_bytes) = progs::find_builtin(name) else {
        return Errno::NotFound.as_isize();
    };
    match ks.spawn_process(Some(parent), name, elf_bytes, &args, stdin, stdout) {
        Ok(child_pid) => child_pid as isize,
        Err(e) => e.as_isize(),
    }
}

fn sys_ps(ks: &KernelState, pt_root: u64, buf_ptr: u64, buf_len: u64) -> isize {
    if let Err(e) = vm::validate_user_write(pt_root, buf_ptr, buf_len) {
        return e.as_isize();
    }
    let formatted = ks.table.format_ps();
    let bytes = formatted.as_bytes();
    let n = bytes.len().min(buf_len as usize);
    if let Err(e) = vm::copy_to_user(pt_root, buf_ptr, &bytes[..n]) {
        return e.as_isize();
    }
    n as isize
}
