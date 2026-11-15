//! Per-connection HTTP/1.1 loop: keep-alive, pipelining, timeouts, and error replies.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::vec;
use std::vec::Vec;

use super::router::Dispatch;
use super::{lock, ActiveConn, ServerInner};
use crate::ws::WebSocket;
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

                let dispatched =
                    match catch_unwind(AssertUnwindSafe(|| inner.router.dispatch(&req))) {
                        Ok(d) => d,
                        Err(_) => {
                            let resp = error_response(500, "Internal Server Error", true);
                            let wire = write_response(&resp);
                            let _ = stream.write_all(&wire);
                            let _ = stream.flush();
                            break;
                        }
                    };

                match dispatched {
                    Dispatch::Response(mut resp) => {
                        if req.method == "HEAD" {
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

                        if !write_ok || !keep_alive || inner.stopping.load(SeqCst) {
                            break;
                        }
                    }
                    Dispatch::WebSocket(handler, accept) => {
                        let resp = Response {
                            minor: 1,
                            status: 101,
                            reason: "Switching Protocols".to_string(),
                            headers: vec![
                                ("Upgrade".to_string(), b"websocket".to_vec()),
                                ("Connection".to_string(), b"Upgrade".to_vec()),
                                ("Sec-WebSocket-Accept".to_string(), accept.into_bytes()),
                            ],
                            body: Vec::new(),
                        };
                        let wire = write_response(&resp);
                        if stream.write_all(&wire).is_ok() && stream.flush().is_ok() {
                            let leftover = core::mem::take(&mut buf);
                            let ws = WebSocket::new(stream, leftover);
                            let _ = catch_unwind(AssertUnwindSafe(|| handler(ws)));
                        }
                        break;
                    }
                    Dispatch::Stream(handler) => {
                        let is_http11 = req.minor >= 1;
                        let is_head = req.method == "HEAD";
                        let close_after = !is_http11 || !keep_alive;
                        let mut head_sent = false;
                        let mut write_err = false;

                        let res = catch_unwind(AssertUnwindSafe(|| {
                            let mut emit = |piece: &[u8]| -> std::io::Result<()> {
                                if piece.is_empty() || is_head {
                                    return Ok(());
                                }
                                if !head_sent {
                                    write_stream_head(
                                        &mut stream,
                                        req.minor,
                                        200,
                                        is_http11,
                                        close_after,
                                    )
                                    .inspect_err(|_| write_err = true)?;
                                    head_sent = true;
                                }
                                if is_http11 {
                                    let hdr = std::format!("{:x}\r\n", piece.len());
                                    stream
                                        .write_all(hdr.as_bytes())
                                        .inspect_err(|_| write_err = true)?;
                                    stream.write_all(piece).inspect_err(|_| write_err = true)?;
                                    stream
                                        .write_all(b"\r\n")
                                        .inspect_err(|_| write_err = true)?;
                                } else {
                                    stream.write_all(piece).inspect_err(|_| write_err = true)?;
                                }
                                stream.flush().inspect_err(|_| write_err = true)?;
                                Ok(())
                            };
                            handler(&req, &mut emit)
                        }));

                        let Ok(status) = res else {
                            let _ = stream.shutdown(Shutdown::Both);
                            break;
                        };
                        if write_err {
                            break;
                        }

                        let no_body = is_head
                            || (100..=199).contains(&status)
                            || status == 204
                            || status == 304;
                        if !head_sent {
                            let chunked = is_http11 && !no_body;
                            if write_stream_head(
                                &mut stream,
                                req.minor,
                                status,
                                chunked,
                                close_after,
                            )
                            .is_err()
                            {
                                break;
                            }
                        }
                        if is_http11
                            && !no_body
                            && (stream.write_all(b"0\r\n\r\n").is_err() || stream.flush().is_err())
                        {
                            break;
                        }
                        in_flight.store(false, SeqCst);
                        last_activity = Instant::now();

                        if close_after || inner.stopping.load(SeqCst) {
                            break;
                        }
                    }
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

fn write_stream_head(
    stream: &mut TcpStream,
    minor: u8,
    status: u16,
    chunked: bool,
    close: bool,
) -> std::io::Result<()> {
    let mut headers = vec![(
        "Content-Type".to_string(),
        b"application/octet-stream".to_vec(),
    )];
    if chunked {
        headers.push(("Transfer-Encoding".to_string(), b"chunked".to_vec()));
    }
    if close {
        headers.push(("Connection".to_string(), b"close".to_vec()));
    }
    let resp = Response {
        minor,
        status,
        reason: reason_for(status).to_string(),
        headers,
        body: Vec::new(),
    };
    let wire = write_response(&resp);
    stream.write_all(&wire)?;
    stream.flush()
}

fn reason_for(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        426 => "Upgrade Required",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
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
