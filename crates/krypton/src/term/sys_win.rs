//! Windows console FFI: raw input mode, key events, console writes and Ctrl-C handling.
//! Every `unsafe` call is a kernel32 function with the documented signature below.

use std::ffi::c_void;
use std::io;

use super::keys::{key_from_win_event, Key};

type Handle = *mut c_void;

const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
const ENABLE_LINE_INPUT: u32 = 0x0002;
const ENABLE_ECHO_INPUT: u32 = 0x0004;
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
const KEY_EVENT: u16 = 0x0001;

/// Layout of the Win32 `INPUT_RECORD` (20 bytes); only the key-event fields are read.
#[repr(C)]
#[derive(Clone, Copy)]
struct InputRecord {
    event_type: u16,
    _pad: u16,
    key_down: i32,
    repeat: u16,
    vkey: u16,
    scan: u16,
    unit: u16,
    control: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetStdHandle(which: u32) -> Handle;
    fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
    fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
    fn ReadConsoleInputW(handle: Handle, buf: *mut InputRecord, len: u32, read: *mut u32) -> i32;
    fn WriteConsoleW(
        handle: Handle,
        buf: *const u16,
        len: u32,
        written: *mut u32,
        reserved: *const c_void,
    ) -> i32;
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

/// Console modes saved before raw mode, plus the handles they belong to.
#[derive(Clone, Copy)]
pub(super) struct Saved {
    input: usize,
    input_mode: u32,
    output: usize,
    output_mode: u32,
}

fn std_handle(which: u32) -> Handle {
    // SAFETY: GetStdHandle takes a plain DWORD and returns a handle value or null/invalid.
    unsafe { GetStdHandle(which) }
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

pub(super) fn is_tty() -> bool {
    let mut mode = 0u32;
    // SAFETY: the handle comes from GetStdHandle and `mode` is a valid out pointer.
    unsafe { GetConsoleMode(std_handle(STD_INPUT_HANDLE), &mut mode) != 0 }
}

/// Switch the console to raw input (no line buffering, echo or Ctrl-C processing) and enable
/// VT escape sequences on output. Returns the previous modes.
pub(super) fn enable_raw() -> io::Result<Saved> {
    let (in_h, out_h) = (std_handle(STD_INPUT_HANDLE), std_handle(STD_OUTPUT_HANDLE));
    let mut in_mode = 0u32;
    let mut out_mode = 0u32;
    // SAFETY: both handles come from GetStdHandle; the mode out-pointers are valid locals.
    unsafe {
        if GetConsoleMode(in_h, &mut in_mode) == 0 || GetConsoleMode(out_h, &mut out_mode) == 0 {
            return Err(last_error());
        }
    }
    let saved = Saved {
        input: in_h as usize,
        input_mode: in_mode,
        output: out_h as usize,
        output_mode: out_mode,
    };
    let raw_in = in_mode & !(ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT);
    // SAFETY: the handle is the console input handle obtained above; the mode is a valid DWORD.
    if unsafe { SetConsoleMode(in_h, raw_in) } == 0 {
        return Err(last_error());
    }
    // SAFETY: the handle is the console output handle obtained above; failure is harmless.
    unsafe { SetConsoleMode(out_h, out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
    Ok(saved)
}

/// Put back the modes saved by [`enable_raw`].
pub(super) fn restore(saved: &Saved) {
    let (in_h, out_h) = (saved.input as Handle, saved.output as Handle);
    // SAFETY: the handles were returned by GetStdHandle and stay valid for the process lifetime.
    unsafe {
        SetConsoleMode(in_h, saved.input_mode);
        SetConsoleMode(out_h, saved.output_mode);
    }
}

/// Block until the next key press and decode it. Empty means input failed (end of input).
pub(super) fn read_keys() -> Vec<Key> {
    let in_h = std_handle(STD_INPUT_HANDLE);
    loop {
        let mut rec = InputRecord {
            event_type: 0,
            _pad: 0,
            key_down: 0,
            repeat: 0,
            vkey: 0,
            scan: 0,
            unit: 0,
            control: 0,
        };
        let mut read = 0u32;
        // SAFETY: `rec` is a valid writable INPUT_RECORD and we ask for exactly one record.
        let ok = unsafe { ReadConsoleInputW(in_h, &mut rec, 1, &mut read) };
        if ok == 0 || read == 0 {
            return Vec::new();
        }
        if rec.event_type == KEY_EVENT && rec.key_down != 0 {
            if let Some(key) = key_from_win_event(rec.vkey, rec.unit) {
                return vec![key];
            }
        }
    }
}

/// Write text to the console as UTF-16, so every character reaches the screen.
pub(super) fn write_text(text: &str) -> io::Result<()> {
    let out_h = std_handle(STD_OUTPUT_HANDLE);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut done = 0u32;
    let len = u32::try_from(wide.len()).unwrap_or(u32::MAX);
    // SAFETY: `wide` is a live buffer of `len` UTF-16 units; `done` is a valid out pointer.
    let ok = unsafe { WriteConsoleW(out_h, wide.as_ptr(), len, &mut done, std::ptr::null()) };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(())
}

extern "system" fn ignore_ctrl(_event: u32) -> i32 {
    // Returning TRUE marks Ctrl-C as handled: the shell survives, and the child process that is
    // running at the time (which has its own default handler) still gets the event.
    1
}

/// Make Ctrl-C and Ctrl-Break interrupt only the running child, never the shell.
pub(super) fn ignore_interrupts() {
    // SAFETY: `ignore_ctrl` is a plain extern "system" function with the handler signature.
    unsafe { SetConsoleCtrlHandler(Some(ignore_ctrl), 1) };
}
