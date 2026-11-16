//! The model client: one chat completion over plain HTTP to a loopback address.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use aci::envelope::Code;
use aci::json::Json;

use crate::types::{Error, Result};

/// Largest reply body accepted, in bytes.
const MAX_BODY: usize = 8 * 1024 * 1024;
/// Longest single header or chunk-size line accepted, in bytes.
const MAX_LINE: u64 = 64 * 1024;
/// The connect timeout never exceeds this.
const CONNECT_CAP: Duration = Duration::from_secs(5);
/// Socket timeouts are never zero (a zero timeout is an error).
const MIN_TIMEOUT: Duration = Duration::from_millis(1);
/// Shown with every URL error.
const EXPECTED: &str =
    "write it as http://127.0.0.1:8080 (the port is optional, the default is 80)";
/// Shown when the host is not this machine.
const DENIED_HINT: &str =
    "the model server must run on this machine (127.0.0.1); reaching other machines needs the operator";

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The harness's standing instructions.
    System,
    /// The task and the observations.
    User,
    /// The model's earlier turns.
    Assistant,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who wrote it.
    pub role: Role,
    /// Its text.
    pub content: String,
}

/// One chat completion request.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest<'a> {
    /// Base URL, loopback only, for example `http://127.0.0.1:8080`.
    pub url: &'a str,
    /// Model name.
    pub model: &'a str,
    /// The conversation so far.
    pub messages: &'a [Message],
    /// Most tokens to write.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f64,
    /// Sampling seed.
    pub seed: i64,
    /// When set, the reply must be JSON matching this schema (sent as `response_format`).
    pub json_schema: Option<&'a Json>,
    /// Let the model think first (`chat_template_kwargs.enable_thinking`).
    pub thinking: bool,
    /// Give up after this long.
    pub timeout: Duration,
}

/// The model's reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// `choices[0].message.content`.
    pub content: String,
    /// `usage.prompt_tokens` (0 if absent).
    pub prompt_tokens: u64,
    /// `usage.completion_tokens` (0 if absent).
    pub completion_tokens: u64,
    /// Wall time in milliseconds.
    pub ms: u64,
}

/// Where the model server is: the host as written (IPv6 keeps its brackets), the port and the base path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Host as written: `127.0.0.1`, `localhost` or `[::1]`.
    pub host: String,
    /// TCP port (80 when the URL has none).
    pub port: u16,
    /// Path before `/v1/...`, without a trailing slash; empty for the root.
    pub base: String,
}

/// Reads `http://HOST[:PORT][/BASE]`. Fails with `denied` unless HOST is this machine.
/// Nothing is connected.
pub fn parse_url(url: &str) -> Result<Endpoint> {
    if url.starts_with("https://") {
        return Err(
            Error::new(Code::Unsupported, "only plain http to this machine")
                .hint("use http://127.0.0.1:PORT"),
        );
    }
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| bad_url(url, "must start with http://"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (host, port) = split_authority(authority, url)?;
    if loopback_ip(host).is_none() {
        return Err(Error::new(
            Code::Denied,
            format!("model server host `{host}` is not this machine"),
        )
        .hint(DENIED_HINT));
    }
    Ok(Endpoint {
        host: host.to_string(),
        port,
        base: path.trim_end_matches('/').to_string(),
    })
}

/// A URL error with the expected form as the hint.
fn bad_url(url: &str, why: &str) -> Error {
    Error::new(Code::Usage, format!("model URL `{url}` {why}")).hint(EXPECTED)
}

/// Splits `HOST[:PORT]` (with `[v6]` hosts) into the host and the port.
fn split_authority<'a>(authority: &'a str, url: &str) -> Result<(&'a str, u16)> {
    let (host, port) = if authority.starts_with('[') {
        let close = authority
            .find(']')
            .ok_or_else(|| bad_url(url, "has an unclosed `[` in the host"))?;
        match &authority[close + 1..] {
            "" => (&authority[..=close], None),
            p => {
                let p = p
                    .strip_prefix(':')
                    .ok_or_else(|| bad_url(url, "has text after the host"))?;
                (&authority[..=close], Some(p))
            }
        }
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(bad_url(url, "has no host"));
    }
    let port = match port {
        None => 80,
        Some(p) => p
            .parse::<u16>()
            .map_err(|_| bad_url(url, &format!("has a bad port `{p}`")))?,
    };
    Ok((host, port))
}

/// The IP address of a loopback host (`localhost`, `[::1]`, or IPv4 starting with 127), if it is one.
fn loopback_ip(host: &str) -> Option<IpAddr> {
    if host == "localhost" {
        return Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    if host == "[::1]" {
        return Some(IpAddr::V6(Ipv6Addr::LOCALHOST));
    }
    match host.parse::<Ipv4Addr>() {
        Ok(ip) if ip.octets()[0] == 127 => Some(IpAddr::V4(ip)),
        _ => None,
    }
}

/// Maps a socket error to an ACI error that says what to do about it.
fn net_error(e: &io::Error, ep: &Endpoint) -> Error {
    if e.kind() == io::ErrorKind::InvalidData {
        return Error::new(Code::Invalid, format!("model server sent bad data: {e}"));
    }
    let err = match e.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Error::new(
            Code::Io,
            format!(
                "timed out waiting for the model server at {}:{}",
                ep.host, ep.port
            ),
        ),
        _ => Error::new(
            Code::Io,
            format!("model server at {}:{} failed: {e}", ep.host, ep.port),
        ),
    };
    err.hint(format!(
        "is llama-server running? try: curl http://127.0.0.1:{}/health",
        ep.port
    ))
}

/// Opens a TCP connection to the loopback endpoint, checking the loopback rule again.
fn connect(ep: &Endpoint, timeout: Duration) -> Result<TcpStream> {
    let ip = loopback_ip(&ep.host).ok_or_else(|| {
        Error::new(
            Code::Denied,
            format!("model server host `{}` is not this machine", ep.host),
        )
        .hint(DENIED_HINT)
    })?;
    let addr = SocketAddr::new(ip, ep.port);
    let stream = TcpStream::connect_timeout(&addr, timeout.min(CONNECT_CAP).max(MIN_TIMEOUT))
        .map_err(|e| net_error(&e, ep))?;
    stream
        .set_read_timeout(Some(timeout.max(MIN_TIMEOUT)))
        .and_then(|()| stream.set_write_timeout(Some(timeout.max(MIN_TIMEOUT))))
        .map_err(|e| net_error(&e, ep))?;
    Ok(stream)
}

/// Reads one line (capped at `MAX_LINE`); returns the bytes read, 0 at end of stream.
fn read_line(reader: &mut impl BufRead, line: &mut String, ep: &Endpoint) -> Result<usize> {
    reader
        .by_ref()
        .take(MAX_LINE)
        .read_line(line)
        .map_err(|e| net_error(&e, ep))
}

/// Sends `request` and reads the whole reply: status code and body.
fn exchange(ep: &Endpoint, request: &str, timeout: Duration) -> Result<(u16, Vec<u8>)> {
    let mut stream = connect(ep, timeout)?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| net_error(&e, ep))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if read_line(&mut reader, &mut line, ep)? == 0 {
        return Err(Error::new(
            Code::Io,
            "model server closed the connection without a reply",
        ));
    }
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            Error::new(
                Code::Invalid,
                format!("bad status line from model server: `{}`", line.trim_end()),
            )
        })?;
    let mut length = None;
    let mut chunked = false;
    loop {
        line.clear();
        let n = read_line(&mut reader, &mut line, ep)?;
        let header = line.trim_end();
        if n == 0 || header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => {
                    let n = value.parse::<usize>().map_err(|_| {
                        Error::new(Code::Invalid, "bad Content-Length from model server")
                    })?;
                    length = Some(n);
                }
                "transfer-encoding" => chunked = value.to_ascii_lowercase().contains("chunked"),
                _ => {}
            }
        }
    }
    let body = if chunked {
        read_chunked(&mut reader, ep)?
    } else if let Some(n) = length {
        if n > MAX_BODY {
            return Err(too_big());
        }
        let mut body = vec![0u8; n];
        reader
            .read_exact(&mut body)
            .map_err(|e| net_error(&e, ep))?;
        body
    } else {
        let mut body = Vec::new();
        (&mut reader)
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| net_error(&e, ep))?;
        if body.len() > MAX_BODY {
            return Err(too_big());
        }
        body
    };
    Ok((status, body))
}

/// The error for a body over the cap.
fn too_big() -> Error {
    Error::new(Code::Invalid, "model server reply is larger than 8 MiB")
}

/// Reads a chunked body: hex sizes (with optional `;ext`), data, the `0` chunk and trailers.
fn read_chunked(reader: &mut impl BufRead, ep: &Endpoint) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        read_line(reader, &mut line, ep)?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| {
            Error::new(
                Code::Invalid,
                format!("bad chunk size `{size_text}` from model server"),
            )
        })?;
        if size == 0 {
            break;
        }
        if body.len() + size > MAX_BODY {
            return Err(too_big());
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader
            .read_exact(&mut body[start..])
            .map_err(|e| net_error(&e, ep))?;
        line.clear();
        read_line(reader, &mut line, ep)?;
    }
    loop {
        line.clear();
        if read_line(reader, &mut line, ep)? == 0 || line.trim_end().is_empty() {
            break;
        }
    }
    Ok(body)
}

/// The first `n` characters of a body, for error messages.
fn preview(raw: &[u8], n: usize) -> String {
    String::from_utf8_lossy(raw).chars().take(n).collect()
}

/// Milliseconds since `t`.
fn ms_since(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// The JSON body of one chat request, as the server expects it.
pub fn request_body(req: &ChatRequest<'_>) -> Json {
    let messages = req
        .messages
        .iter()
        .map(|m| {
            Json::obj([
                ("role", Json::Str(role_name(m.role).into())),
                ("content", Json::Str(m.content.clone())),
            ])
        })
        .collect();
    let mut body = Json::obj([
        ("model", Json::Str(req.model.into())),
        ("messages", Json::Arr(messages)),
        ("max_tokens", Json::Int(i64::from(req.max_tokens))),
        ("temperature", Json::Num(req.temperature)),
        ("top_p", Json::Num(0.8)),
        ("top_k", Json::Int(20)),
        ("seed", Json::Int(req.seed)),
        ("stream", Json::Bool(false)),
        ("cache_prompt", Json::Bool(true)),
        (
            "chat_template_kwargs",
            Json::obj([("enable_thinking", Json::Bool(req.thinking))]),
        ),
    ]);
    if let (Some(schema), Json::Obj(pairs)) = (req.json_schema, &mut body) {
        let format = Json::obj([
            ("type", Json::Str("json_schema".into())),
            (
                "json_schema",
                Json::obj([
                    ("name", Json::Str("action".into())),
                    ("strict", Json::Bool(true)),
                    ("schema", schema.clone()),
                ]),
            ),
        ]);
        pairs.push(("response_format".into(), format));
    }
    body
}

/// Reads the reply body: `choices[0].message.content`, token usage. The timing is set by the caller.
fn parse_reply(raw: &[u8]) -> Result<Reply> {
    let text = String::from_utf8_lossy(raw);
    let invalid = |why: String| Error::new(Code::Invalid, format!("{why}: {}", preview(raw, 200)));
    let v =
        Json::parse(&text).map_err(|e| invalid(format!("model server reply is not JSON ({e})")))?;
    let message = v
        .get("choices")
        .and_then(Json::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .ok_or_else(|| invalid("model server reply has no choices[0].message".into()))?;
    let content = match message.get("content") {
        None | Some(Json::Null) => String::new(),
        Some(Json::Str(s)) => s.clone(),
        Some(_) => return Err(invalid("model server reply content is not a string".into())),
    };
    let usage = |key: &str| {
        v.get("usage")
            .and_then(|u| u.get(key))
            .and_then(Json::as_i64)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0)
    };
    Ok(Reply {
        content,
        prompt_tokens: usage("prompt_tokens"),
        completion_tokens: usage("completion_tokens"),
        ms: 0,
    })
}

/// Sends the request to `POST {url}/v1/chat/completions` and returns the reply.
/// `denied` for a non-loopback host, `io` for connection trouble, `invalid` for a bad response.
pub fn chat(req: &ChatRequest<'_>) -> Result<Reply> {
    let started = Instant::now();
    let ep = parse_url(req.url)?;
    let body = request_body(req).write();
    let head = format!(
        "POST {}/v1/chat/completions HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccept: application/json\r\n\r\n",
        ep.base,
        ep.host,
        ep.port,
        body.len()
    );
    let (status, raw) = exchange(&ep, &(head + &body), req.timeout)?;
    if status != 200 {
        return Err(Error::new(
            Code::Io,
            format!(
                "model server answered HTTP {status}: {}",
                preview(&raw, 300)
            ),
        ));
    }
    let mut reply = parse_reply(&raw)?;
    reply.ms = ms_since(started);
    Ok(reply)
}

/// Checks that the server answers `GET {url}/health` with 200 (llama-server answers 503 while it loads).
pub fn ping(url: &str, timeout: Duration) -> Result<()> {
    let ep = parse_url(url)?;
    let head = format!(
        "GET {}/health HTTP/1.1\r\nHost: {}:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        ep.base, ep.host, ep.port
    );
    let (status, raw) = exchange(&ep, &head, timeout)?;
    if status == 200 {
        return Ok(());
    }
    let err = Error::new(
        Code::Io,
        format!(
            "model server answered HTTP {status}: {}",
            preview(&raw, 300)
        ),
    );
    Err(if status == 503 {
        err.hint("llama-server is still loading the model; wait and try again")
    } else {
        err
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_url_table() {
        let ok = |u: &str, host: &str, port: u16, base: &str| {
            let e = parse_url(u).unwrap();
            assert_eq!(
                (e.host.as_str(), e.port, e.base.as_str()),
                (host, port, base),
                "{u}"
            );
        };
        ok("http://127.0.0.1", "127.0.0.1", 80, "");
        ok("http://127.0.0.1:8080", "127.0.0.1", 8080, "");
        ok("http://127.0.0.1:8080/", "127.0.0.1", 8080, "");
        ok("http://[::1]:9000", "[::1]", 9000, "");
        ok("http://localhost", "localhost", 80, "");
        ok("http://localhost:8080/api/", "localhost", 8080, "/api");

        for bad in [
            "ftp://x",
            "127.0.0.1:8080",
            "http://",
            "http://127.0.0.1:99999",
            "http://127.0.0.1:x",
        ] {
            let e = parse_url(bad).unwrap_err();
            assert_eq!(e.code, Code::Usage, "{bad}");
        }
        let e = parse_url("127.0.0.1:8080").unwrap_err();
        assert!(e.hint.unwrap().contains("http://127.0.0.1:8080"));

        for denied in [
            "http://example.com:8080",
            "http://10.0.0.5:8080",
            "http://192.168.1.2",
        ] {
            let e = parse_url(denied).unwrap_err();
            assert_eq!(e.code, Code::Denied, "{denied}");
            assert!(e.message.contains(
                denied
                    .split("//")
                    .nth(1)
                    .unwrap()
                    .split([':', '/'])
                    .next()
                    .unwrap()
            ));
        }
        let e = parse_url("https://127.0.0.1:8080").unwrap_err();
        assert_eq!(e.code, Code::Unsupported);
    }
}
