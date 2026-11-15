//! U-mode syscall dispatcher matching `docs/TITAN.md` numbers (`0..=15`) and `Errno` codes.

use core::fmt::Write;
use core::sync::atomic::Ordering;

use titan::proc::{Pid, WaitStatus};
use titan::uaccess::Errno;

use crate::proc::KernelState;
use crate::progs;
use crate::vfs;
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

    match num {
        SYS_EXIT => {
            let code = a0 as i32;
            ks.exit_current(code)
        }
        SYS_WRITE => {
            let res = if a0 == 1 || a0 == 2 {
                sys_write_uart(pt_root, a1, a2)
            } else {
                vfs::sys_write_fd(ks, pid, pt_root, a0, a1, a2)
            };
            finish_syscall(tf, res)
        }
        SYS_READ => {
            let res = if a0 == 0 {
                sys_read_stdin(pt_root, a1, a2)
            } else {
                vfs::sys_read_fd(ks, pid, pt_root, a0, a1, a2)
            };
            finish_syscall(tf, res)
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
            let res = sys_spawn(ks, pid, pt_root, a0, a1, a2, a3);
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

fn sys_read_stdin(pt_root: u64, ptr: u64, len: u64) -> isize {
    if let Err(e) = vm::validate_user_write(pt_root, ptr, len) {
        return e.as_isize();
    }
    0
}

fn sys_spawn(
    ks: &mut KernelState,
    parent: Pid,
    pt_root: u64,
    path_ptr: u64,
    path_len: u64,
    argv_ptr: u64,
    argc: u64,
) -> isize {
    if argc > 0 {
        let Some(argv_bytes) = argc.checked_mul(8) else {
            return Errno::BadArg.as_isize();
        };
        if let Err(e) = vm::validate_user_read(pt_root, argv_ptr, argv_bytes) {
            return e.as_isize();
        }
    }
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
    let Some(elf_bytes) = progs::find_builtin(name) else {
        return Errno::NotFound.as_isize();
    };
    match ks.spawn_process(Some(parent), name, elf_bytes) {
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
