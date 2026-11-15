//! Linux system calls through the C library std already links (no external crates).
//! Every pointer handed to the C library is built before `fork`, so the children only make
//! async-signal-safe calls (no allocation) between `fork` and `execve`.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cgroup::Cgroup;
use crate::spec::Spec;

const CLONE_NEWNS: c_int = 0x0002_0000;
const CLONE_NEWUTS: c_int = 0x0400_0000;
const CLONE_NEWIPC: c_int = 0x0800_0000;
const CLONE_NEWUSER: c_int = 0x1000_0000;
const CLONE_NEWPID: c_int = 0x2000_0000;
const CLONE_NEWNET: c_int = 0x4000_0000;
const MS_NOSUID: c_ulong = 2;
const MS_NODEV: c_ulong = 4;
const MS_NOEXEC: c_ulong = 8;
const MS_BIND: c_ulong = 4096;
const MS_REC: c_ulong = 16384;
const MS_PRIVATE: c_ulong = 1 << 18;
const MNT_DETACH: c_int = 2;
#[cfg(target_arch = "x86_64")]
const SYS_PIVOT_ROOT: c_long = 155;
#[cfg(not(target_arch = "x86_64"))]
const SYS_PIVOT_ROOT: c_long = 41; // aarch64 and riscv64 share the generic table

extern "C" {
    fn fork() -> c_int;
    fn unshare(flags: c_int) -> c_int;
    fn sethostname(name: *const c_char, len: usize) -> c_int;
    fn mount(
        src: *const c_char,
        target: *const c_char,
        fstype: *const c_char,
        flags: c_ulong,
        data: *const c_void,
    ) -> c_int;
    fn umount2(target: *const c_char, flags: c_int) -> c_int;
    fn syscall(num: c_long, ...) -> c_long;
    fn chdir(path: *const c_char) -> c_int;
    fn mkdir(path: *const c_char, mode: u32) -> c_int;
    fn rmdir(path: *const c_char) -> c_int;
    fn execve(path: *const c_char, argv: *const *const c_char, envp: *const *const c_char)
        -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    fn close(fd: c_int) -> c_int;
    fn _exit(code: c_int) -> !;
    fn getuid() -> u32;
    fn getgid() -> u32;
}

/// Everything the children need, as C pointers owned by this struct.
struct Prepared {
    rootfs: CString,
    hostname: CString,
    argv: Vec<CString>,
    envp: Vec<CString>,
    argv_ptrs: Vec<*const c_char>,
    envp_ptrs: Vec<*const c_char>,
}

fn cstr(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL byte"))
}

fn prepare(spec: &Spec) -> io::Result<Prepared> {
    let argv = spec
        .argv
        .iter()
        .map(|a| cstr(a))
        .collect::<io::Result<Vec<_>>>()?;
    let envp = spec
        .env
        .iter()
        .map(|(k, v)| cstr(&format!("{k}={v}")))
        .collect::<io::Result<Vec<_>>>()?;
    let mut p = Prepared {
        rootfs: cstr(&spec.rootfs.to_string_lossy())?,
        hostname: cstr(&spec.hostname)?,
        argv,
        envp,
        argv_ptrs: Vec::new(),
        envp_ptrs: Vec::new(),
    };
    p.argv_ptrs = p
        .argv
        .iter()
        .map(|c| c.as_ptr())
        .chain([ptr::null()])
        .collect();
    p.envp_ptrs = p
        .envp
        .iter()
        .map(|c| c.as_ptr())
        .chain([ptr::null()])
        .collect();
    Ok(p)
}

/// Write a fixed message to stderr and exit; safe to call after fork.
fn die(msg: &[u8], code: c_int) -> ! {
    // SAFETY: msg is a valid slice for its whole length; write and _exit are async-signal-safe.
    unsafe {
        write(2, msg.as_ptr().cast(), msg.len());
        _exit(code)
    }
}

fn exit_code(status: c_int) -> i32 {
    if status & 0x7f == 0 {
        (status >> 8) & 0xff
    } else {
        128 + (status & 0x7f)
    }
}

/// Child B, PID 1 of the new namespaces: hostname, private mounts, pivot_root, /proc, exec.
fn stage_and_exec(p: &Prepared) -> ! {
    let c = |s: &'static [u8]| s.as_ptr().cast::<c_char>();
    // SAFETY: every pointer is either a CString owned by `p` (alive until exec or exit) or a static
    // NUL-terminated literal; only async-signal-safe libc calls are made.
    unsafe {
        if sethostname(p.hostname.as_ptr(), p.hostname.as_bytes().len()) != 0 {
            die(b"pandora: sethostname failed\n", 126);
        }
        if mount(
            ptr::null(),
            c(b"/\0"),
            ptr::null(),
            MS_REC | MS_PRIVATE,
            ptr::null(),
        ) != 0
        {
            die(b"pandora: making / private failed\n", 126);
        }
        if mount(
            p.rootfs.as_ptr(),
            p.rootfs.as_ptr(),
            ptr::null(),
            MS_BIND | MS_REC,
            ptr::null(),
        ) != 0
        {
            die(b"pandora: bind mount of rootfs failed\n", 126);
        }
        if chdir(p.rootfs.as_ptr()) != 0 {
            die(b"pandora: chdir rootfs failed\n", 126);
        }
        mkdir(c(b".oldroot\0"), 0o700); // may already exist; pivot_root reports a real problem
        if syscall(SYS_PIVOT_ROOT, c(b".\0"), c(b".oldroot\0")) != 0 {
            die(b"pandora: pivot_root failed\n", 126);
        }
        if chdir(c(b"/\0")) != 0 {
            die(b"pandora: chdir / failed\n", 126);
        }
        if mount(
            c(b"proc\0"),
            c(b"/proc\0"),
            c(b"proc\0"),
            MS_NOSUID | MS_NODEV | MS_NOEXEC,
            ptr::null(),
        ) != 0
        {
            die(b"pandora: mounting /proc failed\n", 126);
        }
        if umount2(c(b"/.oldroot\0"), MNT_DETACH) != 0 {
            die(b"pandora: detaching old root failed\n", 126);
        }
        rmdir(c(b"/.oldroot\0"));
        execve(p.argv_ptrs[0], p.argv_ptrs.as_ptr(), p.envp_ptrs.as_ptr());
    }
    die(b"pandora: exec failed\n", 127)
}

static SERIAL: AtomicU32 = AtomicU32::new(0);

pub(crate) fn kill_proc(pid: i32, sig: i32) -> io::Result<()> {
    if pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pid must be positive",
        ));
    }
    // SAFETY: `pid` is positive and `sig` is passed directly to POSIX `kill(2)`.
    if unsafe { kill(pid, sig) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn run_with_pid<F>(spec: &Spec, cgroup_base: &Path, on_pid: F) -> io::Result<i32>
where
    F: FnOnce(i32) -> io::Result<()>,
{
    spec.validate()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    fs::create_dir_all(spec.rootfs.join("proc"))?;
    let p = prepare(spec)?;
    let cg = if spec.limits.is_empty() {
        None
    } else {
        let name = format!(
            "pandora-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        );
        Some(
            Cgroup::create(cgroup_base, &name, &spec.limits).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "cgroup v2 at {} (needs root or delegation): {e}",
                        cgroup_base.display()
                    ),
                )
            })?,
        )
    };
    let (mut up, mut down) = ([0 as c_int; 2], [0 as c_int; 2]);
    // SAFETY: each array has room for the two descriptors `pipe` writes.
    if unsafe { pipe(up.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `down` has room for two descriptors; close `up` on failure.
    if unsafe { pipe(down.as_mut_ptr()) } != 0 {
        let err = io::Error::last_os_error();
        // SAFETY: `up` holds valid open descriptors from `pipe` above.
        unsafe {
            close(up[0]);
            close(up[1]);
        }
        return Err(err);
    }
    let flags =
        CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWUTS | CLONE_NEWIPC | CLONE_NEWNET;
    // SAFETY: the child (pid 0) only makes async-signal-safe calls on memory prepared before the fork.
    let a = unsafe { fork() };
    if a < 0 {
        let err = io::Error::last_os_error();
        // SAFETY: `up` and `down` hold valid open descriptors from `pipe` above.
        unsafe {
            close(up[0]);
            close(up[1]);
            close(down[0]);
            close(down[1]);
        }
        return Err(err);
    }
    if a == 0 {
        let mut byte = 0u8;
        // SAFETY: descriptors come from pipe above; `byte` and `pid_bytes` are valid stack buffers.
        unsafe {
            close(up[0]);
            close(down[1]);
            if unshare(flags) != 0 {
                close(up[1]);
                close(down[0]);
                die(b"pandora: unshare failed (user namespaces blocked?)\n", 125);
            }
            write(up[1], (&byte as *const u8).cast(), 1);
            if read(down[0], (&mut byte as *mut u8).cast(), 1) != 1 {
                close(up[1]);
                close(down[0]);
                _exit(125); // parent could not write the id maps and has already reported why
            }
            close(down[0]);
            let b = fork();
            if b < 0 {
                close(up[1]);
                die(b"pandora: fork failed\n", 125);
            }
            if b == 0 {
                close(up[1]);
                stage_and_exec(&p);
            }
            let b_pid: i32 = b;
            let pid_bytes = b_pid.to_le_bytes();
            let mut off = 0usize;
            while off < pid_bytes.len() {
                let n = write(
                    up[1],
                    pid_bytes[off..].as_ptr().cast(),
                    pid_bytes.len() - off,
                );
                if n <= 0 {
                    break;
                }
                off += n as usize;
            }
            close(up[1]);
            let mut status = 0;
            if waitpid(b, &mut status, 0) != b {
                _exit(125);
            }
            _exit(exit_code(status));
        }
    }
    // SAFETY: closing our copies of the child's pipe ends.
    unsafe {
        close(up[1]);
        close(down[0]);
    }
    let mut byte = 0u8;
    // SAFETY: valid one-byte buffer and descriptor.
    let got = unsafe { read(up[0], (&mut byte as *mut u8).cast(), 1) };
    let setup = (|| -> io::Result<()> {
        if got != 1 {
            return Err(io::Error::other(
                "container process failed before namespaces were ready",
            ));
        }
        if let Some(cg) = &cg {
            cg.add(a as u32)?;
        }
        // SAFETY: getuid/getgid cannot fail.
        let (uid, gid) = unsafe { (getuid(), getgid()) };
        fs::write(format!("/proc/{a}/setgroups"), "deny")?;
        fs::write(format!("/proc/{a}/uid_map"), format!("0 {uid} 1\n"))?;
        fs::write(format!("/proc/{a}/gid_map"), format!("0 {gid} 1\n"))?;
        Ok(())
    })();
    if setup.is_ok() {
        // SAFETY: valid one-byte buffer and descriptor.
        unsafe { write(down[1], (&byte as *const u8).cast(), 1) };
    }
    // SAFETY: closing the write end; the child sees EOF if setup failed and exits.
    unsafe { close(down[1]) };
    let mut pid_cb_res = Ok(());
    if setup.is_ok() {
        let mut pid_buf = [0u8; 4];
        let mut off = 0usize;
        while off < 4 {
            // SAFETY: `pid_buf[off..]` has `4 - off` writable bytes and `up[0]` is open.
            let n = unsafe { read(up[0], pid_buf[off..].as_mut_ptr().cast(), 4 - off) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if n == 0 {
                break;
            }
            off += n as usize;
        }
        if off == 4 {
            let child_b_pid = i32::from_le_bytes(pid_buf);
            if let Err(e) = on_pid(child_b_pid) {
                let _ = kill_proc(child_b_pid, 9);
                pid_cb_res = Err(e);
            }
        }
    }
    // SAFETY: done with the read end.
    unsafe { close(up[0]) };
    let mut status = 0;
    // SAFETY: `a` is our child; status is a valid out pointer.
    if unsafe { waitpid(a, &mut status, 0) } != a {
        return Err(io::Error::last_os_error());
    }
    setup?;
    pid_cb_res?;
    drop(cg);
    Ok(exit_code(status))
}
