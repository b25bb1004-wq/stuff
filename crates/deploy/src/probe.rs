//! TCP and HTTP/1.1 health probes with bounded timeouts.

#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const MAX_HTTP_BODY: usize = 4 * 1024 * 1024;
const MAX_HTTP_HEADERS: usize = 64 * 1024;

/// Probe TCP connectivity to `addr` within `timeout`.
pub fn tcp(addr: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let _stream = connect_with_timeout(addr, deadline)?;
    Ok(())
}

/// Perform an HTTP/1.1 GET on `addr` at `path` within `timeout`, returning the body on 200 OK.
pub fn http(addr: &str, path: &str, timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut stream = connect_with_timeout(addr, deadline)?;

    let target = if path.is_empty() {
        "/"
    } else if path.starts_with('/') {
        path
    } else {
        return http_with_target(&mut stream, addr, &format!("/{path}"), deadline);
    };
    http_with_target(&mut stream, addr, target, deadline)
}

fn http_with_target(
    stream: &mut TcpStream,
    host: &str,
    target: &str,
    deadline: Instant,
) -> Result<String, String> {
    let req = format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    write_all_deadline(stream, req.as_bytes(), deadline)?;
    let raw = read_response_deadline(stream, deadline)?;
    parse_http_response(&raw)
}

fn connect_with_timeout(addr: &str, deadline: Instant) -> Result<TcpStream, String> {
    let addrs: Vec<SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| format!("resolve {addr}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("resolve {addr}: no addresses"));
    }

    let mut last_err = String::from("connection failed");
    for sa in addrs {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!("connect {addr}: timed out"));
        }
        let rem = deadline
            .saturating_duration_since(now)
            .max(Duration::from_millis(1));
        match TcpStream::connect_timeout(&sa, rem) {
            Ok(stream) => return Ok(stream),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(format!("connect {addr}: {last_err}"))
}

fn write_all_deadline(
    stream: &mut TcpStream,
    mut data: &[u8],
    deadline: Instant,
) -> Result<(), String> {
    while !data.is_empty() {
        let rem = remaining_or_timeout(deadline, "http write")?;
        let _ = stream.set_write_timeout(Some(rem));
        match stream.write(data) {
            Ok(0) => return Err("http write: connection closed".to_string()),
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if is_timeout_err(&e) => return Err("http write: timed out".to_string()),
            Err(e) => return Err(format!("http write: {e}")),
        }
    }
    Ok(())
}

fn read_response_deadline(stream: &mut TcpStream, deadline: Instant) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut header_end: Option<usize> = None;
    let mut expected_len: Option<usize> = None;
    let mut chunked = false;

    loop {
        if let (Some(hend), Some(clen)) = (header_end, expected_len) {
            if buf.len().saturating_sub(hend) >= clen {
                break;
            }
        }
        if let Some(hend) = header_end {
            if chunked && is_chunked_complete(&buf[hend..])? {
                break;
            }
        }

        let rem = remaining_or_timeout(deadline, "http read")?;
        let _ = stream.set_read_timeout(Some(rem));
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len().saturating_add(n) > MAX_HTTP_HEADERS.saturating_add(MAX_HTTP_BODY) {
                    return Err("http response exceeds maximum size".to_string());
                }
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                        let hend = pos.saturating_add(4);
                        header_end = Some(hend);
                        let (clen, is_chunked) = inspect_headers(&buf[..pos])?;
                        expected_len = clen;
                        chunked = is_chunked;
                    } else if buf.len() > MAX_HTTP_HEADERS {
                        return Err("http headers exceed maximum size".to_string());
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if is_timeout_err(&e) => return Err("http read: timed out".to_string()),
            Err(e) => return Err(format!("http read: {e}")),
        }
    }
    Ok(buf)
}

fn remaining_or_timeout(deadline: Instant, op: &str) -> Result<Duration, String> {
    let now = Instant::now();
    if now >= deadline {
        return Err(format!("{op}: timed out"));
    }
    Ok(deadline
        .saturating_duration_since(now)
        .max(Duration::from_millis(1)))
}

fn is_timeout_err(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn inspect_headers(header_bytes: &[u8]) -> Result<(Option<usize>, bool), String> {
    let text = std::str::from_utf8(header_bytes)
        .map_err(|_| "http headers are not valid utf-8".to_string())?;
    let mut content_length = None;
    let mut chunked = false;
    for line in text.split("\r\n").skip(1) {
        if let Some((name, val)) = line.split_once(':') {
            let name_lc = name.trim().to_ascii_lowercase();
            let val_trim = val.trim();
            if name_lc == "content-length" {
                let len = val_trim
                    .parse::<usize>()
                    .map_err(|_| "invalid content-length".to_string())?;
                if len > MAX_HTTP_BODY {
                    return Err("content-length exceeds maximum size".to_string());
                }
                content_length = Some(len);
            } else if name_lc == "transfer-encoding"
                && val_trim.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    Ok((content_length, chunked))
}

fn is_chunked_complete(body: &[u8]) -> Result<bool, String> {
    Ok(decode_chunked(body)?.is_some())
}

fn decode_chunked(mut data: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    loop {
        let Some(line_end) = find_subslice(data, b"\r\n") else {
            return Ok(None);
        };
        let size_line = std::str::from_utf8(&data[..line_end])
            .map_err(|_| "invalid utf-8 in chunk size".to_string())?;
        let hex_part = size_line.split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(hex_part, 16).map_err(|_| "invalid chunk size".to_string())?;
        data = &data[line_end + 2..];
        if size == 0 {
            if data.starts_with(b"\r\n") {
                return Ok(Some(out));
            }
            if find_subslice(data, b"\r\n\r\n").is_some() {
                return Ok(Some(out));
            }
            return Ok(None);
        }
        if out.len().saturating_add(size) > MAX_HTTP_BODY {
            return Err("chunked body exceeds maximum size".to_string());
        }
        if data.len() < size.saturating_add(2) {
            return Ok(None);
        }
        out.extend_from_slice(&data[..size]);
        if &data[size..size + 2] != b"\r\n" {
            return Err("malformed chunk terminator".to_string());
        }
        data = &data[size + 2..];
    }
}

fn parse_http_response(raw: &[u8]) -> Result<String, String> {
    let Some(pos) = find_subslice(raw, b"\r\n\r\n") else {
        return Err("incomplete http response".to_string());
    };
    let head = std::str::from_utf8(&raw[..pos])
        .map_err(|_| "http headers are not valid utf-8".to_string())?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| "missing http status line".to_string())?;
    let mut parts = status_line.split_ascii_whitespace();
    let proto = parts
        .next()
        .ok_or_else(|| "missing http protocol".to_string())?;
    if !proto.starts_with("HTTP/1.") {
        return Err(format!("unexpected protocol: {proto}"));
    }
    let code_str = parts
        .next()
        .ok_or_else(|| "missing http status code".to_string())?;
    let status: u16 = code_str
        .parse()
        .map_err(|_| format!("invalid http status code: {code_str}"))?;
    if status != 200 {
        return Err(format!("http status {status}"));
    }

    let (content_length, chunked) = inspect_headers(&raw[..pos])?;
    let body_raw = &raw[pos + 4..];
    let body_bytes = if chunked {
        decode_chunked(body_raw)?.ok_or_else(|| "incomplete chunked body".to_string())?
    } else if let Some(len) = content_length {
        if body_raw.len() < len {
            return Err("truncated http body".to_string());
        }
        body_raw[..len].to_vec()
    } else {
        body_raw.to_vec()
    };

    String::from_utf8(body_bytes).map_err(|_| "http body is not valid utf-8".to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_200_and_non_200_responses() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        assert_eq!(parse_http_response(ok).unwrap(), "hello");

        let err = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\n\r\nbusy";
        assert!(parse_http_response(err).unwrap_err().contains("503"));

        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        assert_eq!(parse_http_response(chunked).unwrap(), "hello");
    }

    #[test]
    fn http_probe_succeeds_and_rejects_non_200() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                if idx == 0 {
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                } else {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                }
            }
        });

        assert_eq!(
            http(&addr, "/health", Duration::from_secs(1)).unwrap(),
            "ok"
        );
        assert!(http(&addr, "/missing", Duration::from_secs(1)).is_err());
        handle.join().unwrap();
    }

    #[test]
    fn prop_parse_http_response_never_panics_and_roundtrips() {
        assay::forall(400, 0x77_00_01, |rng| {
            let garbage = rng.bytes(256);
            let _ = parse_http_response(&garbage);

            let body_bytes: Vec<u8> = rng
                .bytes(128)
                .into_iter()
                .map(|b| b'a' + (b % 26))
                .collect();
            let body_str = std::str::from_utf8(&body_bytes).unwrap();
            let raw = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body_str}",
                body_str.len()
            );
            assert_eq!(parse_http_response(raw.as_bytes()).unwrap(), body_str);
        });
    }
}
