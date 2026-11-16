//! Unix terminal FFI through termios (glibc Linux layout). Other Unix targets report no
//! raw mode, so the shell falls back to plain line input there.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod imp {
    use crate::term::keys::{decode_bytes, Key};
    use std::io;

    /// glibc `struct termios` for Linux (NCCS = 32).
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(in crate::term) struct Termios {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        c_line: u8,
        c_cc: [u8; 32],
        c_ispeed: u32,
        c_ospeed: u32,
    }

    const ICRNL: u32 = 0o400;
    const IXON: u32 = 0o2000;
    const ISIG: u32 = 0o1;
    const ICANON: u32 = 0o2;
    const ECHO: u32 = 0o10;
    const IEXTEN: u32 = 0o100000;
    const VTIME: usize = 5;
    const VMIN: usize = 6;
    const TCSANOW: i32 = 0;
    const SIGINT: i32 = 2;
    const STDIN: i32 = 0;

    extern "C" {
        fn isatty(fd: i32) -> i32;
        fn tcgetattr(fd: i32, t: *mut Termios) -> i32;
        fn tcsetattr(fd: i32, action: i32, t: *const Termios) -> i32;
        fn signal(sig: i32, handler: usize) -> usize;
        fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    }

    pub(in crate::term) fn is_tty() -> bool {
        // SAFETY: isatty only inspects the descriptor number it is given.
        unsafe { isatty(STDIN) == 1 }
    }

    pub(in crate::term) fn enable_raw() -> io::Result<Termios> {
        let mut saved = std::mem::MaybeUninit::<Termios>::uninit();
        // SAFETY: tcgetattr writes a full Termios into the valid out pointer when it returns 0.
        if unsafe { tcgetattr(STDIN, saved.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr returned 0, so `saved` is fully initialized.
        let saved = unsafe { saved.assume_init() };
        let mut raw = saved;
        raw.c_iflag &= !(ICRNL | IXON);
        raw.c_lflag &= !(ICANON | ECHO | ISIG | IEXTEN);
        raw.c_cc[VMIN] = 1;
        raw.c_cc[VTIME] = 0;
        // SAFETY: `raw` is a valid, initialized Termios built from the kernel's own settings.
        if unsafe { tcsetattr(STDIN, TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(saved)
    }

    pub(in crate::term) fn restore(saved: &Termios) {
        // SAFETY: `saved` holds settings previously returned by tcgetattr on this descriptor.
        unsafe { tcsetattr(STDIN, TCSANOW, saved) };
    }

    /// Block for input and decode everything it delivered; empty means end of input.
    pub(in crate::term) fn read_keys() -> Vec<Key> {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is a valid writable buffer of exactly the length passed.
        let n = unsafe { read(STDIN, buf.as_mut_ptr(), buf.len()) };
        match usize::try_from(n) {
            Ok(n) if n > 0 => decode_bytes(&buf[..n]),
            _ => Vec::new(),
        }
    }

    pub(in crate::term) fn ignore_interrupts() {
        extern "C" fn on_sigint(_sig: i32) {}
        // SAFETY: the handler does nothing and is async-signal-safe. exec resets handled
        // signals to default in child processes, so only the shell ignores Ctrl-C.
        unsafe { signal(SIGINT, on_sigint as usize) };
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
mod imp {
    use crate::term::keys::Key;
    use std::io;

    #[derive(Clone, Copy)]
    pub(in crate::term) struct Termios;

    pub(in crate::term) fn is_tty() -> bool {
        false
    }
    pub(in crate::term) fn enable_raw() -> io::Result<Termios> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "raw mode not built for this Unix",
        ))
    }
    pub(in crate::term) fn restore(_saved: &Termios) {}
    pub(in crate::term) fn read_keys() -> Vec<Key> {
        Vec::new()
    }
    pub(in crate::term) fn ignore_interrupts() {}
}

pub(super) use imp::{enable_raw, ignore_interrupts, is_tty, read_keys, restore, Termios as Saved};
