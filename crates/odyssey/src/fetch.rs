//! Minimal HTTP/1.1 client built on `std::net::TcpStream` and `mercury`.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use mercury::{write_request, Error as HttpError, Limits, Request, Response};

/// Maximum response body size accepted by [`get`] (8 MiB).
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Maximum number of HTTP redirects followed by [`get`].
pub const MAX_REDIRECTS: usize = 5;
/// Connect and per-read socket timeout.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Overall deadline for reading a single HTTP response (defeats slow-drip servers).
const RESPONSE_DEADLINE: Duration = Duration::from_secs(5);
/// Chunk size for socket reads.
const READ_CHUNK: usize = 8192;

/// Errors returned by [`get`] and URL resolution helpers.
#[derive(Debug)]
pub enum FetchError {
    /// The URL is malformed or missing a host.
    InvalidUrl(String),
    /// The URL uses an unsupported scheme (such as `https`).
    Unsupported(String),
    /// Network, DNS, or socket timeout error.
    Io(io::Error),
    /// HTTP framing or resource-limit error from `mercury`.
    Http(HttpError),
    /// More than [`MAX_REDIRECTS`] redirects were encountered.
    TooManyRedirects,
    /// A redirect status lacked a valid `Location` header.
    InvalidRedirect(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::InvalidUrl(msg) => write!(f, "invalid URL: {msg}"),
            FetchError::Unsupported(msg) => write!(f, "unsupported URL: {msg}"),
            FetchError::Io(err) => write!(f, "I/O error: {err}"),
            FetchError::Http(err) => write!(f, "HTTP error: {err:?}"),
            FetchError::TooManyRedirects => write!(f, "too many redirects (limit {MAX_REDIRECTS})"),
            FetchError::InvalidRedirect(msg) => write!(f, "invalid redirect: {msg}"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FetchError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for FetchError {
    fn from(err: io::Error) -> Self {
        FetchError::Io(err)
    }
}

impl From<HttpError> for FetchError {
    fn from(err: HttpError) -> Self {
        FetchError::Http(err)
    }
}

/// Parsed `http://host[:port]/path` components.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUrl {
    host: String,
    host_header: String,
    port: u16,
    target: String,
    is_ipv6: bool,
}

impl ParsedUrl {
    fn authority(&self) -> String {
        if self.is_ipv6 {
            if self.port == 80 {
                format!("[{}]", self.host)
            } else {
                format!("[{}]:{}", self.host, self.port)
            }
        } else if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// Fetch `url` over HTTP/1.1, following up to 5 redirects and enforcing `limits` (capped at 8 MiB).
pub fn get(url: &str, limits: &Limits) -> Result<Response, FetchError> {
    let (resp, _) = get_with_final_url(url, limits)?;
    Ok(resp)
}

/// Fetch `url` over HTTP/1.1 and return `(response, final_url)`.
pub(crate) fn get_with_final_url(
    url: &str,
    limits: &Limits,
) -> Result<(Response, String), FetchError> {
    let eff_limits = Limits {
        max_head: limits.max_head,
        max_headers: limits.max_headers,
        max_body: limits.max_body.min(MAX_BODY_BYTES),
    };
    let mut current = url.to_string();
    for redirects in 0..=MAX_REDIRECTS {
        let parsed = parse_http_url(&current)?;
        let resp = fetch_once(&parsed, &eff_limits)?;
        if is_redirect(resp.status) {
            if redirects == MAX_REDIRECTS {
                return Err(FetchError::TooManyRedirects);
            }
            let Some(loc_bytes) = resp.header("location") else {
                return Err(FetchError::InvalidRedirect(String::from(
                    "missing Location header",
                )));
            };
            let loc = std::str::from_utf8(loc_bytes)
                .map_err(|_| FetchError::InvalidRedirect(String::from("non-UTF-8 Location")))?
                .trim();
            if loc.is_empty() {
                return Err(FetchError::InvalidRedirect(String::from(
                    "empty Location header",
                )));
            }
            current = resolve_against(&parsed, loc)?;
            continue;
        }
        return Ok((resp, current));
    }
    Err(FetchError::TooManyRedirects)
}

/// Resolve a possibly-relative `reference` against `base_url`.
pub(crate) fn resolve_url(base_url: &str, reference: &str) -> Result<String, FetchError> {
    let trimmed = reference.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(base_url.to_string());
    }
    let parsed = parse_http_url(base_url)?;
    resolve_against(&parsed, trimmed)
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn has_bad_ctl(s: &str) -> bool {
    s.bytes().any(|b| b <= 0x20 || b == 0x7f)
}

fn parse_http_url(raw: &str) -> Result<ParsedUrl, FetchError> {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return Err(FetchError::InvalidUrl(String::from("missing scheme")));
    };
    if scheme.eq_ignore_ascii_case("https") {
        return Err(FetchError::Unsupported(String::from(
            "https is not supported",
        )));
    }
    if !scheme.eq_ignore_ascii_case("http") {
        return Err(FetchError::Unsupported(format!(
            "unsupported scheme '{scheme}'"
        )));
    }
    let rest = rest.split('#').next().unwrap_or(rest);
    let split_idx = rest.find(['/', '?']).unwrap_or(rest.len());
    let auth = rest.get(..split_idx).unwrap_or("");
    let tail = rest.get(split_idx..).unwrap_or("");
    if auth.is_empty() || auth.contains('@') || has_bad_ctl(auth) {
        return Err(FetchError::InvalidUrl(String::from("invalid authority")));
    }
    let target = if tail.is_empty() {
        String::from("/")
    } else if tail.starts_with('?') {
        format!("/{tail}")
    } else {
        tail.to_string()
    };
    if has_bad_ctl(&target) {
        return Err(FetchError::InvalidUrl(String::from("invalid request path")));
    }

    let (host, port, is_ipv6, host_header) = parse_authority(auth)?;
    Ok(ParsedUrl {
        host,
        host_header,
        port,
        target,
        is_ipv6,
    })
}

fn parse_authority(auth: &str) -> Result<(String, u16, bool, String), FetchError> {
    if let Some(after_open) = auth.strip_prefix('[') {
        let Some(close_idx) = after_open.find(']') else {
            return Err(FetchError::InvalidUrl(String::from(
                "unterminated IPv6 host",
            )));
        };
        let Some(host) = after_open.get(..close_idx) else {
            return Err(FetchError::InvalidUrl(String::from("invalid IPv6 host")));
        };
        if host.is_empty() {
            return Err(FetchError::InvalidUrl(String::from("empty IPv6 host")));
        }
        let port_part = after_open.get(close_idx + 1..).unwrap_or("");
        let port = parse_optional_port(port_part)?;
        let header = if port_part.is_empty() {
            format!("[{host}]")
        } else {
            format!("[{host}]:{port}")
        };
        return Ok((host.to_string(), port, true, header));
    }

    if let Some((host, port_str)) = auth.split_once(':') {
        if host.is_empty() || port_str.contains(':') {
            return Err(FetchError::InvalidUrl(String::from("invalid host/port")));
        }
        let port = parse_port(port_str)?;
        let header = format!("{host}:{port}");
        Ok((host.to_string(), port, false, header))
    } else {
        Ok((auth.to_string(), 80, false, auth.to_string()))
    }
}

fn parse_optional_port(s: &str) -> Result<u16, FetchError> {
    if s.is_empty() {
        Ok(80)
    } else if let Some(digits) = s.strip_prefix(':') {
        parse_port(digits)
    } else {
        Err(FetchError::InvalidUrl(String::from("invalid port")))
    }
}

fn parse_port(s: &str) -> Result<u16, FetchError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FetchError::InvalidUrl(String::from("invalid port")));
    }
    let port: u16 = s
        .parse()
        .map_err(|_| FetchError::InvalidUrl(String::from("port out of range")))?;
    if port == 0 {
        return Err(FetchError::InvalidUrl(String::from("port 0 is invalid")));
    }
    Ok(port)
}

fn resolve_against(base: &ParsedUrl, loc: &str) -> Result<String, FetchError> {
    if has_bad_ctl(loc) {
        return Err(FetchError::InvalidUrl(String::from(
            "control characters in URL",
        )));
    }
    let loc_no_frag = loc.split('#').next().unwrap_or(loc);
    if has_scheme_prefix(loc_no_frag) {
        return Ok(loc_no_frag.to_string());
    }
    if let Some(rest) = loc_no_frag.strip_prefix("//") {
        return Ok(format!("http://{rest}"));
    }
    let auth = base.authority();
    if loc_no_frag.starts_with('/') {
        return Ok(format!("http://{auth}{loc_no_frag}"));
    }
    let base_path = base.target.split('?').next().unwrap_or("/");
    if loc_no_frag.is_empty() {
        return Ok(format!("http://{auth}{}", base.target));
    }
    if loc_no_frag.starts_with('?') {
        return Ok(format!("http://{auth}{base_path}{loc_no_frag}"));
    }
    let dir = match base_path.rfind('/') {
        Some(idx) => base_path.get(..=idx).unwrap_or("/"),
        None => "/",
    };
    let (rel_path, rel_query) = match loc_no_frag.find('?') {
        Some(q) => (
            loc_no_frag.get(..q).unwrap_or(loc_no_frag),
            loc_no_frag.get(q..).unwrap_or(""),
        ),
        None => (loc_no_frag, ""),
    };
    let joined = format!("{dir}{rel_path}");
    let normalized = normalize_path(&joined);
    Ok(format!("http://{auth}{normalized}{rel_query}"))
}

fn has_scheme_prefix(s: &str) -> bool {
    let Some(idx) = s.find("://") else {
        return false;
    };
    let Some(prefix) = s.get(..idx) else {
        return false;
    };
    !prefix.is_empty()
        && !prefix.contains('/')
        && !prefix.contains('?')
        && prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

fn normalize_path(path: &str) -> String {
    let trailing_slash = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");
    let mut segs: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    if segs.is_empty() {
        return String::from("/");
    }
    let mut out = String::new();
    for s in segs {
        out.push('/');
        out.push_str(s);
    }
    if trailing_slash {
        out.push('/');
    }
    out
}

fn fetch_once(url: &ParsedUrl, lim: &Limits) -> Result<Response, FetchError> {
    let addrs = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(FetchError::Io)?;
    let mut last_err = None;
    let mut connected = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, IO_TIMEOUT) {
            Ok(s) => {
                connected = Some(s);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let mut stream = match connected {
        Some(s) => s,
        None => {
            return Err(FetchError::Io(last_err.unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::AddrNotAvailable, "no address resolved")
            })));
        }
    };
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let req = Request {
        method: String::from("GET"),
        target: url.target.clone(),
        minor: 1,
        headers: vec![
            (String::from("Host"), url.host_header.as_bytes().to_vec()),
            (String::from("Connection"), b"close".to_vec()),
        ],
        body: Vec::new(),
    };
    let wire = write_request(&req);
    stream.write_all(&wire)?;
    stream.flush()?;

    read_response(&mut stream, lim)
}

fn read_response(stream: &mut TcpStream, lim: &Limits) -> Result<Response, FetchError> {
    let deadline = Instant::now() + RESPONSE_DEADLINE;
    let mut buf = Vec::new();
    let mut tmp = [0u8; READ_CHUNK];
    let head_end = loop {
        let prev = buf.len();
        if !read_more(stream, &mut buf, &mut tmp, deadline)? {
            return Err(FetchError::Http(HttpError::Incomplete));
        }
        let search_from = prev.saturating_sub(3);
        let tail = buf.get(search_from..).unwrap_or(&[]);
        if let Some(pos) = find_head_end(tail) {
            let end = search_from.saturating_add(pos);
            if end > lim.max_head {
                return Err(FetchError::Http(HttpError::HeadTooLarge));
            }
            break end;
        }
        if buf.len() > lim.max_head {
            return Err(FetchError::Http(HttpError::HeadTooLarge));
        }
    };

    match mercury::parse_response(&buf, lim, false) {
        Ok((resp, _)) => return Ok(resp),
        Err(HttpError::Incomplete) => {}
        Err(e) => return Err(FetchError::Http(e)),
    }

    let head_slice = buf
        .get(..head_end)
        .ok_or(FetchError::Http(HttpError::Incomplete))?;
    let (mut head_resp, _) = mercury::parse_response(head_slice, lim, true)?;
    if head_resp.header("transfer-encoding").is_some() {
        return read_chunked_response(stream, buf, &mut tmp, lim, deadline);
    }
    if let Some(cl_bytes) = head_resp.header("content-length") {
        let cl = parse_decimal(cl_bytes)?;
        if cl > lim.max_body {
            return Err(FetchError::Http(HttpError::BodyTooLarge));
        }
        let need = head_end.saturating_add(cl);
        while buf.len() < need {
            if !read_more(stream, &mut buf, &mut tmp, deadline)? {
                return Err(FetchError::Http(HttpError::Incomplete));
            }
        }
        let (resp, _) = mercury::parse_response(&buf, lim, false)?;
        return Ok(resp);
    }

    loop {
        if buf.len().saturating_sub(head_end) > lim.max_body {
            return Err(FetchError::Http(HttpError::BodyTooLarge));
        }
        if !read_more(stream, &mut buf, &mut tmp, deadline)? {
            break;
        }
    }
    if buf.len().saturating_sub(head_end) > lim.max_body {
        return Err(FetchError::Http(HttpError::BodyTooLarge));
    }
    head_resp.body = buf.get(head_end..).unwrap_or(&[]).to_vec();
    Ok(head_resp)
}

fn read_chunked_response(
    stream: &mut TcpStream,
    mut buf: Vec<u8>,
    tmp: &mut [u8],
    lim: &Limits,
    deadline: Instant,
) -> Result<Response, FetchError> {
    let max_wire = lim
        .max_head
        .saturating_add(lim.max_body.saturating_mul(4))
        .saturating_add(64 * 1024);
    let mut next_probe = buf.len().saturating_add(64 * 1024);
    loop {
        if buf.len() > max_wire {
            return Err(FetchError::Http(HttpError::BodyTooLarge));
        }
        let prev = buf.len();
        let got_more = read_more(stream, &mut buf, tmp, deadline)?;
        let search_from = prev.saturating_sub(4);
        let tail = buf.get(search_from..).unwrap_or(&[]);
        let saw_end =
            tail.windows(5).any(|w| w == b"\n0\r\n\r") || tail.windows(4).any(|w| w == b"\r\n\r\n");
        if !got_more || saw_end || buf.len() >= next_probe {
            match mercury::parse_response(&buf, lim, false) {
                Ok((resp, _)) => return Ok(resp),
                Err(HttpError::Incomplete) => {
                    if !got_more {
                        return Err(FetchError::Http(HttpError::Incomplete));
                    }
                    next_probe = buf.len().saturating_add(64 * 1024);
                }
                Err(e) => return Err(FetchError::Http(e)),
            }
        }
    }
}

fn read_more(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    tmp: &mut [u8],
    deadline: Instant,
) -> Result<bool, FetchError> {
    loop {
        if Instant::now() >= deadline {
            return Err(FetchError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "response deadline exceeded",
            )));
        }
        match stream.read(tmp) {
            Ok(0) => return Ok(false),
            Ok(n) => {
                if let Some(slice) = tmp.get(..n) {
                    buf.extend_from_slice(slice);
                }
                return Ok(true);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FetchError::Io(e)),
        }
    }
}

fn find_head_end(slice: &[u8]) -> Option<usize> {
    slice
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
}

fn parse_decimal(bytes: &[u8]) -> Result<usize, FetchError> {
    let s = std::str::from_utf8(bytes)
        .map_err(|_| FetchError::Http(HttpError::BadContentLength))?
        .trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FetchError::Http(HttpError::BadContentLength));
    }
    s.parse::<usize>()
        .map_err(|_| FetchError::Http(HttpError::BodyTooLarge))
}
