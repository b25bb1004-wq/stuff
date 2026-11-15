//! Graceful shutdown signal registration and polling.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_signal(_sig: std::ffi::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Register SIGTERM and SIGINT handlers on Unix; no-op on other platforms.
#[cfg(unix)]
pub fn install() {
    extern "C" {
        fn signal(sig: std::ffi::c_int, handler: extern "C" fn(std::ffi::c_int)) -> usize;
    }
    const SIGINT: std::ffi::c_int = 2;
    const SIGTERM: std::ffi::c_int = 15;
    // SAFETY: `handle_signal` is an `extern "C"` function whose body only performs an
    // async-signal-safe atomic store to a static `AtomicBool`.
    unsafe {
        signal(SIGINT, handle_signal);
        signal(SIGTERM, handle_signal);
    }
}

/// Register SIGTERM and SIGINT handlers on Unix; no-op on other platforms.
#[cfg(not(unix))]
pub fn install() {}

/// Return whether shutdown has been requested by a signal or `request()`.
pub fn requested() -> bool {
    STOP.load(Ordering::Acquire)
}

/// Mark shutdown as requested.
pub fn request() {
    STOP.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_request_toggle_flag() {
        install();
        request();
        assert!(requested());
    }
}
