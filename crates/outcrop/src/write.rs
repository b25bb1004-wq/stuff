//! Wire serialization for HTTP/1.1 requests and responses.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::types::{Request, Response};

/// Serialize an HTTP/1.x request to wire bytes, adding `Content-Length` if the body is non-empty
/// and no `Content-Length` header is already present.
pub fn write_request(r: &Request) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + r.body.len());
    let mut line = String::new();
    let _ = write!(line, "{} {} HTTP/1.{}\r\n", r.method, r.target, r.minor);
    out.extend_from_slice(line.as_bytes());
    write_headers(
        &mut out,
        &r.headers,
        r.body.len(),
        r.header("content-length").is_none(),
    );
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&r.body);
    out
}

/// Serialize an HTTP/1.x response to wire bytes, adding `Content-Length` if the body is non-empty
/// and no `Content-Length` header is already present.
pub fn write_response(r: &Response) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + r.body.len());
    let mut line = String::new();
    if r.reason.is_empty() {
        let _ = write!(line, "HTTP/1.{} {:03}\r\n", r.minor, r.status);
    } else {
        let _ = write!(line, "HTTP/1.{} {:03} {}\r\n", r.minor, r.status, r.reason);
    }
    out.extend_from_slice(line.as_bytes());
    write_headers(
        &mut out,
        &r.headers,
        r.body.len(),
        r.header("content-length").is_none(),
    );
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&r.body);
    out
}

/// Write header lines and optionally append `Content-Length`.
fn write_headers(
    out: &mut Vec<u8>,
    headers: &[(String, Vec<u8>)],
    body_len: usize,
    need_content_length: bool,
) {
    for (k, v) in headers {
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v);
        out.extend_from_slice(b"\r\n");
    }
    if body_len > 0 && need_content_length {
        let mut cl = String::new();
        let _ = write!(cl, "Content-Length: {}\r\n", body_len);
        out.extend_from_slice(cl.as_bytes());
    }
}
