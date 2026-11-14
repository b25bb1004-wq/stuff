//! outcrop: zero-copy-friendly HTTP/1.1 parser and writer with no I/O inside.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod body;
mod head;
#[cfg(feature = "std")]
pub mod server;
mod types;
mod write;
pub mod ws;

use alloc::vec::Vec;

pub use types::{Error, Limits, Request, Response};
pub use write::{write_request, write_response};

/// Parse one request from the front of `buf`. `Ok((request, bytes_used))`; `Err(Incomplete)` when
/// more bytes are needed; any other `Err` is final for this connection.
pub fn parse_request(buf: &[u8], lim: &Limits) -> Result<(Request, usize), Error> {
    let head = head::parse_request_head(buf, lim)?;
    let (body, used) = if body::is_chunked(&head.headers) {
        body::parse_chunked_body(buf, head.head_end, lim)?
    } else if let Some(len) = body::parse_content_length(&head.headers)? {
        body::parse_fixed_body(buf, head.head_end, len, lim)?
    } else {
        (Vec::new(), head.head_end)
    };
    Ok((
        Request {
            method: head.method,
            target: head.target,
            minor: head.minor,
            headers: head.headers,
            body,
        },
        used,
    ))
}

/// Same for a response. `head_only` is true for replies to HEAD and for 1xx/204/304 (no body).
pub fn parse_response(
    buf: &[u8],
    lim: &Limits,
    head_only: bool,
) -> Result<(Response, usize), Error> {
    let head = head::parse_response_head(buf, lim)?;
    let no_body =
        head_only || (100..=199).contains(&head.status) || head.status == 204 || head.status == 304;
    let (body, used) = if no_body {
        (Vec::new(), head.head_end)
    } else if body::is_chunked(&head.headers) {
        body::parse_chunked_body(buf, head.head_end, lim)?
    } else if let Some(len) = body::parse_content_length(&head.headers)? {
        body::parse_fixed_body(buf, head.head_end, len, lim)?
    } else {
        return Err(Error::Incomplete);
    };
    Ok((
        Response {
            minor: head.minor,
            status: head.status,
            reason: head.reason,
            headers: head.headers,
            body,
        },
        used,
    ))
}
