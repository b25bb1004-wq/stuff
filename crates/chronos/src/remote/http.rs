//! Minimal HTTP/1.1 client over `TcpStream` using `mercury` wire serialization and parsing.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::string::{String, ToString};
use std::time::Duration;
use std::vec;
use std::vec::Vec;

use mercury::{parse_response, write_request, Error as HttpError, Limits, Request};

use super::pack::MAX_PACK_BYTES;
use super::RemoteError;

struct ParsedUrl {
    connect_addr: String,
    host_header: String,
    prefix: String,
}

fn parse_url(url: &str) -> Result<ParsedUrl, RemoteError> {
    let rest = url.strip_prefix("http://").ok_or(RemoteError::Protocol)?;
    let (authority, raw_path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(RemoteError::Protocol);
    }
    let connect_addr = if authority.contains(':') {
        authority.to_string()
    } else {
        std::format!("{authority}:80")
    };
    let trimmed = raw_path.trim_matches('/');
    let prefix = if trimmed.is_empty() {
        String::new()
    } else {
        std::format!("/{trimmed}")
    };
    Ok(ParsedUrl {
        connect_addr,
        host_header: authority.to_string(),
        prefix,
    })
}

pub(crate) fn http_get(url: &str, suffix: &str) -> Result<Vec<u8>, RemoteError> {
    send_request(url, "GET", suffix, Vec::new())
}

pub(crate) fn http_post(url: &str, suffix: &str, body: Vec<u8>) -> Result<Vec<u8>, RemoteError> {
    send_request(url, "POST", suffix, body)
}

fn send_request(
    url: &str,
    method: &str,
    suffix: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, RemoteError> {
    let parsed = parse_url(url)?;
    let target = std::format!("{}{}", parsed.prefix, suffix);
    let mut headers = vec![
        ("Host".to_string(), parsed.host_header.into_bytes()),
        ("Connection".to_string(), b"close".to_vec()),
    ];
    if body.is_empty() && method == "POST" {
        headers.push(("Content-Length".to_string(), b"0".to_vec()));
    }
    let req = Request {
        method: method.to_string(),
        target,
        minor: 1,
        headers,
        body,
    };
    let wire = write_request(&req);

    let mut stream =
        TcpStream::connect(&parsed.connect_addr).map_err(|e| RemoteError::Io(e.to_string()))?;
    let timeout = Some(Duration::from_secs(30));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    stream
        .write_all(&wire)
        .map_err(|e| RemoteError::Io(e.to_string()))?;
    stream.flush().map_err(|e| RemoteError::Io(e.to_string()))?;

    let lim = Limits {
        max_head: 16 * 1024,
        max_headers: 100,
        max_body: MAX_PACK_BYTES + 1024 * 1024,
    };
    let max_total = MAX_PACK_BYTES + 2 * 1024 * 1024;
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 8192];

    loop {
        match parse_response(&buf, &lim, false) {
            Ok((resp, _)) => {
                if resp.status != 200 {
                    return Err(RemoteError::Http(resp.status));
                }
                return Ok(resp.body);
            }
            Err(HttpError::Incomplete) => {}
            Err(_) => return Err(RemoteError::Protocol),
        }

        if buf.len() >= max_total {
            return Err(RemoteError::Protocol);
        }

        match stream.read(&mut tmp) {
            Ok(0) => {
                return match parse_response(&buf, &lim, false) {
                    Ok((resp, _)) if resp.status == 200 => Ok(resp.body),
                    Ok((resp, _)) => Err(RemoteError::Http(resp.status)),
                    Err(_) => Err(RemoteError::Protocol),
                };
            }
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(RemoteError::Io(e.to_string())),
        }
    }
}
