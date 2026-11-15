//! Core HTTP/1.1 types, limits, and errors.

use alloc::string::String;
use alloc::vec::Vec;

/// Resource limits enforced before allocating while parsing HTTP/1.1 messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum allowed size of the message head (start-line + headers + blank line) in bytes.
    pub max_head: usize,
    /// Maximum allowed number of header fields.
    pub max_headers: usize,
    /// Maximum allowed body size in bytes.
    pub max_body: usize,
}

impl Default for Limits {
    /// Default limits: 16 KiB head, 100 headers, 8 MiB body.
    fn default() -> Self {
        Self {
            max_head: 16 * 1024,
            max_headers: 100,
            max_body: 8 * 1024 * 1024,
        }
    }
}

/// Errors returned by [`crate::parse_request`] and [`crate::parse_response`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// More bytes are needed to complete the message.
    Incomplete,
    /// The request line is malformed.
    BadRequestLine,
    /// The status line is malformed.
    BadStatusLine,
    /// The HTTP version is not HTTP/1.0 or HTTP/1.1.
    BadVersion,
    /// A header line is malformed or uses obsolete line folding.
    BadHeader,
    /// The message head exceeded [`Limits::max_head`].
    HeadTooLarge,
    /// The number of headers exceeded [`Limits::max_headers`].
    TooManyHeaders,
    /// The body exceeded [`Limits::max_body`].
    BodyTooLarge,
    /// A chunked body is malformed or its chunk size overflowed.
    BadChunk,
    /// A `Content-Length` header is malformed or conflicts with another.
    BadContentLength,
    /// The message has both `Transfer-Encoding: chunked` and `Content-Length` (request smuggling risk).
    AmbiguousFraming,
    /// The request method is not a valid HTTP token.
    BadMethod,
}

/// A parsed HTTP/1.1 or HTTP/1.0 request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// HTTP method token (e.g., `"GET"`, `"POST"`).
    pub method: String,
    /// Request target (e.g., `"/index.html"`).
    pub target: String,
    /// Minor version (`0` for `HTTP/1.0`, `1` for `HTTP/1.1`).
    pub minor: u8,
    /// Header fields in wire order, with names as received and trimmed values.
    pub headers: Vec<(String, Vec<u8>)>,
    /// Decoded message body.
    pub body: Vec<u8>,
}

/// A parsed HTTP/1.1 or HTTP/1.0 response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Minor version (`0` for `HTTP/1.0`, `1` for `HTTP/1.1`).
    pub minor: u8,
    /// Three-digit status code (e.g., `200`).
    pub status: u16,
    /// Reason phrase (may be empty).
    pub reason: String,
    /// Header fields in wire order, with names as received and trimmed values.
    pub headers: Vec<(String, Vec<u8>)>,
    /// Decoded message body.
    pub body: Vec<u8>,
}

impl Request {
    /// Return the value of the first header matching `name` (ASCII case-insensitive).
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        find_header(&self.headers, name)
    }

    /// Return whether the connection should be kept alive after this request.
    pub fn keep_alive(&self) -> bool {
        let mut has_close = false;
        let mut has_keep_alive = false;
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case("connection") {
                for tok in v.split(|&b| b == b',') {
                    let trimmed = trim_ows(tok);
                    if trimmed.eq_ignore_ascii_case(b"close") {
                        has_close = true;
                    } else if trimmed.eq_ignore_ascii_case(b"keep-alive") {
                        has_keep_alive = true;
                    }
                }
            }
        }
        if self.minor >= 1 {
            !has_close
        } else {
            !has_close && has_keep_alive
        }
    }
}

impl Response {
    /// Return the value of the first header matching `name` (ASCII case-insensitive).
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        find_header(&self.headers, name)
    }
}

/// Find the first header matching `name` (ASCII case-insensitive).
pub(crate) fn find_header<'a>(headers: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    for (k, v) in headers {
        if k.eq_ignore_ascii_case(name) {
            return Some(v.as_slice());
        }
    }
    None
}

/// Trim leading and trailing optional whitespace (`SP` and `HTAB`).
pub(crate) fn trim_ows(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if b == b' ' || b == b'\t' {
            s = &s[1..];
        } else {
            break;
        }
    }
    while let Some(&b) = s.last() {
        if b == b' ' || b == b'\t' {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}
