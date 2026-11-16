//! Terminal access for the interactive shell: raw mode (restored on exit and on panic), key
//! input and text output. The only `unsafe` code in krypton lives in the two `sys_*` modules.

use std::collections::VecDeque;
use std::io;
use std::sync::Mutex;

pub(crate) mod keys;
use keys::Key;

#[cfg(windows)]
#[path = "sys_win.rs"]
#[allow(unsafe_code)]
mod sys;

#[cfg(not(windows))]
#[path = "sys_unix.rs"]
#[allow(unsafe_code)]
mod sys;

/// Console state saved before raw mode, so a panic can still restore the terminal.
static SAVED: Mutex<Option<sys::Saved>> = Mutex::new(None);

fn restore_saved() {
    if let Ok(mut slot) = SAVED.try_lock() {
        if let Some(saved) = slot.take() {
            sys::restore(&saved);
        }
    }
}

/// Raw mode is active while this guard lives; dropping it restores the saved console modes.
pub(crate) struct RawMode;

impl RawMode {
    /// Enable raw input. Fails when stdin is not a console or terminal.
    pub(crate) fn enable() -> io::Result<RawMode> {
        let saved = sys::enable_raw()?;
        if let Ok(mut slot) = SAVED.lock() {
            *slot = Some(saved);
        }
        Ok(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        restore_saved();
    }
}

/// Restore the terminal before a panic message is printed (panic=abort skips destructors).
pub(crate) fn restore_on_panic() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_saved();
        previous(info);
    }));
}

/// True when stdin is a console or terminal.
pub(crate) fn is_tty() -> bool {
    sys::is_tty()
}

/// Make Ctrl-C interrupt only the running child program, never the shell itself.
pub(crate) fn ignore_interrupts() {
    sys::ignore_interrupts();
}

/// Keys read from the terminal, with the keys of one read queued for later calls.
#[derive(Default)]
pub(crate) struct Input {
    pending: VecDeque<Key>,
}

impl Input {
    /// Block until the next key. Returns `Key::Eof` when input ends.
    pub(crate) fn next_key(&mut self) -> Key {
        loop {
            if let Some(key) = self.pending.pop_front() {
                return key;
            }
            let keys = sys::read_keys();
            if keys.is_empty() {
                return Key::Eof;
            }
            self.pending.extend(keys);
        }
    }
}

/// Write text that is already prepared for the terminal (may contain VT escape sequences).
pub(crate) fn write_text(text: &str) -> io::Result<()> {
    #[cfg(windows)]
    {
        sys::write_text(text)
    }
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let mut out = io::stdout();
        out.write_all(text.as_bytes())?;
        out.flush()
    }
}
