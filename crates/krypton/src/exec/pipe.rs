//! In-memory bounded byte pipe for concurrent pipeline stages.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex};

struct PipeInner {
    buf: VecDeque<u8>,
    cap: usize,
    closed_read: bool,
    closed_write: bool,
}

struct SharedPipe {
    inner: Mutex<PipeInner>,
    cvar: Condvar,
}

/// Reading end of an in-memory pipe.
pub(crate) struct MemReader {
    shared: Arc<SharedPipe>,
}

/// Writing end of an in-memory pipe.
pub(crate) struct MemWriter {
    shared: Arc<SharedPipe>,
}

/// Create a bounded in-memory pipe with capacity `cap` bytes.
pub(crate) fn mem_pipe(cap: usize) -> (MemReader, MemWriter) {
    let shared = Arc::new(SharedPipe {
        inner: Mutex::new(PipeInner {
            buf: VecDeque::new(),
            cap: cap.max(1024),
            closed_read: false,
            closed_write: false,
        }),
        cvar: Condvar::new(),
    });
    (
        MemReader {
            shared: Arc::clone(&shared),
        },
        MemWriter { shared },
    )
}

impl Read for MemReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut guard = self
            .shared
            .inner
            .lock()
            .map_err(|_| io::Error::other("pipe mutex poisoned"))?;
        while guard.buf.is_empty() && !guard.closed_write {
            guard = self
                .shared
                .cvar
                .wait(guard)
                .map_err(|_| io::Error::other("pipe mutex poisoned"))?;
        }
        if guard.buf.is_empty() {
            return Ok(0);
        }
        let n = out.len().min(guard.buf.len());
        for slot in out.iter_mut().take(n) {
            if let Some(b) = guard.buf.pop_front() {
                *slot = b;
            }
        }
        self.shared.cvar.notify_all();
        Ok(n)
    }
}

impl Drop for MemReader {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.shared.inner.lock() {
            guard.closed_read = true;
            self.shared.cvar.notify_all();
        }
    }
}

impl Write for MemWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let mut guard = self
            .shared
            .inner
            .lock()
            .map_err(|_| io::Error::other("pipe mutex poisoned"))?;
        while guard.buf.len() >= guard.cap && !guard.closed_read {
            guard = self
                .shared
                .cvar
                .wait(guard)
                .map_err(|_| io::Error::other("pipe mutex poisoned"))?;
        }
        if guard.closed_read {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"));
        }
        let avail = guard.cap.saturating_sub(guard.buf.len()).max(1);
        let n = data.len().min(avail);
        if let Some(slice) = data.get(..n) {
            guard.buf.extend(slice);
        }
        self.shared.cvar.notify_all();
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for MemWriter {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.shared.inner.lock() {
            guard.closed_write = true;
            self.shared.cvar.notify_all();
        }
    }
}
