//! Per-process file descriptor table and `hades::Fs`-backed filesystem syscalls.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use hades::{Error as FsError, Kind};
use titan::proc::Pid;
use titan::uaccess::Errno;

use crate::proc::KernelState;
use crate::vm;

pub const FD_MIN: usize = 3;
pub const FD_MAX: usize = 15;
const FD_SLOTS: usize = FD_MAX - FD_MIN + 1;

const O_READ: u64 = 1;
const O_WRITE: u64 = 2;
const O_CREATE: u64 = 4;
const O_TRUNC: u64 = 8;
const O_ALL: u64 = O_READ | O_WRITE | O_CREATE | O_TRUNC;

const MAX_PATH_LEN: u64 = 1024;
const IO_CHUNK: usize = 4096;

/// One open file or directory descriptor owned by a process.
pub struct OpenFd {
    pub path: String,
    pub offset: u64,
    pub flags: u64,
    pub is_dir: bool,
    pub dir_entries: Option<Vec<String>>,
}

/// Fixed-size per-process descriptor table covering fds `3..=15`.
pub struct FdTable {
    slots: [Option<OpenFd>; FD_SLOTS],
}

impl FdTable {
    pub const fn new() -> Self {
        const EMPTY: Option<OpenFd> = None;
        Self {
            slots: [EMPTY; FD_SLOTS],
        }
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
    }

    fn find_free(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    fn slot_index(fd: u64) -> Option<usize> {
        let fd_usize = usize::try_from(fd).ok()?;
        if (FD_MIN..=FD_MAX).contains(&fd_usize) {
            Some(fd_usize - FD_MIN)
        } else {
            None
        }
    }

    fn get_mut(&mut self, fd: u64) -> Option<&mut OpenFd> {
        let idx = Self::slot_index(fd)?;
        self.slots.get_mut(idx)?.as_mut()
    }

    fn close(&mut self, fd: u64) -> bool {
        let Some(idx) = Self::slot_index(fd) else {
            return false;
        };
        if self.slots[idx].is_some() {
            self.slots[idx] = None;
            true
        } else {
            false
        }
    }
}

fn map_fs_err(err: FsError) -> Errno {
    match err {
        FsError::NotFound => Errno::NotFound,
        FsError::Exists => Errno::Exists,
        FsError::NotDir => Errno::NotDir,
        FsError::NotEmpty => Errno::Busy,
        FsError::NoSpace | FsError::TooLarge => Errno::NoMem,
        FsError::Io
        | FsError::Corrupt
        | FsError::IsDir
        | FsError::NameTooLong
        | FsError::BadPath => Errno::BadArg,
    }
}

fn read_user_path(pt_root: u64, ptr: u64, len: u64) -> Result<String, Errno> {
    if len == 0 || len > MAX_PATH_LEN {
        let _ = vm::validate_user_read(pt_root, ptr, len);
        return Err(Errno::BadArg);
    }
    let mut buf = vec![0u8; len as usize];
    vm::copy_from_user(pt_root, ptr, &mut buf)?;
    let s = core::str::from_utf8(&buf).map_err(|_| Errno::BadArg)?;
    Ok(String::from(s))
}

pub fn sys_open(
    ks: &mut KernelState,
    pid: Pid,
    pt_root: u64,
    path_ptr: u64,
    path_len: u64,
    flags: u64,
) -> isize {
    if (flags & !O_ALL) != 0
        || (flags & (O_READ | O_WRITE)) == 0
        || ((flags & O_TRUNC) != 0 && (flags & O_WRITE) == 0)
    {
        return Errno::BadArg.as_isize();
    }
    let path = match read_user_path(pt_root, path_ptr, path_len) {
        Ok(p) => p,
        Err(e) => return e.as_isize(),
    };
    let Some(fs) = ks.fs.as_mut() else {
        return Errno::NotFound.as_isize();
    };
    let Some(slot_idx) = ks.procs.get(&pid).and_then(|p| p.fds.find_free()) else {
        return Errno::NoMem.as_isize();
    };

    let entry = match fs.stat(&path) {
        Ok(st) if st.kind == Kind::Dir => {
            if flags != O_READ {
                return Errno::BadArg.as_isize();
            }
            OpenFd {
                path,
                offset: 0,
                flags,
                is_dir: true,
                dir_entries: None,
            }
        }
        Ok(_) => {
            if (flags & O_TRUNC) != 0 {
                if let Err(e) = fs.truncate(&path, 0) {
                    return map_fs_err(e).as_isize();
                }
            }
            OpenFd {
                path,
                offset: 0,
                flags,
                is_dir: false,
                dir_entries: None,
            }
        }
        Err(FsError::NotFound) if (flags & O_CREATE) != 0 => {
            if let Err(e) = fs.create(&path) {
                return map_fs_err(e).as_isize();
            }
            OpenFd {
                path,
                offset: 0,
                flags,
                is_dir: false,
                dir_entries: None,
            }
        }
        Err(e) => return map_fs_err(e).as_isize(),
    };

    let Some(proc) = ks.procs.get_mut(&pid) else {
        return Errno::BadArg.as_isize();
    };
    proc.fds.slots[slot_idx] = Some(entry);
    (FD_MIN + slot_idx) as isize
}

pub fn sys_close(ks: &mut KernelState, pid: Pid, fd: u64) -> isize {
    let Some(proc) = ks.procs.get_mut(&pid) else {
        return Errno::BadArg.as_isize();
    };
    if proc.fds.close(fd) {
        0
    } else {
        Errno::BadFd.as_isize()
    }
}

pub fn sys_read_fd(
    ks: &mut KernelState,
    pid: Pid,
    pt_root: u64,
    fd: u64,
    ptr: u64,
    len: u64,
) -> isize {
    let (path, mut offset) = {
        let Some(proc) = ks.procs.get_mut(&pid) else {
            return Errno::BadArg.as_isize();
        };
        let Some(entry) = proc.fds.get_mut(fd) else {
            return Errno::BadFd.as_isize();
        };
        if entry.is_dir {
            return Errno::BadArg.as_isize();
        }
        if (entry.flags & O_READ) == 0 {
            return Errno::BadFd.as_isize();
        }
        (entry.path.clone(), entry.offset)
    };

    if let Err(e) = vm::validate_user_write(pt_root, ptr, len) {
        return e.as_isize();
    }
    if len == 0 {
        return 0;
    }
    let Some(fs) = ks.fs.as_mut() else {
        return Errno::BadFd.as_isize();
    };

    let mut total: usize = 0;
    let target = len as usize;
    let mut kbuf = vec![0u8; target.min(IO_CHUNK)];
    while total < target {
        let chunk = (target - total).min(kbuf.len());
        let n = match fs.read(&path, offset, &mut kbuf[..chunk]) {
            Ok(n) => n,
            Err(e) => {
                if total > 0 {
                    break;
                }
                return map_fs_err(e).as_isize();
            }
        };
        if n == 0 {
            break;
        }
        if let Err(e) = vm::copy_to_user(pt_root, ptr + total as u64, &kbuf[..n]) {
            return e.as_isize();
        }
        let Some(next_off) = offset.checked_add(n as u64) else {
            return Errno::BadArg.as_isize();
        };
        offset = next_off;
        total += n;
        if n < chunk {
            break;
        }
    }

    if let Some(entry) = ks.procs.get_mut(&pid).and_then(|p| p.fds.get_mut(fd)) {
        entry.offset = offset;
    }
    total as isize
}

pub fn sys_write_fd(
    ks: &mut KernelState,
    pid: Pid,
    pt_root: u64,
    fd: u64,
    ptr: u64,
    len: u64,
) -> isize {
    let (path, mut offset) = {
        let Some(proc) = ks.procs.get_mut(&pid) else {
            return Errno::BadArg.as_isize();
        };
        let Some(entry) = proc.fds.get_mut(fd) else {
            return Errno::BadFd.as_isize();
        };
        if entry.is_dir {
            return Errno::BadArg.as_isize();
        }
        if (entry.flags & O_WRITE) == 0 {
            return Errno::BadFd.as_isize();
        }
        (entry.path.clone(), entry.offset)
    };

    if let Err(e) = vm::validate_user_read(pt_root, ptr, len) {
        return e.as_isize();
    }
    if len == 0 {
        return 0;
    }
    let Some(fs) = ks.fs.as_mut() else {
        return Errno::BadFd.as_isize();
    };

    let mut total: usize = 0;
    let target = len as usize;
    let mut kbuf = vec![0u8; target.min(IO_CHUNK)];
    while total < target {
        let chunk = (target - total).min(kbuf.len());
        if let Err(e) = vm::copy_from_user(pt_root, ptr + total as u64, &mut kbuf[..chunk]) {
            return e.as_isize();
        }
        let n = match fs.write(&path, offset, &kbuf[..chunk]) {
            Ok(n) => n,
            Err(e) => {
                if total > 0 {
                    break;
                }
                return map_fs_err(e).as_isize();
            }
        };
        if n == 0 {
            break;
        }
        let Some(next_off) = offset.checked_add(n as u64) else {
            return Errno::BadArg.as_isize();
        };
        offset = next_off;
        total += n;
        if n < chunk {
            break;
        }
    }

    if let Some(entry) = ks.procs.get_mut(&pid).and_then(|p| p.fds.get_mut(fd)) {
        entry.offset = offset;
    }
    total as isize
}

pub fn sys_mkdir(ks: &mut KernelState, pt_root: u64, path_ptr: u64, path_len: u64) -> isize {
    let path = match read_user_path(pt_root, path_ptr, path_len) {
        Ok(p) => p,
        Err(e) => return e.as_isize(),
    };
    let Some(fs) = ks.fs.as_mut() else {
        return Errno::NotFound.as_isize();
    };
    match fs.mkdir(&path) {
        Ok(()) => 0,
        Err(e) => map_fs_err(e).as_isize(),
    }
}

pub fn sys_unlink(ks: &mut KernelState, pt_root: u64, path_ptr: u64, path_len: u64) -> isize {
    let path = match read_user_path(pt_root, path_ptr, path_len) {
        Ok(p) => p,
        Err(e) => return e.as_isize(),
    };
    let Some(fs) = ks.fs.as_mut() else {
        return Errno::NotFound.as_isize();
    };
    match fs.remove(&path) {
        Ok(()) => 0,
        Err(FsError::IsDir) => match fs.rmdir(&path) {
            Ok(()) => 0,
            Err(e) => map_fs_err(e).as_isize(),
        },
        Err(e) => map_fs_err(e).as_isize(),
    }
}

pub fn sys_readdir(
    ks: &mut KernelState,
    pid: Pid,
    pt_root: u64,
    fd: u64,
    buf_ptr: u64,
    buf_len: u64,
) -> isize {
    let (path, idx, need_list) = {
        let Some(proc) = ks.procs.get_mut(&pid) else {
            return Errno::BadArg.as_isize();
        };
        let Some(entry) = proc.fds.get_mut(fd) else {
            return Errno::BadFd.as_isize();
        };
        if !entry.is_dir {
            return Errno::NotDir.as_isize();
        }
        (
            entry.path.clone(),
            entry.offset as usize,
            entry.offset == 0 || entry.dir_entries.is_none(),
        )
    };

    if let Err(e) = vm::validate_user_write(pt_root, buf_ptr, buf_len) {
        return e.as_isize();
    }

    if need_list {
        let Some(fs) = ks.fs.as_mut() else {
            return Errno::NotFound.as_isize();
        };
        let listed = match fs.list(&path) {
            Ok(v) => v,
            Err(e) => return map_fs_err(e).as_isize(),
        };
        let names: Vec<String> = listed.into_iter().map(|e| e.name).collect();
        if let Some(entry) = ks.procs.get_mut(&pid).and_then(|p| p.fds.get_mut(fd)) {
            entry.dir_entries = Some(names);
        }
    }

    let Some(proc) = ks.procs.get_mut(&pid) else {
        return Errno::BadArg.as_isize();
    };
    let Some(entry) = proc.fds.get_mut(fd) else {
        return Errno::BadFd.as_isize();
    };
    let Some(names) = entry.dir_entries.as_ref() else {
        return 0;
    };
    if idx >= names.len() {
        return 0;
    }

    let cap = buf_len as usize;
    let first_need = names[idx].len().saturating_add(1);
    if first_need > cap {
        return Errno::BadArg.as_isize();
    }

    let mut out = Vec::new();
    let mut next = idx;
    while next < names.len() {
        let need = names[next].len().saturating_add(1);
        if out.len().saturating_add(need) > cap {
            break;
        }
        out.extend_from_slice(names[next].as_bytes());
        out.push(b'\n');
        next += 1;
    }

    if let Err(e) = vm::copy_to_user(pt_root, buf_ptr, &out) {
        return e.as_isize();
    }
    entry.offset = next as u64;
    out.len() as isize
}
