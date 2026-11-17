//! Wrappers for system calls 0 to 16 and 25, numbers from docs/TITAN.md and contract C2.
#![allow(dead_code)] // the table has calls the atlas API does not expose yet
use super::ecall::call;

const EXIT: u64 = 0;
const WRITE: u64 = 1;
const READ: u64 = 2;
const OPEN: u64 = 3;
const CLOSE: u64 = 4;
const SPAWN: u64 = 5;
const WAIT: u64 = 6;
const KILL: u64 = 7;
const GETPID: u64 = 8;
const YIELD: u64 = 9;
const SBRK: u64 = 10;
const MKDIR: u64 = 11;
const UNLINK: u64 = 12;
const READDIR: u64 = 13;
const PS: u64 = 14;
const UPTIME: u64 = 15;
const RESULT: u64 = 16;
const CHILD_RESULT: u64 = 25;

/// Ends the process with `code`; never returns.
pub fn exit(code: i32) -> ! {
    loop {
        call(EXIT, [code as u64, 0, 0, 0, 0, 0]);
    }
}

/// Writes `buf` to `fd`.
pub fn write(fd: u64, buf: &[u8]) -> isize {
    call(WRITE, [fd, buf.as_ptr() as u64, buf.len() as u64, 0, 0, 0])
}

/// Reads from `fd` into `buf`.
pub fn read(fd: u64, buf: &mut [u8]) -> isize {
    call(
        READ,
        [fd, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0],
    )
}

/// Opens `path` with the flags of docs/TITAN.md.
pub fn open(path: &str, flags: u64) -> isize {
    call(
        OPEN,
        [path.as_ptr() as u64, path.len() as u64, flags, 0, 0, 0],
    )
}

/// Closes `fd`.
pub fn close(fd: u64) -> isize {
    call(CLOSE, [fd, 0, 0, 0, 0, 0])
}

/// Spawns `path` with argv pointers `argv`; `stdin`/`stdout` are parent fds or -1 for the console.
pub fn spawn(path: &str, argv: &[*const u8], stdin: i64, stdout: i64) -> isize {
    call(
        SPAWN,
        [
            path.as_ptr() as u64,
            path.len() as u64,
            argv.as_ptr() as u64,
            argv.len() as u64,
            stdin as u64,
            stdout as u64,
        ],
    )
}

/// Waits for `pid` and returns its exit code, or a negative error.
pub fn wait(pid: u64) -> isize {
    call(WAIT, [pid, 0, 0, 0, 0, 0])
}

/// Kills `pid`.
pub fn kill(pid: u64) -> isize {
    call(KILL, [pid, 0, 0, 0, 0, 0])
}

/// Returns this process id.
pub fn getpid() -> isize {
    call(GETPID, [0; 6])
}

/// Gives up the CPU.
pub fn yield_now() -> isize {
    call(YIELD, [0; 6])
}

/// Moves the heap break by `delta` bytes.
pub fn sbrk(delta: isize) -> isize {
    call(SBRK, [delta as i64 as u64, 0, 0, 0, 0, 0])
}

/// Creates a directory.
pub fn mkdir(path: &str) -> isize {
    call(MKDIR, [path.as_ptr() as u64, path.len() as u64, 0, 0, 0, 0])
}

/// Removes a file.
pub fn unlink(path: &str) -> isize {
    call(
        UNLINK,
        [path.as_ptr() as u64, path.len() as u64, 0, 0, 0, 0],
    )
}

/// Reads directory entries from `fd` as newline-terminated names.
pub fn readdir(fd: u64, buf: &mut [u8]) -> isize {
    call(
        READDIR,
        [fd, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0],
    )
}

/// Writes the process table into `buf`.
pub fn ps(buf: &mut [u8]) -> isize {
    call(PS, [buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0, 0])
}

/// Kernel tick counter.
pub fn uptime() -> isize {
    call(UPTIME, [0; 6])
}

/// Stores `envelope` as this process's result.
pub fn result(envelope: &str) -> isize {
    call(
        RESULT,
        [envelope.as_ptr() as u64, envelope.len() as u64, 0, 0, 0, 0],
    )
}

/// Copies the result of child `pid` into `buf`.
pub fn child_result(pid: u64, buf: &mut [u8]) -> isize {
    call(
        CHILD_RESULT,
        [pid, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0],
    )
}
