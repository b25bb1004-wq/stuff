//! Mercury HTTP/1.1 request parser and response writer demo.

use std::fmt::Write as _;

use mercury::{parse_request, write_response, Limits, Response};

use super::{check_input, Demo, Output};

const MAX_DISPLAY_HEADERS: usize = 64;

const DEFAULT_HTTP: &str = "\
POST /api/layers HTTP/1.1
Host: monolith.local
Content-Type: text/plain
Content-Length: 13

hello mercury";

/// Registered Mercury HTTP/1.1 demo.
pub const DEMO: Demo = Demo {
    id: "mercury",
    title: "Mercury HTTP/1.1",
    default_input: DEFAULT_HTTP,
    run,
};

/// Normalize `\n` header line endings to `\r\n`, parse a request, and emit a 200 response.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    if input.trim().is_empty() {
        return Err(String::from("empty HTTP request"));
    }

    let wire = normalize_head_crlf(input);
    let limits = Limits::default();
    let (req, used) =
        parse_request(wire.as_bytes(), &limits).map_err(|e| format!("parse error: {e:?}"))?;

    let mut out = String::new();
    let _ = writeln!(out, "method: {}", req.method);
    let _ = writeln!(out, "target: {}", req.target);
    let _ = writeln!(out, "version: HTTP/1.{}", req.minor);
    let _ = writeln!(out, "keep-alive: {}", req.keep_alive());
    let _ = writeln!(out, "headers ({}):", req.headers.len());
    for (name, val) in req.headers.iter().take(MAX_DISPLAY_HEADERS) {
        let _ = writeln!(out, "  {name}: {}", String::from_utf8_lossy(val));
    }
    if req.headers.len() > MAX_DISPLAY_HEADERS {
        let _ = writeln!(
            out,
            "  ... ({} more headers)",
            req.headers.len() - MAX_DISPLAY_HEADERS
        );
    }
    let _ = writeln!(
        out,
        "body length: {} bytes (consumed {used} wire bytes)",
        req.body.len()
    );

    let resp_body = format!(
        "ok: {} {} ({} headers, {} body bytes)\n",
        req.method,
        req.target,
        req.headers.len(),
        req.body.len()
    );
    let conn_hdr = if req.keep_alive() {
        b"keep-alive".to_vec()
    } else {
        b"close".to_vec()
    };
    let resp = Response {
        minor: req.minor,
        status: 200,
        reason: String::from("OK"),
        headers: vec![
            (
                String::from("Content-Type"),
                b"text/plain; charset=utf-8".to_vec(),
            ),
            (String::from("Connection"), conn_hdr),
        ],
        body: resp_body.into_bytes(),
    };
    let raw_resp = write_response(&resp);

    out.push_str("\n--- 200 response (write_response) ---\n");
    out.push_str(&String::from_utf8_lossy(&raw_resp).replace("\r\n", "\\r\\n\n"));

    Ok(Output::Text(out))
}

/// Convert `\n` line endings in the HTTP head up to the blank separator line into `\r\n`,
/// preserving any trailing message body bytes verbatim.
fn normalize_head_crlf(input: &str) -> String {
    let mut out = String::with_capacity(input.len().saturating_add(16));
    let mut rest = input;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(nl) => {
                let line = rest[..nl].strip_suffix('\r').unwrap_or(&rest[..nl]);
                out.push_str(line);
                out.push_str("\r\n");
                rest = &rest[nl + 1..];
                if line.is_empty() {
                    out.push_str(rest);
                    break;
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}
