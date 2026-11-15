//! Userland thin syscall wrappers, formatting helpers, and bump `#[global_allocator]` over `sbrk`.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::cell::UnsafeCell;
use core::fmt::{self, Write};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, Ordering};

pub const O_READ: u64 = 1;
pub const O_WRITE: u64 = 2;
pub const O_CREATE: u64 = 4;
pub const O_TRUNC: u64 = 8;
pub const O_APPEND: u64 = 16;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 2;
const SYS_OPEN: u64 = 3;
const SYS_CLOSE: u64 = 4;
const SYS_SPAWN: u64 = 5;
const SYS_WAIT: u64 = 6;
const SYS_GETPID: u64 = 8;
const SYS_SBRK: u64 = 10;
const SYS_MKDIR: u64 = 11;
const SYS_UNLINK: u64 = 12;
const SYS_READDIR: u64 = 13;
const SYS_PS: u64 = 14;

#[inline(always)]
unsafe fn syscall0(num: u64) -> isize {
    let ret: isize;
    // SAFETY: Caller ensures syscall number and register ABI contract are valid.
    unsafe {
        asm!(
            "ecall",
            in("a7") num,
            lateout("a0") ret,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall1(num: u64, a0: u64) -> isize {
    let ret: isize;
    // SAFETY: Caller ensures syscall number and argument `a0` satisfy the kernel ABI.
    unsafe {
        asm!(
            "ecall",
            in("a7") num,
            inout("a0") a0 => ret,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall2(num: u64, a0: u64, a1: u64) -> isize {
    let ret: isize;
    // SAFETY: Caller ensures syscall number and arguments satisfy the kernel ABI.
    unsafe {
        asm!(
            "ecall",
            in("a7") num,
            inout("a0") a0 => ret,
            in("a1") a1,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall3(num: u64, a0: u64, a1: u64, a2: u64) -> isize {
    let ret: isize;
    // SAFETY: Caller ensures syscall number and arguments satisfy the kernel ABI.
    unsafe {
        asm!(
            "ecall",
            in("a7") num,
            inout("a0") a0 => ret,
            in("a1") a1,
            in("a2") a2,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall6(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> isize {
    let ret: isize;
    // SAFETY: Caller ensures syscall number and arguments satisfy the kernel ABI.
    unsafe {
        asm!(
            "ecall",
            in("a7") num,
            inout("a0") a0 => ret,
            in("a1") a1,
            in("a2") a2,
            in("a3") a3,
            in("a4") a4,
            in("a5") a5,
            options(nostack),
        );
    }
    ret
}

pub fn exit(code: i32) -> ! {
    // SAFETY: `SYS_EXIT` terminates the current process and never returns.
    unsafe {
        asm!(
            "ecall",
            in("a7") SYS_EXIT,
            in("a0") code as i64 as u64,
            options(noreturn, nostack),
        );
    }
}

pub fn write(fd: usize, buf: &[u8]) -> isize {
    // SAFETY: `buf.as_ptr()..buf.as_ptr()+buf.len()` is a valid user slice.
    unsafe { syscall3(SYS_WRITE, fd as u64, buf.as_ptr() as u64, buf.len() as u64) }
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    // SAFETY: `buf.as_mut_ptr()..buf.as_mut_ptr()+buf.len()` is a valid writable user slice.
    unsafe {
        syscall3(
            SYS_READ,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    }
}

pub fn open(path: &str, flags: u64) -> isize {
    // SAFETY: `path` is a valid UTF-8 slice in user memory.
    unsafe { syscall3(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, flags) }
}

pub fn close(fd: usize) -> isize {
    // SAFETY: `SYS_CLOSE` validates `fd` in the kernel.
    unsafe { syscall1(SYS_CLOSE, fd as u64) }
}

pub fn spawn(name: &str, argv: &[*const u8], stdin_fd: i64, stdout_fd: i64) -> isize {
    // SAFETY: `name` and `argv` pointers refer to valid user memory checked by the kernel.
    unsafe {
        syscall6(
            SYS_SPAWN,
            name.as_ptr() as u64,
            name.len() as u64,
            argv.as_ptr() as u64,
            argv.len() as u64,
            stdin_fd as u64,
            stdout_fd as u64,
        )
    }
}

pub fn wait(pid: usize) -> isize {
    // SAFETY: `SYS_WAIT` checks `pid` ownership in the kernel.
    unsafe { syscall1(SYS_WAIT, pid as u64) }
}

pub fn getpid() -> isize {
    // SAFETY: `SYS_GETPID` takes no pointer arguments.
    unsafe { syscall0(SYS_GETPID) }
}

pub fn sbrk(delta: isize) -> isize {
    // SAFETY: `SYS_SBRK` grows/shrinks the user heap safely in the kernel.
    unsafe { syscall1(SYS_SBRK, delta as i64 as u64) }
}

pub fn mkdir(path: &str) -> isize {
    // SAFETY: `path` is a valid slice in user memory.
    unsafe { syscall2(SYS_MKDIR, path.as_ptr() as u64, path.len() as u64) }
}

pub fn unlink(path: &str) -> isize {
    // SAFETY: `path` is a valid slice in user memory.
    unsafe { syscall2(SYS_UNLINK, path.as_ptr() as u64, path.len() as u64) }
}

pub fn readdir(fd: usize, buf: &mut [u8]) -> isize {
    // SAFETY: `buf` is a valid writable slice in user memory.
    unsafe {
        syscall3(
            SYS_READDIR,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    }
}

pub fn ps(buf: &mut [u8]) -> isize {
    // SAFETY: `buf` is a valid writable slice in user memory.
    unsafe { syscall2(SYS_PS, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn write_all(fd: usize, mut data: &[u8]) -> isize {
    while !data.is_empty() {
        let n = write(fd, data);
        if n <= 0 {
            return if n < 0 { n } else { -1 };
        }
        data = &data[n as usize..];
    }
    0
}

pub struct FdWriter(pub usize);

impl Write for FdWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if write_all(self.0, s.as_bytes()) < 0 {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

struct BumpState {
    pos: usize,
    limit: usize,
}

pub struct BumpAllocator {
    lock: AtomicBool,
    state: UnsafeCell<BumpState>,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
            state: UnsafeCell::new(BumpState { pos: 0, limit: 0 }),
        }
    }

    fn acquire(&self) {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

// SAFETY: All reads and writes to `state` are serialized by `self.lock`.
unsafe impl Sync for BumpAllocator {}

// SAFETY: Allocator returns suitably aligned blocks obtained via `sbrk` within `[heap_base, limit)`.
unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.acquire();
        // SAFETY: `self.lock` is held exclusively for the duration of this allocation.
        let state = unsafe { &mut *self.state.get() };
        if state.limit == 0 {
            let base = sbrk(0);
            if base < 0 {
                self.release();
                return null_mut();
            }
            state.pos = base as usize;
            state.limit = base as usize;
        }
        let align = layout.align().max(8);
        let Some(aligned) = state.pos.checked_add(align - 1).map(|v| v & !(align - 1)) else {
            self.release();
            return null_mut();
        };
        let Some(end) = aligned.checked_add(layout.size()) else {
            self.release();
            return null_mut();
        };
        if end > state.limit {
            let need = end - state.limit;
            let grow = need.checked_add(4095).map_or(need, |v| v & !4095);
            let mut got = 0usize;
            if let Ok(delta) = isize::try_from(grow) {
                let res = sbrk(delta);
                if res >= 0 {
                    got = grow;
                }
            }
            if got == 0 {
                let Ok(delta) = isize::try_from(need) else {
                    self.release();
                    return null_mut();
                };
                let res = sbrk(delta);
                if res < 0 {
                    self.release();
                    return null_mut();
                }
                got = need;
            }
            state.limit += got;
        }
        state.pos = end;
        self.release();
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
