//! Body framing for HTTP/1.1 messages (`Content-Length` and `Transfer-Encoding: chunked`).

use alloc::string::String;
use alloc::vec::Vec;

use crate::head::{is_tchar, parse_header_line, read_line_limited};
use crate::types::{trim_ows, Error, Limits};

/// Validate `Transfer-Encoding` headers and return `Ok(true)` if chunked framing applies.
/// Rejects empty, malformed, duplicate `chunked`, or non-`chunked`-terminated codings.
pub(crate) fn parse_transfer_encoding(headers: &[(String, Vec<u8>)]) -> Result<bool, Error> {
    let mut has_te = false;
    let mut chunked_count = 0usize;
    let mut last: Option<&[u8]> = None;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("transfer-encoding") {
            has_te = true;
            for part in v.split(|&b| b == b',') {
                let trimmed = trim_ows(part);
                if trimmed.is_empty() {
                    continue;
                }
                if !trimmed.iter().all(|&b| is_tchar(b)) {
                    return Err(Error::BadHeader);
                }
                if trimmed.eq_ignore_ascii_case(b"chunked") {
                    chunked_count += 1;
                }
                last = Some(trimmed);
            }
        }
    }
    if !has_te {
        return Ok(false);
    }
    if chunked_count == 1 && last.is_some_and(|c| c.eq_ignore_ascii_case(b"chunked")) {
        Ok(true)
    } else {
        Err(Error::BadHeader)
    }
}

/// Parse and validate `Content-Length` headers, rejecting non-digits, overflows, and mismatches.
pub(crate) fn parse_content_length(headers: &[(String, Vec<u8>)]) -> Result<Option<usize>, Error> {
    let mut expected: Option<usize> = None;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            for part in v.split(|&b| b == b',') {
                let s = trim_ows(part);
                if s.is_empty() || !s.iter().all(|b| b.is_ascii_digit()) {
                    return Err(Error::BadContentLength);
                }
                let mut val: usize = 0;
                for &b in s {
                    val = val
                        .checked_mul(10)
                        .and_then(|n| n.checked_add(usize::from(b - b'0')))
                        .ok_or(Error::BadContentLength)?;
                }
                match expected {
                    Some(prev) if prev != val => return Err(Error::BadContentLength),
                    None => expected = Some(val),
                    _ => {}
                }
            }
        }
    }
    Ok(expected)
}

/// Parse a `Content-Length`-delimited body starting at `start`.
pub(crate) fn parse_fixed_body(
    buf: &[u8],
    start: usize,
    len: usize,
    lim: &Limits,
) -> Result<(Vec<u8>, usize), Error> {
    if len > lim.max_body {
        return Err(Error::BodyTooLarge);
    }
    let avail = buf.len().saturating_sub(start);
    if avail < len {
        return Err(Error::Incomplete);
    }
    let end = start + len;
    Ok((buf[start..end].to_vec(), end))
}

/// Parse a `Transfer-Encoding: chunked` body starting at `start`.
pub(crate) fn parse_chunked_body(
    buf: &[u8],
    start: usize,
    lim: &Limits,
) -> Result<(Vec<u8>, usize), Error> {
    let mut pos = start;
    let mut body = Vec::new();
    let max_chunk_line = lim.max_head.max(64);

    loop {
        let max_end = pos.saturating_add(max_chunk_line);
        let (line, next_pos) = read_line_limited(buf, pos, max_end).map_err(|e| match e {
            Error::HeadTooLarge => Error::BadChunk,
            other => other,
        })?;
        pos = next_pos;

        let size = parse_chunk_size_line(line)?;
        if size == 0 {
            pos = parse_trailers(buf, pos, lim)?;
            return Ok((body, pos));
        }

        let new_len = body.len().checked_add(size).ok_or(Error::BodyTooLarge)?;
        if new_len > lim.max_body {
            return Err(Error::BodyTooLarge);
        }

        let avail = buf.len().saturating_sub(pos);
        if avail < size {
            return Err(Error::Incomplete);
        }
        let after_data = pos + size;
        let next_chunk_pos = consume_eol(buf, after_data)?;
        body.extend_from_slice(&buf[pos..after_data]);
        pos = next_chunk_pos;
    }
}

/// Parse `<hex-size>[;ext]` from a chunk header line, ignoring valid extensions.
fn parse_chunk_size_line(line: &[u8]) -> Result<usize, Error> {
    if line
        .iter()
        .any(|&b| b == b'\r' || (b < 0x20 && b != b'\t') || b == 0x7F)
    {
        return Err(Error::BadChunk);
    }
    let raw_size = match line.iter().position(|&b| b == b';') {
        Some(semi) => trim_trailing_ows(&line[..semi]),
        None => line,
    };
    if raw_size.is_empty() {
        return Err(Error::BadChunk);
    }
    let mut size: usize = 0;
    for &b in raw_size {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(Error::BadChunk),
        };
        size = size
            .checked_mul(16)
            .and_then(|n| n.checked_add(usize::from(digit)))
            .ok_or(Error::BadChunk)?;
    }
    Ok(size)
}

/// Consume a single CRLF or LF at `pos`, returning the index after it.
fn consume_eol(buf: &[u8], pos: usize) -> Result<usize, Error> {
    if pos >= buf.len() {
        return Err(Error::Incomplete);
    }
    if buf[pos] == b'\n' {
        Ok(pos + 1)
    } else if buf[pos] == b'\r' {
        if pos + 1 >= buf.len() {
            Err(Error::Incomplete)
        } else if buf[pos + 1] == b'\n' {
            Ok(pos + 2)
        } else {
            Err(Error::BadChunk)
        }
    } else {
        Err(Error::BadChunk)
    }
}

/// Parse and discard trailer headers after the `0`-size last chunk.
fn parse_trailers(buf: &[u8], start: usize, lim: &Limits) -> Result<usize, Error> {
    let mut pos = start;
    let max_end = start.saturating_add(lim.max_head);
    let mut count = 0usize;
    loop {
        let (line, next_pos) = read_line_limited(buf, pos, max_end)?;
        pos = next_pos;
        if line.is_empty() {
            return Ok(pos);
        }
        if count >= lim.max_headers {
            return Err(Error::TooManyHeaders);
        }
        let _ = parse_header_line(line)?;
        count += 1;
    }
}

/// Trim trailing `SP` and `HTAB` only.
fn trim_trailing_ows(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.last() {
        if b == b' ' || b == b'\t' {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}
