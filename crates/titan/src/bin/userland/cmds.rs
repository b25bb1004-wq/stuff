//! Built-in multicall userland utilities: `ls`, `cat`, `echo`, `mkdir`, `rm`, `ps`.

use core::fmt::Write;

use crate::sys::{self, FdWriter};

fn prog_err(prog: &str, path: &str, code: isize) -> i32 {
    let _ = writeln!(FdWriter(2), "{prog}: {path}: error {code}");
    1
}

pub fn run_echo(args: &[&str]) -> i32 {
    let mut out = FdWriter(1);
    for (i, arg) in args.iter().skip(1).enumerate() {
        if i > 0 && out.write_str(" ").is_err() {
            return 1;
        }
        if out.write_str(arg).is_err() {
            return 1;
        }
    }
    if out.write_str("\n").is_err() {
        return 1;
    }
    0
}

pub fn run_ls(args: &[&str]) -> i32 {
    let dir = args.get(1).copied().unwrap_or("/");
    let fd = sys::open(dir, sys::O_READ);
    if fd < 0 {
        return prog_err("ls", dir, fd);
    }
    let mut buf = [0u8; 512];
    loop {
        let n = sys::readdir(fd as usize, &mut buf);
        if n < 0 {
            let _ = sys::close(fd as usize);
            return prog_err("ls", dir, n);
        }
        if n == 0 {
            break;
        }
        if sys::write_all(1, &buf[..n as usize]) < 0 {
            let _ = sys::close(fd as usize);
            return 1;
        }
    }
    let _ = sys::close(fd as usize);
    0
}

fn copy_fd(fd: usize, prog: &str, label: &str) -> Result<(), i32> {
    let mut buf = [0u8; 512];
    loop {
        let n = sys::read(fd, &mut buf);
        if n < 0 {
            return Err(prog_err(prog, label, n));
        }
        if n == 0 {
            return Ok(());
        }
        if sys::write_all(1, &buf[..n as usize]) < 0 {
            return Err(1);
        }
    }
}

pub fn run_cat(args: &[&str]) -> i32 {
    if args.len() <= 1 {
        return match copy_fd(0, "cat", "-") {
            Ok(()) => 0,
            Err(code) => code,
        };
    }
    for &path in &args[1..] {
        if path == "-" {
            if let Err(code) = copy_fd(0, "cat", "-") {
                return code;
            }
            continue;
        }
        let fd = sys::open(path, sys::O_READ);
        if fd < 0 {
            return prog_err("cat", path, fd);
        }
        let res = copy_fd(fd as usize, "cat", path);
        let _ = sys::close(fd as usize);
        if let Err(code) = res {
            return code;
        }
    }
    0
}

pub fn run_mkdir(args: &[&str]) -> i32 {
    if args.len() < 2 {
        return prog_err("mkdir", "", -1);
    }
    for &dir in &args[1..] {
        let r = sys::mkdir(dir);
        if r < 0 {
            return prog_err("mkdir", dir, r);
        }
    }
    0
}

pub fn run_rm(args: &[&str]) -> i32 {
    if args.len() < 2 {
        return prog_err("rm", "", -1);
    }
    for &path in &args[1..] {
        let r = sys::unlink(path);
        if r < 0 {
            return prog_err("rm", path, r);
        }
    }
    0
}

pub fn run_ps() -> i32 {
    let mut buf = [0u8; 2048];
    let n = sys::ps(&mut buf);
    if n < 0 {
        return prog_err("ps", "", n);
    }
    if sys::write_all(1, &buf[..n as usize]) < 0 {
        return 1;
    }
    0
}
