#![forbid(unsafe_code)]
//! Command-line tool for the `mercury` HTTP/1.1 parser, static file server, and client.

use std::env;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use mercury::server::{Config, Router, Server};
use mercury::{parse_request, parse_response, write_request, Error, Limits, Request};

const VERSION: &str = "mercury 0.1.0";
const DEFAULT_LISTEN: &str = "127.0.0.1:8080";
const IO_TIMEOUT: Duration = Duration::from_secs(10);

const USAGE: &str = "\
mercury 0.1.0 - HTTP/1.1 parser, static file server, and client

USAGE:
  mercury <SUBCOMMAND> [ARGS]

SUBCOMMANDS:
  parse FILE                                Parse a raw HTTP/1.x request or response (- for stdin)
  serve DIR [--listen 127.0.0.1:8080]       Serve static files from DIR (Ctrl+C stops)
  get http://HOST[:PORT]/PATH               Fetch a plain HTTP URL

OPTIONS:
  -h, --help       Print help information
  -V, --version    Print version information

EXAMPLES:
  mercury parse request.http
  mercury serve ./public --listen 127.0.0.1:8080
  mercury get http://127.0.0.1:8080/index.html";

enum CmdError {
    Usage(String),
    Failed(String),
}

enum Subcommand {
    Help,
    Version,
    Parse { file: String },
    Serve { dir: String, listen: String },
    Get { url: String },
}

struct ParsedUrl {
    connect_host: String,
    port: u16,
    host_header: String,
    path: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CmdError::Failed(msg)) => {
            eprintln!("mercury: {msg}");
            ExitCode::from(1)
        }
        Err(CmdError::Usage(msg)) => {
            eprintln!("mercury: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), CmdError> {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_args(&args)? {
        Subcommand::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Subcommand::Version => {
            println!("{VERSION}");
            Ok(())
        }
        Subcommand::Parse { file } => cmd_parse(&file),
        Subcommand::Serve { dir, listen } => cmd_serve(&dir, &listen),
        Subcommand::Get { url } => cmd_get(&url),
    }
}

fn parse_args(args: &[String]) -> Result<Subcommand, CmdError> {
    let Some(first) = args.first() else {
        return Err(CmdError::Usage(
            "missing subcommand (try --help)".to_string(),
        ));
    };
    match first.as_str() {
        "-h" | "--help" => Ok(Subcommand::Help),
        "-V" | "--version" => Ok(Subcommand::Version),
        "parse" => parse_parse_args(&args[1..]),
        "serve" => parse_serve_args(&args[1..]),
        "get" => parse_get_args(&args[1..]),
        other if other.starts_with('-') => {
            Err(CmdError::Usage(format!("unknown option '{other}'")))
        }
        other => Err(CmdError::Usage(format!(
            "unknown subcommand '{other}' (try --help)"
        ))),
    }
}

fn parse_parse_args(args: &[String]) -> Result<Subcommand, CmdError> {
    let mut file: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            s if s != "-" && s.starts_with('-') => {
                return Err(CmdError::Usage(format!("unknown option '{s}'")));
            }
            _ if file.is_none() => file = Some(arg.clone()),
            other => {
                return Err(CmdError::Usage(format!("unexpected argument '{other}'")));
            }
        }
    }
    let file = file.ok_or_else(|| CmdError::Usage("parse requires FILE".to_string()))?;
    Ok(Subcommand::Parse { file })
}

fn parse_serve_args(args: &[String]) -> Result<Subcommand, CmdError> {
    let mut dir: Option<String> = None;
    let mut listen: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            "--listen" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err(CmdError::Usage("--listen requires an address".to_string()));
                };
                listen = Some(val.clone());
            }
            s if s.starts_with("--listen=") => {
                let val = s.strip_prefix("--listen=").unwrap_or("");
                if val.is_empty() {
                    return Err(CmdError::Usage("--listen requires an address".to_string()));
                }
                listen = Some(val.to_string());
            }
            s if s.starts_with('-') => {
                return Err(CmdError::Usage(format!("unknown option '{s}'")));
            }
            _ if dir.is_none() => dir = Some(arg.clone()),
            other => {
                return Err(CmdError::Usage(format!("unexpected argument '{other}'")));
            }
        }
        i += 1;
    }
    let dir = dir.ok_or_else(|| CmdError::Usage("serve requires DIR".to_string()))?;
    Ok(Subcommand::Serve {
        dir,
        listen: listen.unwrap_or_else(|| DEFAULT_LISTEN.to_string()),
    })
}

fn parse_get_args(args: &[String]) -> Result<Subcommand, CmdError> {
    let mut url: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            s if s.starts_with('-') => {
                return Err(CmdError::Usage(format!("unknown option '{s}'")));
            }
            _ if url.is_none() => url = Some(arg.clone()),
            other => {
                return Err(CmdError::Usage(format!("unexpected argument '{other}'")));
            }
        }
    }
    let url = url.ok_or_else(|| CmdError::Usage("get requires URL".to_string()))?;
    Ok(Subcommand::Get { url })
}

fn cmd_parse(file: &str) -> Result<(), CmdError> {
    let buf = read_input(file)?;
    let lim = Limits::default();
    if buf.starts_with(b"HTTP/") {
        let (resp, used) = parse_response(&buf, &lim, false)
            .map_err(|e| CmdError::Failed(format!("parse failed: {}", describe_error(&e))))?;
        println!("kind: response");
        println!("version: HTTP/1.{}", resp.minor);
        println!("status: {}", resp.status);
        println!("reason: {}", resp.reason);
        for (k, v) in &resp.headers {
            println!("header: {k}: {}", String::from_utf8_lossy(v));
        }
        println!("bytes: {used}");
        println!("body-len: {}", resp.body.len());
        if !resp.body.is_empty() {
            println!("body: {}", String::from_utf8_lossy(&resp.body));
        }
    } else {
        let (req, used) = parse_request(&buf, &lim)
            .map_err(|e| CmdError::Failed(format!("parse failed: {}", describe_error(&e))))?;
        println!("kind: request");
        println!("method: {}", req.method);
        println!("target: {}", req.target);
        println!("version: HTTP/1.{}", req.minor);
        for (k, v) in &req.headers {
            println!("header: {k}: {}", String::from_utf8_lossy(v));
        }
        println!("bytes: {used}");
        println!("body-len: {}", req.body.len());
        if !req.body.is_empty() {
            println!("body: {}", String::from_utf8_lossy(&req.body));
        }
    }
    Ok(())
}

fn read_input(file: &str) -> Result<Vec<u8>, CmdError> {
    if file == "-" {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| CmdError::Failed(format!("cannot read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read(file).map_err(|e| CmdError::Failed(format!("cannot read '{file}': {e}")))
    }
}

fn cmd_serve(dir: &str, listen: &str) -> Result<(), CmdError> {
    let meta =
        fs::metadata(dir).map_err(|e| CmdError::Failed(format!("cannot access '{dir}': {e}")))?;
    if !meta.is_dir() {
        return Err(CmdError::Failed(format!("not a directory: '{dir}'")));
    }

    let mut router = Router::new();
    router.static_dir("/", PathBuf::from(dir));

    let cfg = Config {
        addr: listen.to_string(),
        ..Config::default()
    };
    let server = Server::start(cfg, router)
        .map_err(|e| CmdError::Failed(format!("cannot listen on '{listen}': {e}")))?;

    println!("listening: {}", server.local_addr());
    let _ = io::stdout().flush();

    loop {
        thread::park();
    }
}

fn cmd_get(url: &str) -> Result<(), CmdError> {
    let parsed = parse_http_url(url)?;
    let addrs: Vec<SocketAddr> = (parsed.connect_host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|e| CmdError::Failed(format!("resolve failed: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(CmdError::Failed(
            "resolve returned no addresses".to_string(),
        ));
    }

    let mut stream = connect_any(&addrs)?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    let req = Request {
        method: "GET".to_string(),
        target: parsed.path,
        minor: 1,
        headers: vec![
            ("Host".to_string(), parsed.host_header.into_bytes()),
            ("Connection".to_string(), b"close".to_vec()),
            ("User-Agent".to_string(), b"mercury/0.1.0".to_vec()),
        ],
        body: Vec::new(),
    };
    let wire = write_request(&req);
    stream
        .write_all(&wire)
        .map_err(|e| CmdError::Failed(format!("write failed: {e}")))?;
    let _ = stream.flush();

    let lim = Limits::default();
    let max_total = lim.max_head.saturating_add(lim.max_body);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];

    let resp = loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                let (resp, _) = parse_response(&buf, &lim, false).map_err(|e| {
                    CmdError::Failed(format!("invalid response: {}", describe_error(&e)))
                })?;
                break resp;
            }
            Ok(n) => {
                if buf.len().saturating_add(n) > max_total {
                    return Err(CmdError::Failed("response exceeds size limit".to_string()));
                }
                buf.extend_from_slice(&chunk[..n]);
                match parse_response(&buf, &lim, false) {
                    Ok((resp, _)) => break resp,
                    Err(Error::Incomplete) => continue,
                    Err(e) => {
                        return Err(CmdError::Failed(format!(
                            "invalid response: {}",
                            describe_error(&e)
                        )));
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(CmdError::Failed(format!("read failed: {e}"))),
        }
    };

    if resp.reason.is_empty() {
        println!("status: {}", resp.status);
    } else {
        println!("status: {} {}", resp.status, resp.reason);
    }
    for (k, v) in &resp.headers {
        println!("header: {k}: {}", String::from_utf8_lossy(v));
    }
    println!();
    let mut out = io::stdout().lock();
    out.write_all(&resp.body)
        .map_err(|e| CmdError::Failed(format!("stdout write failed: {e}")))?;
    if !resp.body.is_empty() && !resp.body.ends_with(b"\n") {
        out.write_all(b"\n")
            .map_err(|e| CmdError::Failed(format!("stdout write failed: {e}")))?;
    }
    Ok(())
}

fn connect_any(addrs: &[SocketAddr]) -> Result<TcpStream, CmdError> {
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(addr, IO_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => Err(CmdError::Failed(format!("connect failed: {e}"))),
        None => Err(CmdError::Failed("connect failed".to_string())),
    }
}

fn parse_http_url(url: &str) -> Result<ParsedUrl, CmdError> {
    if url
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        return Err(CmdError::Failed("https not supported".to_string()));
    }
    let Some(prefix) = url.get(..7) else {
        return Err(CmdError::Failed(format!(
            "invalid URL '{url}': expected http://HOST[:PORT]/PATH"
        )));
    };
    if !prefix.eq_ignore_ascii_case("http://") {
        return Err(CmdError::Failed(format!(
            "invalid URL '{url}': expected http://HOST[:PORT]/PATH"
        )));
    }
    let rest = &url[7..];
    let split_idx = rest.find(['/', '?', '#']);
    let (authority, raw_tail) = match split_idx {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };

    if authority.is_empty() || authority.contains('@') {
        return Err(CmdError::Failed(format!("invalid host in URL '{url}'")));
    }

    let (connect_host, port, host_header) = parse_authority(authority, url)?;
    let no_frag = raw_tail.split('#').next().unwrap_or("");
    let path = if no_frag.is_empty() {
        "/".to_string()
    } else if no_frag.starts_with('/') {
        no_frag.to_string()
    } else {
        format!("/{no_frag}")
    };

    if path.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(CmdError::Failed(format!("invalid path in URL '{url}'")));
    }

    Ok(ParsedUrl {
        connect_host,
        port,
        host_header,
        path,
    })
}

fn parse_authority(authority: &str, url: &str) -> Result<(String, u16, String), CmdError> {
    if let Some(after_open) = authority.strip_prefix('[') {
        let Some(close) = after_open.find(']') else {
            return Err(CmdError::Failed(format!(
                "invalid IPv6 host in URL '{url}'"
            )));
        };
        let host = &after_open[..close];
        if host.is_empty() || host.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(CmdError::Failed(format!(
                "invalid IPv6 host in URL '{url}'"
            )));
        }
        let suffix = &after_open[close + 1..];
        if suffix.is_empty() {
            return Ok((host.to_string(), 80, format!("[{host}]")));
        }
        let Some(port_str) = suffix.strip_prefix(':') else {
            return Err(CmdError::Failed(format!(
                "invalid authority in URL '{url}'"
            )));
        };
        let port = parse_port(port_str, url)?;
        Ok((host.to_string(), port, format!("[{host}]:{port}")))
    } else if let Some((host, port_str)) = authority.split_once(':') {
        if host.is_empty() || host.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(CmdError::Failed(format!("invalid host in URL '{url}'")));
        }
        let port = parse_port(port_str, url)?;
        Ok((host.to_string(), port, format!("{host}:{port}")))
    } else {
        if authority.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(CmdError::Failed(format!("invalid host in URL '{url}'")));
        }
        Ok((authority.to_string(), 80, authority.to_string()))
    }
}

fn parse_port(s: &str, url: &str) -> Result<u16, CmdError> {
    let port: u16 = s
        .parse()
        .map_err(|_| CmdError::Failed(format!("invalid port in URL '{url}'")))?;
    if port == 0 {
        return Err(CmdError::Failed(format!("invalid port 0 in URL '{url}'")));
    }
    Ok(port)
}

fn describe_error(err: &Error) -> &'static str {
    match err {
        Error::Incomplete => "incomplete message",
        Error::BadRequestLine => "malformed request line",
        Error::BadStatusLine => "malformed status line",
        Error::BadVersion => "unsupported HTTP version",
        Error::BadHeader => "malformed header",
        Error::HeadTooLarge => "message head too large",
        Error::TooManyHeaders => "too many headers",
        Error::BodyTooLarge => "message body too large",
        Error::BadChunk => "malformed chunked body",
        Error::BadContentLength => "malformed or conflicting Content-Length",
        Error::BadMethod => "invalid HTTP method",
    }
}
