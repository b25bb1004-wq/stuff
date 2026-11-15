//! Wire serialization for HTTP/1.1 requests and responses.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::head::{is_field_value_byte, is_tchar, is_vchar};
use crate::types::{Request, Response};

/// Serialize an HTTP/1.x request to wire bytes, adding `Content-Length` if the body is non-empty
/// and no `Content-Length` header is already present.
pub fn write_request(r: &Request) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + r.body.len());
    push_filtered(&mut out, r.method.as_bytes(), is_tchar);
    out.push(b' ');
    push_filtered(&mut out, r.target.as_bytes(), is_vchar);
    let mut ver = String::new();
    let _ = write!(ver, " HTTP/1.{}\r\n", r.minor);
    out.extend_from_slice(ver.as_bytes());
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
    let _ = write!(line, "HTTP/1.{} {:03}", r.minor, r.status);
    out.extend_from_slice(line.as_bytes());
    if r.reason.bytes().any(is_field_value_byte) {
        out.push(b' ');
        push_filtered(&mut out, r.reason.as_bytes(), is_field_value_byte);
    }
    out.extend_from_slice(b"\r\n");
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

/// Write header lines and optionally append `Content-Length`, rejecting invalid names and
/// stripping CR/LF/control bytes from values to prevent header injection.
fn write_headers(
    out: &mut Vec<u8>,
    headers: &[(String, Vec<u8>)],
    body_len: usize,
    need_content_length: bool,
) {
    for (k, v) in headers {
        if k.is_empty() || !k.bytes().all(is_tchar) {
            continue;
        }
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        push_filtered(out, v, is_field_value_byte);
        out.extend_from_slice(b"\r\n");
    }
    if body_len > 0 && need_content_length {
        let mut cl = String::new();
        let _ = write!(cl, "Content-Length: {}\r\n", body_len);
        out.extend_from_slice(cl.as_bytes());
    }
}

/// Append only bytes satisfying `pred` to `out`.
fn push_filtered(out: &mut Vec<u8>, bytes: &[u8], pred: fn(u8) -> bool) {
    for &b in bytes {
        if pred(b) {
            out.push(b);
        }
    }
}
