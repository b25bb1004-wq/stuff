//! Per-connection HTTP/1.1 loop: keep-alive, pipelining, timeouts, and error replies.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::vec;
use std::vec::Vec;

use super::{lock, ActiveConn, ServerInner};
use crate::{parse_request, write_response, Error, Response};

/// Guard that registers an open connection and decrements `open` on drop.
struct OpenGuard {
    id: u64,
    inner: Arc<ServerInner>,
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        lock(&self.inner.active).remove(&self.id);
        self.inner.open.fetch_sub(1, SeqCst);
    }
}

/// Serve one accepted TCP connection until closed, timed out, or `keep_alive_max` reached.
pub(crate) fn handle_connection(mut stream: TcpStream, inner: Arc<ServerInner>) {
    let conn_id = inner.next_conn_id.fetch_add(1, SeqCst);
    let in_flight = Arc::new(AtomicBool::new(false));
    if let Ok(cloned) = stream.try_clone() {
        lock(&inner.active).insert(
            conn_id,
            ActiveConn {
                stream: cloned,
                in_flight: Arc::clone(&in_flight),
            },
        );
    }
    let _guard = OpenGuard {
        id: conn_id,
        inner: Arc::clone(&inner),
    };

    let slice = inner.read_timeout.min(Duration::from_millis(50));
    let _ = stream.set_read_timeout(Some(slice));
    let _ = stream.set_write_timeout(Some(inner.read_timeout));
    let _ = stream.set_nodelay(true);

    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 8192];
    let mut req_count: u32 = 0;
    let mut last_activity = Instant::now();

    loop {
        if inner.stopping.load(SeqCst) {
            break;
        }

        match parse_request(&buf, &inner.limits) {
            Ok((req, used)) => {
                buf.drain(..used);
                req_count = req_count.saturating_add(1);
                inner.requests.fetch_add(1, SeqCst);

                in_flight.store(true, SeqCst);
                let keep_alive = req.keep_alive()
                    && req_count < inner.keep_alive_max
                    && !inner.stopping.load(SeqCst);

                let result = catch_unwind(AssertUnwindSafe(|| inner.router.dispatch(&req)));
                let mut panicked = false;
                let mut resp = match result {
                    Ok(r) => r,
                    Err(_) => {
                        panicked = true;
                        error_response(500, "Internal Server Error", true)
                    }
                };

                if req.method == "HEAD" && !panicked {
                    if resp.header("content-length").is_none() && !resp.body.is_empty() {
                        let len = resp.body.len().to_string();
                        resp.headers
                            .push(("Content-Length".to_string(), len.into_bytes()));
                    }
                    resp.body.clear();
                }

                if !keep_alive && resp.header("connection").is_none() {
                    resp.headers
                        .push(("Connection".to_string(), b"close".to_vec()));
                }
                if resp.header("content-length").is_none() && resp.body.is_empty() {
                    resp.headers
                        .push(("Content-Length".to_string(), b"0".to_vec()));
                }

                let wire = write_response(&resp);
                let write_ok = stream.write_all(&wire).is_ok() && stream.flush().is_ok();
                in_flight.store(false, SeqCst);
                last_activity = Instant::now();

                if panicked || !write_ok || !keep_alive || inner.stopping.load(SeqCst) {
                    break;
                }
            }
            Err(Error::Incomplete) => {
                if inner.stopping.load(SeqCst) {
                    break;
                }
                match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        last_activity = Instant::now();
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                        if inner.stopping.load(SeqCst)
                            || last_activity.elapsed() >= inner.read_timeout
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            Err(err) => {
                let (status, reason) = match err {
                    Error::HeadTooLarge | Error::BodyTooLarge => (413, "Payload Too Large"),
                    Error::TooManyHeaders => (431, "Request Header Fields Too Large"),
                    _ => (400, "Bad Request"),
                };
                let resp = error_response(status, reason, true);
                let wire = write_response(&resp);
                let _ = stream.write_all(&wire);
                let _ = stream.flush();
                break;
            }
        }
    }
}

/// Write a `503 Service Unavailable` response and close the rejected socket.
pub(crate) fn reject_503(mut stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(50)));
    let resp = error_response(503, "Service Unavailable", true);
    let wire = write_response(&resp);
    let _ = stream.write_all(&wire);
    let _ = stream.flush();
    // Closing with the request still unread makes Windows send RST, so the client never sees
    // the 503. Send FIN, then drain briefly (bounded: 20 ms, 16 KiB) so the accept loop stays fast.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(20)));
    let mut buf = [0u8; 4096];
    for _ in 0..4 {
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => {}
            _ => break,
        }
    }
}

fn error_response(status: u16, reason: &str, close: bool) -> Response {
    let mut headers = vec![("Content-Length".to_string(), b"0".to_vec())];
    if close {
        headers.push(("Connection".to_string(), b"close".to_vec()));
    }
    Response {
        minor: 1,
        status,
        reason: reason.to_string(),
        headers,
        body: Vec::new(),
    }
}
