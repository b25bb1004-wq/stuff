//! Head parsing for HTTP/1.1 requests and responses (start-line and headers).

use alloc::string::String;
use alloc::vec::Vec;

use crate::types::{trim_ows, Error, Limits};

/// Check whether `b` is an HTTP token character (`tchar` per RFC 9110 Section 5.6.2).
pub(crate) fn is_tchar(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
            | b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
    )
}

/// Check whether `b` is a visible ASCII character (`VCHAR`).
pub(crate) fn is_vchar(b: u8) -> bool {
    (0x21..=0x7E).contains(&b)
}

/// Check whether `b` is allowed inside an HTTP header field value or reason phrase.
pub(crate) fn is_field_value_byte(b: u8) -> bool {
    b == b'\t' || (0x20..=0x7E).contains(&b) || b >= 0x80
}

/// Read one CRLF- or LF-terminated line starting at `pos` within `max_end`.
pub(crate) fn read_line_limited(
    buf: &[u8],
    pos: usize,
    max_end: usize,
) -> Result<(&[u8], usize), Error> {
    let limit = buf.len().min(max_end);
    if pos >= limit {
        return if buf.len() >= max_end && (!buf.is_empty() || max_end > 0) {
            Err(Error::HeadTooLarge)
        } else {
            Err(Error::Incomplete)
        };
    }
    let slice = &buf[pos..limit];
    if let Some(rel_nl) = slice.iter().position(|&b| b == b'\n') {
        let nl_idx = pos + rel_nl;
        let line = if rel_nl > 0 && buf[nl_idx - 1] == b'\r' {
            &buf[pos..nl_idx - 1]
        } else {
            &buf[pos..nl_idx]
        };
        Ok((line, nl_idx + 1))
    } else if buf.len() >= max_end {
        Err(Error::HeadTooLarge)
    } else {
        Err(Error::Incomplete)
    }
}

/// Parse `HTTP/1.0` or `HTTP/1.1` into its minor version (`0` or `1`).
fn parse_version(v: &[u8]) -> Result<u8, Error> {
    if v == b"HTTP/1.0" {
        Ok(0)
    } else if v == b"HTTP/1.1" {
        Ok(1)
    } else {
        Err(Error::BadVersion)
    }
}

/// Parse a single header line (`name: value`) and validate token, value, and obs-fold.
pub(crate) fn parse_header_line(line: &[u8]) -> Result<(String, Vec<u8>), Error> {
    if line.first().is_some_and(|&b| b == b' ' || b == b'\t') {
        return Err(Error::BadHeader);
    }
    let colon = line
        .iter()
        .position(|&b| b == b':')
        .ok_or(Error::BadHeader)?;
    let name_bytes = &line[..colon];
    if name_bytes.is_empty() || !name_bytes.iter().all(|&b| is_tchar(b)) {
        return Err(Error::BadHeader);
    }
    let val_bytes = trim_ows(&line[colon + 1..]);
    if !val_bytes.iter().all(|&b| is_field_value_byte(b)) {
        return Err(Error::BadHeader);
    }
    // `name_bytes` is guaranteed pure ASCII (`tchar`), so UTF-8 conversion cannot fail.
    let name = String::from_utf8(name_bytes.to_vec()).map_err(|_| Error::BadHeader)?;
    Ok((name, val_bytes.to_vec()))
}

/// Parsed request head fields before body decoding.
pub(crate) struct ParsedRequestHead {
    pub method: String,
    pub target: String,
    pub minor: u8,
    pub headers: Vec<(String, Vec<u8>)>,
    pub head_end: usize,
}

/// Parsed response head fields before body decoding.
pub(crate) struct ParsedResponseHead {
    pub minor: u8,
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub head_end: usize,
}

/// Parse the request line and all headers up to and including the blank line.
pub(crate) fn parse_request_head(buf: &[u8], lim: &Limits) -> Result<ParsedRequestHead, Error> {
    let (line, mut pos) = read_line_limited(buf, 0, lim.max_head)?;
    if line.is_empty() {
        return Err(Error::BadRequestLine);
    }
    let sp1 = line
        .iter()
        .position(|&b| b == b' ')
        .ok_or(Error::BadRequestLine)?;
    let method_bytes = &line[..sp1];
    if method_bytes.is_empty() || !method_bytes.iter().all(|&b| is_tchar(b)) {
        return Err(Error::BadMethod);
    }
    let rest = &line[sp1 + 1..];
    let sp2 = rest
        .iter()
        .position(|&b| b == b' ')
        .ok_or(Error::BadRequestLine)?;
    let target_bytes = &rest[..sp2];
    // Raw UTF-8 bytes (>= 0x80) are allowed; the router serves them like their %-escaped form.
    if target_bytes.is_empty() || !target_bytes.iter().all(|&b| is_vchar(b) || b >= 0x80) {
        return Err(Error::BadRequestLine);
    }
    let version_bytes = &rest[sp2 + 1..];
    if version_bytes.is_empty() || version_bytes.contains(&b' ') {
        return if version_bytes.starts_with(b"HTTP/") {
            Err(Error::BadVersion)
        } else {
            Err(Error::BadRequestLine)
        };
    }
    let minor = parse_version(version_bytes)?;

    let headers = parse_headers_loop(buf, &mut pos, lim)?;
    let method = String::from_utf8(method_bytes.to_vec()).map_err(|_| Error::BadMethod)?;
    let target = String::from_utf8(target_bytes.to_vec()).map_err(|_| Error::BadRequestLine)?;
    Ok(ParsedRequestHead {
        method,
        target,
        minor,
        headers,
        head_end: pos,
    })
}

/// Parse the status line and all headers up to and including the blank line.
pub(crate) fn parse_response_head(buf: &[u8], lim: &Limits) -> Result<ParsedResponseHead, Error> {
    let (line, mut pos) = read_line_limited(buf, 0, lim.max_head)?;
    if line.is_empty() {
        return Err(Error::BadStatusLine);
    }
    let sp1 = line
        .iter()
        .position(|&b| b == b' ')
        .ok_or(Error::BadStatusLine)?;
    let version_bytes = &line[..sp1];
    let minor = if version_bytes.starts_with(b"HTTP/") {
        parse_version(version_bytes)?
    } else {
        return Err(Error::BadStatusLine);
    };

    let rest = &line[sp1 + 1..];
    if rest.len() < 3 {
        return Err(Error::BadStatusLine);
    }
    let code_bytes = &rest[..3];
    if !code_bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(Error::BadStatusLine);
    }
    let status = u16::from(code_bytes[0] - b'0') * 100
        + u16::from(code_bytes[1] - b'0') * 10
        + u16::from(code_bytes[2] - b'0');

    let reason_bytes = if rest.len() == 3 {
        &[][..]
    } else if rest[3] == b' ' {
        &rest[4..]
    } else {
        return Err(Error::BadStatusLine);
    };
    if !reason_bytes.iter().all(|&b| is_field_value_byte(b)) {
        return Err(Error::BadStatusLine);
    }
    let reason = String::from_utf8(reason_bytes.to_vec()).map_err(|_| Error::BadStatusLine)?;

    let headers = parse_headers_loop(buf, &mut pos, lim)?;
    Ok(ParsedResponseHead {
        minor,
        status,
        reason,
        headers,
        head_end: pos,
    })
}

/// Parse header lines until the blank line, enforcing `max_headers` and `max_head`.
fn parse_headers_loop(
    buf: &[u8],
    pos: &mut usize,
    lim: &Limits,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let mut headers = Vec::new();
    loop {
        let (line, next_pos) = read_line_limited(buf, *pos, lim.max_head)?;
        *pos = next_pos;
        if line.is_empty() {
            return Ok(headers);
        }
        if headers.len() >= lim.max_headers {
            return Err(Error::TooManyHeaders);
        }
        headers.push(parse_header_line(line)?);
    }
}
