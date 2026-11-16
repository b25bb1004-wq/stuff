//! Terminal access for the calliope editor: raw mode, the alternate screen, input events, the
//! window size and frame output. Windows uses kernel32 console calls; Linux (glibc) uses termios.
//! Every `unsafe` block states why it is sound. Other Unix targets report that raw mode is
//! unavailable instead of guessing at a termios layout.

use std::io;
use std::sync::Mutex;

use calliope::Ev;

/// Restores the terminal when dropped (normal exit and early return).
pub struct Terminal;

impl Drop for Terminal {
    fn drop(&mut self) {
        leave();
    }
}

/// Saved terminal settings, kept so a panic can still restore the terminal (panic=abort skips
/// destructors, and the panic hook runs first).
static SAVED: Mutex<Option<sys::Saved>> = Mutex::new(None);

const ENTER: &str = "\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const LEAVE: &str = "\x1b[0m\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[?1049l";

/// True when stdin is a console or terminal.
pub fn is_terminal() -> bool {
    sys::is_tty()
}

/// Switch to raw input and the alternate screen. Fails when the terminal cannot be prepared.
pub fn enter() -> io::Result<Terminal> {
    let saved = sys::enable_raw()?;
    if let Ok(mut slot) = SAVED.lock() {
        *slot = Some(saved);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        previous(info);
    }));
    write(ENTER)?;
    Ok(Terminal)
}

/// Leave the alternate screen and put back the saved settings. Safe to call more than once.
pub fn leave() {
    let _ = write(LEAVE);
    let saved = SAVED.lock().ok().and_then(|mut slot| slot.take());
    if let Some(saved) = saved {
        sys::restore(&saved);
    }
}

/// Write text (escape sequences included) to the terminal.
pub fn write(text: &str) -> io::Result<()> {
    sys::write_text(text)
}

/// Terminal size as `(columns, rows)`, with 80 x 24 when it cannot be read.
pub fn size() -> (usize, usize) {
    sys::size().unwrap_or((80, 24))
}

/// Block until input arrives. `None` means the input ended. `Some` may be empty (a resize or a
/// key release); the caller then redraws and asks again.
pub fn read_events() -> Option<Vec<Ev>> {
    sys::read_events()
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::io;
    use std::sync::atomic::{AtomicU32, Ordering};

    use calliope::input::{key_from_windows, mouse_from_windows};
    use calliope::Ev;

    type Handle = *mut c_void;

    const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    const ENABLE_LINE_INPUT: u32 = 0x0002;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_WINDOW_INPUT: u32 = 0x0008;
    const ENABLE_MOUSE_INPUT: u32 = 0x0010;
    const ENABLE_QUICK_EDIT_MODE: u32 = 0x0040;
    const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const KEY_EVENT: u16 = 0x0001;
    const MOUSE_EVENT: u16 = 0x0002;

    /// `INPUT_RECORD`: a type word, then the 16-byte event payload read as four DWORDs.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct InputRecord {
        event_type: u16,
        _pad: u16,
        body: [u32; 4],
    }

    /// `CONSOLE_SCREEN_BUFFER_INFO` (22 bytes); only the visible window is read.
    #[repr(C)]
    #[derive(Default)]
    struct ScreenInfo {
        size: [i16; 2],
        cursor: [i16; 2],
        attrs: u16,
        window: [i16; 4],
        max: [i16; 2],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> Handle;
        fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
        fn ReadConsoleInputW(
            handle: Handle,
            buf: *mut InputRecord,
            len: u32,
            read: *mut u32,
        ) -> i32;
        fn WriteConsoleW(
            handle: Handle,
            buf: *const u16,
            len: u32,
            written: *mut u32,
            reserved: *const c_void,
        ) -> i32;
        fn GetConsoleScreenBufferInfo(handle: Handle, info: *mut ScreenInfo) -> i32;
    }

    /// Console modes before raw mode, with the handles they belong to.
    #[derive(Clone, Copy)]
    pub(super) struct Saved {
        input: usize,
        input_mode: u32,
        output: usize,
        output_mode: u32,
    }

    /// Last button state, so presses and releases are told apart from plain movement.
    static BUTTONS: AtomicU32 = AtomicU32::new(0);

    fn std_handle(which: u32) -> Handle {
        // SAFETY: GetStdHandle takes a plain DWORD and returns a handle value (or null/invalid).
        unsafe { GetStdHandle(which) }
    }

    pub(super) fn is_tty() -> bool {
        let mut mode = 0u32;
        // SAFETY: the handle comes from GetStdHandle and `mode` is a valid out pointer.
        unsafe { GetConsoleMode(std_handle(STD_INPUT_HANDLE), &mut mode) != 0 }
    }

    pub(super) fn enable_raw() -> io::Result<Saved> {
        let (in_h, out_h) = (std_handle(STD_INPUT_HANDLE), std_handle(STD_OUTPUT_HANDLE));
        let mut in_mode = 0u32;
        let mut out_mode = 0u32;
        // SAFETY: both handles come from GetStdHandle; the mode out-pointers are valid locals.
        let ok = unsafe {
            GetConsoleMode(in_h, &mut in_mode) != 0 && GetConsoleMode(out_h, &mut out_mode) != 0
        };
        if !ok {
            return Err(io::Error::last_os_error());
        }
        let saved = Saved {
            input: in_h as usize,
            input_mode: in_mode,
            output: out_h as usize,
            output_mode: out_mode,
        };
        let raw_in = (in_mode
            & !(ENABLE_PROCESSED_INPUT
                | ENABLE_LINE_INPUT
                | ENABLE_ECHO_INPUT
                | ENABLE_QUICK_EDIT_MODE))
            | ENABLE_WINDOW_INPUT
            | ENABLE_MOUSE_INPUT
            | ENABLE_EXTENDED_FLAGS;
        // SAFETY: `in_h` is the console input handle from above; `raw_in` is a valid DWORD.
        if unsafe { SetConsoleMode(in_h, raw_in) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `out_h` is the console output handle from above; failure is harmless here.
        unsafe { SetConsoleMode(out_h, out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
        Ok(saved)
    }

    pub(super) fn restore(saved: &Saved) {
        let (in_h, out_h) = (saved.input as Handle, saved.output as Handle);
        // SAFETY: the handles came from GetStdHandle and stay valid for the process lifetime.
        unsafe {
            SetConsoleMode(in_h, saved.input_mode);
            SetConsoleMode(out_h, saved.output_mode);
        }
    }

    pub(super) fn read_events() -> Option<Vec<Ev>> {
        let mut rec = InputRecord {
            event_type: 0,
            _pad: 0,
            body: [0; 4],
        };
        let mut read = 0u32;
        // SAFETY: `rec` is a valid writable INPUT_RECORD and exactly one record is requested.
        let ok = unsafe { ReadConsoleInputW(std_handle(STD_INPUT_HANDLE), &mut rec, 1, &mut read) };
        if ok == 0 || read == 0 {
            return None;
        }
        let b = rec.body;
        let events = match rec.event_type {
            // KEY_EVENT_RECORD: bKeyDown, wRepeatCount|wVirtualKeyCode, wVirtualScanCode|uChar,
            // dwControlKeyState.
            KEY_EVENT if b[0] != 0 => {
                let vkey = (b[1] >> 16) as u16;
                let unit = (b[2] >> 16) as u16;
                key_from_windows(vkey, unit, b[3]).into_iter().collect()
            }
            // MOUSE_EVENT_RECORD: dwMousePosition, dwButtonState, dwControlKeyState, dwEventFlags.
            MOUSE_EVENT => {
                let x = (b[0] & 0xffff) as u16 as i16;
                let y = (b[0] >> 16) as u16 as i16;
                let prev = BUTTONS.swap(b[1], Ordering::Relaxed);
                mouse_from_windows(x, y, b[1], b[3], prev)
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        };
        Some(events)
    }

    pub(super) fn size() -> Option<(usize, usize)> {
        let mut info = ScreenInfo::default();
        // SAFETY: `info` is a valid writable CONSOLE_SCREEN_BUFFER_INFO-sized struct.
        if unsafe { GetConsoleScreenBufferInfo(std_handle(STD_OUTPUT_HANDLE), &mut info) } == 0 {
            return None;
        }
        let w = i32::from(info.window[2]) - i32::from(info.window[0]) + 1;
        let h = i32::from(info.window[3]) - i32::from(info.window[1]) + 1;
        Some((usize::try_from(w).ok()?, usize::try_from(h).ok()?))
    }

    pub(super) fn write_text(text: &str) -> io::Result<()> {
        let out_h = std_handle(STD_OUTPUT_HANDLE);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut done = 0u32;
        let len = u32::try_from(wide.len()).unwrap_or(u32::MAX);
        // SAFETY: `wide` is a live buffer of `len` UTF-16 units; `done` is a valid out pointer.
        let ok = unsafe { WriteConsoleW(out_h, wide.as_ptr(), len, &mut done, std::ptr::null()) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(unix, not(all(target_os = "linux", target_env = "gnu"))))]
mod sys {
    use std::io;

    use calliope::Ev;

    #[derive(Clone, Copy)]
    pub(super) struct Saved;

    pub(super) fn is_tty() -> bool {
        false
    }

    pub(super) fn enable_raw() -> io::Result<Saved> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the terminal editor needs Linux or Windows",
        ))
    }

    pub(super) fn restore(_saved: &Saved) {}

    pub(super) fn read_events() -> Option<Vec<Ev>> {
        None
    }

    pub(super) fn size() -> Option<(usize, usize)> {
        None
    }

    pub(super) fn write_text(text: &str) -> io::Result<()> {
        use std::io::Write;
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())?;
        out.flush()
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod sys {
    use std::io::{self, Write};

    use calliope::input::decode_bytes;
    use calliope::Ev;

    /// glibc `struct termios` for Linux (NCCS = 32).
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct Saved {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        c_line: u8,
        c_cc: [u8; 32],
        c_ispeed: u32,
        c_ospeed: u32,
    }

    #[repr(C)]
    struct Winsize {
        rows: u16,
        cols: u16,
        xpixel: u16,
        ypixel: u16,
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
    const TIOCGWINSZ: u64 = 0x5413;
    const STDIN: i32 = 0;
    const STDOUT: i32 = 1;

    extern "C" {
        fn isatty(fd: i32) -> i32;
        fn tcgetattr(fd: i32, t: *mut Saved) -> i32;
        fn tcsetattr(fd: i32, action: i32, t: *const Saved) -> i32;
        fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }

    pub(super) fn is_tty() -> bool {
        // SAFETY: isatty only inspects the descriptor number it is given.
        unsafe { isatty(STDIN) == 1 }
    }

    pub(super) fn enable_raw() -> io::Result<Saved> {
        let mut saved = std::mem::MaybeUninit::<Saved>::uninit();
        // SAFETY: tcgetattr writes a full termios into the valid out pointer when it returns 0.
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
        // SAFETY: `raw` is a valid, initialized termios built from the kernel's own settings.
        if unsafe { tcsetattr(STDIN, TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(saved)
    }

    pub(super) fn restore(saved: &Saved) {
        // SAFETY: `saved` holds settings previously returned by tcgetattr on this descriptor.
        unsafe { tcsetattr(STDIN, TCSANOW, saved) };
    }

    pub(super) fn read_events() -> Option<Vec<Ev>> {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is a valid writable buffer of exactly the length passed.
        let n = unsafe { read(STDIN, buf.as_mut_ptr(), buf.len()) };
        match usize::try_from(n) {
            Ok(0) => None,
            Ok(n) => Some(decode_bytes(&buf[..n])),
            Err(_) => {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    Some(Vec::new())
                } else {
                    None
                }
            }
        }
    }

    pub(super) fn size() -> Option<(usize, usize)> {
        let mut ws = Winsize {
            rows: 0,
            cols: 0,
            xpixel: 0,
            ypixel: 0,
        };
        // SAFETY: TIOCGWINSZ writes one struct winsize (four u16) through the pointer it gets.
        if unsafe { ioctl(STDOUT, TIOCGWINSZ, &mut ws as *mut Winsize) } != 0 {
            return None;
        }
        Some((usize::from(ws.cols), usize::from(ws.rows)))
    }

    pub(super) fn write_text(text: &str) -> io::Result<()> {
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())?;
        out.flush()
    }
}
