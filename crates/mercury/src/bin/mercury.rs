#![forbid(unsafe_code)]
//! Command-line tool for the `mercury` HTTP/1.1 parser, static file server, and client.
//!
//! Text mode prints the same lines as before. `--describe` prints the manifest, and `--json` prints one
//! ACI envelope (docs/ai-computer/ACI.md). Both modes share `fetch`, `parse_input` and `cmd_serve`.

use std::convert::Infallible;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use aci::envelope::{self, Code};
use aci::json::Json;
use aci::manifest::{Arg, Command, Effect, Kind as ArgKind, Manifest};
use mercury::server::{Config, Router, Server};
use mercury::{parse_request, parse_response, write_request, Error, Limits, Request, Response};

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

const fn req(name: &'static str, kind: ArgKind) -> Arg {
    Arg {
        name,
        kind,
        required: true,
        default: None,
        choices: &[],
    }
}

const fn opt(name: &'static str, kind: ArgKind, default: &'static str) -> Arg {
    Arg {
        name,
        kind,
        required: false,
        default: Some(default),
        choices: &[],
    }
}

const PARSE_ARGS: &[Arg] = &[req("FILE", ArgKind::Path)];
const SERVE_ARGS: &[Arg] = &[
    req("DIR", ArgKind::Path),
    opt("--listen", ArgKind::Text, "127.0.0.1:8080"),
];
const GET_ARGS: &[Arg] = &[
    req("URL", ArgKind::Text),
    Arg {
        name: "--out",
        kind: ArgKind::Path,
        required: false,
        default: None,
        choices: &[],
    },
];

const COMMANDS: &[Command] = &[
    Command {
        name: "parse",
        summary: "Parse a raw HTTP/1.x request or response from FILE (- for stdin)",
        effect: Effect::Read,
        args: PARSE_ARGS,
        returns: "kind, method, path, status (null where they do not apply), version, headers: [{name, text}], text (capped, with truncated, bytes, lossy when they apply)",
        example: "mercury --json parse request.http",
    },
    Command {
        name: "serve",
        summary: "Serve static files from DIR on --listen HOST:PORT; prints one envelope once bound, then runs until stopped",
        effect: Effect::Net,
        args: SERVE_ARGS,
        returns: "listen (the bound address), path (the served folder)",
        example: "mercury --json serve public --listen 127.0.0.1:0",
    },
    Command {
        name: "get",
        summary: "Fetch a plain http:// URL; --out FILE (--json only) writes the body to FILE instead of text",
        effect: Effect::Net,
        args: GET_ARGS,
        returns: "status, headers: [{name, text}], text (capped, with truncated, bytes, lossy when they apply); with --out: path, bytes",
        example: "mercury --json get http://127.0.0.1:8080/index.html",
    },
];

const MANIFEST: Manifest = Manifest {
    tool: "mercury",
    version: env!("CARGO_PKG_VERSION"),
    summary: "HTTP/1.1 parser, static file server and client",
    commands: COMMANDS,
};

/// A failed command: a usage error (exit 2) or an operation failure (exit 1) with its code and hint.
enum CmdError {
    Usage(String),
    Failed(Fail),
}

struct Fail {
    code: Code,
    message: String,
    hint: Option<String>,
}

fn failed(code: Code, message: String) -> CmdError {
    CmdError::Failed(Fail {
        code,
        message,
        hint: None,
    })
}

fn invalid(message: String) -> CmdError {
    failed(Code::Invalid, message)
}

/// A network failure; the hint names the address so the next step is clear.
fn io_at(message: String, target: &str) -> CmdError {
    CmdError::Failed(Fail {
        code: Code::Io,
        message,
        hint: Some(format!("check the server at {target}, or start one with: mercury --json serve DIR --listen {target}")),
    })
}

fn io_code(e: &io::Error) -> Code {
    if e.kind() == ErrorKind::NotFound {
        Code::NotFound
    } else {
        Code::Io
    }
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

/// A parsed message and how many bytes it used.
enum Message {
    Request(Request),
    Response(Response),
}

struct Parsed {
    msg: Message,
    used: usize,
}

fn main() -> ExitCode {
    let args: Result<Vec<String>, OsString> =
        env::args_os().skip(1).map(OsString::into_string).collect();
    let Ok(mut args) = args else {
        eprintln!("mercury: arguments must be valid UTF-8");
        return ExitCode::from(2);
    };
    let json = args.first().map(String::as_str) == Some("--json");
    if json {
        args.remove(0);
    }
    if args.len() == 1 && args[0] == "--describe" {
        println!("{}", MANIFEST.to_json().write());
        return ExitCode::SUCCESS;
    }
    if json {
        return run_json(&args);
    }
    match run_text(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CmdError::Failed(fail)) => {
            eprintln!("mercury: {}", fail.message);
            ExitCode::from(1)
        }
        Err(CmdError::Usage(msg)) => {
            eprintln!("mercury: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run_text(args: &[String]) -> Result<(), CmdError> {
    match parse_args(args)? {
        Subcommand::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Subcommand::Version => {
            println!("{VERSION}");
            Ok(())
        }
        Subcommand::Parse { file } => {
            let parsed = parse_input(&read_input(&file)?)?;
            print_parsed(&parsed);
            Ok(())
        }
        Subcommand::Serve { dir, listen } => match cmd_serve(&dir, &listen, false)? {},
        Subcommand::Get { url } => print_get(&fetch(&url)?),
    }
}

/// `mercury --json ...`: one envelope on stdout; stderr keeps the `mercury: message` line.
fn run_json(args: &[String]) -> ExitCode {
    let cmd = known_command(args.first());
    match json_command(args) {
        Ok(data) => {
            print!("{}", envelope::ok("mercury", cmd, data));
            ExitCode::SUCCESS
        }
        Err(CmdError::Usage(message)) => {
            eprintln!("mercury: {message}");
            let hint = Some("run: mercury --describe");
            print!(
                "{}",
                envelope::err("mercury", cmd, Code::Usage, &message, hint)
            );
            ExitCode::from(2)
        }
        Err(CmdError::Failed(fail)) => {
            eprintln!("mercury: {}", fail.message);
            let hint = fail.hint.as_deref();
            print!(
                "{}",
                envelope::err("mercury", cmd, fail.code, &fail.message, hint)
            );
            ExitCode::from(1)
        }
    }
}

/// The subcommand name for the envelope, or the empty string when the name is not one of ours.
fn known_command(first: Option<&String>) -> &'static str {
    const NAMES: [&str; 5] = ["parse", "serve", "get", "--help", "--version"];
    first
        .and_then(|a| NAMES.iter().copied().find(|n| *n == a.as_str()))
        .unwrap_or("")
}

/// Run one command under `--json` and return its data. `serve` prints its own envelope once bound.
fn json_command(args: &[String]) -> Result<Json, CmdError> {
    let mut args = args.to_vec();
    let out = take_out(&mut args)?;
    match parse_args(&args)? {
        Subcommand::Help => Ok(Json::obj([("usage", Json::Str(USAGE.into()))])),
        Subcommand::Version => Ok(Json::obj([("version", Json::Str(VERSION.into()))])),
        Subcommand::Parse { file } => Ok(parse_data(&parse_input(&read_input(&file)?)?)),
        Subcommand::Serve { dir, listen } => match cmd_serve(&dir, &listen, true)? {},
        Subcommand::Get { url } => get_data(&fetch(&url)?, out.as_deref()),
    }
}

/// Pull `--out FILE` out of `get` arguments. It is JSON-only, so in text mode it stays and fails as before.
fn take_out(args: &mut Vec<String>) -> Result<Option<PathBuf>, CmdError> {
    if args.first().map(String::as_str) != Some("get") {
        return Ok(None);
    }
    let Some(i) = args.iter().position(|a| a == "--out") else {
        return Ok(None);
    };
    let Some(value) = args.get(i + 1).cloned() else {
        return Err(CmdError::Usage("--out requires a file".to_string()));
    };
    args.drain(i..=i + 1);
    Ok(Some(PathBuf::from(value)))
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

/// Parse a request or a response; the text and JSON outputs both read this value.
fn parse_input(buf: &[u8]) -> Result<Parsed, CmdError> {
    let lim = Limits::default();
    let fail = |e: Error| invalid(format!("parse failed: {}", describe_error(&e)));
    if buf.starts_with(b"HTTP/") {
        let (resp, used) = parse_response(buf, &lim, false).map_err(fail)?;
        Ok(Parsed {
            msg: Message::Response(resp),
            used,
        })
    } else {
        let (req, used) = parse_request(buf, &lim).map_err(fail)?;
        Ok(Parsed {
            msg: Message::Request(req),
            used,
        })
    }
}

fn print_parsed(p: &Parsed) {
    match &p.msg {
        Message::Response(resp) => {
            println!("kind: response");
            println!("version: HTTP/1.{}", resp.minor);
            println!("status: {}", resp.status);
            println!("reason: {}", resp.reason);
            print_headers(&resp.headers);
            println!("bytes: {}", p.used);
            print_body(&resp.body);
        }
        Message::Request(req) => {
            println!("kind: request");
            println!("method: {}", req.method);
            println!("target: {}", req.target);
            println!("version: HTTP/1.{}", req.minor);
            print_headers(&req.headers);
            println!("bytes: {}", p.used);
            print_body(&req.body);
        }
    }
}

fn print_headers(headers: &[(String, Vec<u8>)]) {
    for (k, v) in headers {
        println!("header: {k}: {}", String::from_utf8_lossy(v));
    }
}

fn print_body(body: &[u8]) {
    println!("body-len: {}", body.len());
    if !body.is_empty() {
        println!("body: {}", String::from_utf8_lossy(body));
    }
}

fn parse_data(p: &Parsed) -> Json {
    let (kind, method, path, status, minor, headers, body) = match &p.msg {
        Message::Request(req) => (
            "request",
            Json::Str(req.method.clone()),
            Json::Str(req.target.clone()),
            Json::Null,
            req.minor,
            &req.headers,
            &req.body,
        ),
        Message::Response(resp) => (
            "response",
            Json::Null,
            Json::Null,
            Json::Int(i64::from(resp.status)),
            resp.minor,
            &resp.headers,
            &resp.body,
        ),
    };
    with_text(
        vec![
            ("kind", Json::Str(kind.into())),
            ("method", method),
            ("path", path),
            ("status", status),
            ("version", version_json(minor)),
            ("headers", headers_json(headers)),
        ],
        body,
    )
}

fn read_input(file: &str) -> Result<Vec<u8>, CmdError> {
    if file == "-" {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| failed(Code::Io, format!("cannot read stdin: {e}")))?;
        Ok(buf)
    } else {
        fs::read(file).map_err(|e| failed(io_code(&e), format!("cannot read '{file}': {e}")))
    }
}

/// Serve DIR. Prints the listening address (text) or one envelope (JSON) once bound, then never returns.
fn cmd_serve(dir: &str, listen: &str, json: bool) -> Result<Infallible, CmdError> {
    let meta = fs::metadata(dir)
        .map_err(|e| failed(io_code(&e), format!("cannot access '{dir}': {e}")))?;
    if !meta.is_dir() {
        return Err(invalid(format!("not a directory: '{dir}'")));
    }

    let mut router = Router::new();
    router.static_dir("/", PathBuf::from(dir));

    let cfg = Config {
        addr: listen.to_string(),
        ..Config::default()
    };
    let server = Server::start(cfg, router).map_err(|e| {
        CmdError::Failed(Fail {
            code: Code::Io,
            message: format!("cannot listen on '{listen}': {e}"),
            hint: Some(format!(
                "nothing is bound to {listen}; choose a free address such as 127.0.0.1:0"
            )),
        })
    })?;

    let addr = server.local_addr();
    if json {
        let data = Json::obj([
            ("path", Json::path(Path::new(dir))),
            ("listen", Json::Str(addr.to_string())),
        ]);
        print!("{}", envelope::ok("mercury", "serve", data));
    } else {
        println!("listening: {addr}");
    }
    let _ = io::stdout().flush();

    loop {
        thread::park();
    }
}

/// Fetch a plain HTTP URL and return the whole response.
fn fetch(url: &str) -> Result<Response, CmdError> {
    let parsed = parse_http_url(url)?;
    let target = if parsed.connect_host.contains(':') {
        format!("[{}]:{}", parsed.connect_host, parsed.port)
    } else {
        format!("{}:{}", parsed.connect_host, parsed.port)
    };
    let addrs: Vec<SocketAddr> = (parsed.connect_host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|e| io_at(format!("resolve failed: {e}"), &target))?
        .collect();
    if addrs.is_empty() {
        return Err(io_at("resolve returned no addresses".to_string(), &target));
    }

    let mut stream = connect_any(&addrs, &target)?;
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
        .map_err(|e| io_at(format!("write failed: {e}"), &target))?;
    let _ = stream.flush();

    let lim = Limits::default();
    let max_total = lim.max_head.saturating_add(lim.max_body);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];

    let resp = loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                let (resp, _) = parse_response(&buf, &lim, false)
                    .map_err(|e| invalid(format!("invalid response: {}", describe_error(&e))))?;
                break resp;
            }
            Ok(n) => {
                if buf.len().saturating_add(n) > max_total {
                    return Err(invalid("response exceeds size limit".to_string()));
                }
                buf.extend_from_slice(&chunk[..n]);
                match parse_response(&buf, &lim, false) {
                    Ok((resp, _)) => break resp,
                    Err(Error::Incomplete) => continue,
                    Err(e) => {
                        return Err(invalid(format!("invalid response: {}", describe_error(&e))));
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_at(format!("read failed: {e}"), &target)),
        }
    };
    Ok(resp)
}

fn print_get(resp: &Response) -> Result<(), CmdError> {
    if resp.reason.is_empty() {
        println!("status: {}", resp.status);
    } else {
        println!("status: {} {}", resp.status, resp.reason);
    }
    print_headers(&resp.headers);
    println!();
    let mut out = io::stdout().lock();
    out.write_all(&resp.body)
        .map_err(|e| failed(Code::Io, format!("stdout write failed: {e}")))?;
    if !resp.body.is_empty() && !resp.body.ends_with(b"\n") {
        out.write_all(b"\n")
            .map_err(|e| failed(Code::Io, format!("stdout write failed: {e}")))?;
    }
    Ok(())
}

/// The data of `get --json`: the body as capped text, or written to `out` with its path and size.
fn get_data(resp: &Response, out: Option<&Path>) -> Result<Json, CmdError> {
    let mut pairs = vec![
        ("status", Json::Int(i64::from(resp.status))),
        ("headers", headers_json(&resp.headers)),
    ];
    match out {
        Some(path) => {
            fs::write(path, &resp.body)
                .map_err(|e| failed(Code::Io, format!("cannot write '{}': {e}", path.display())))?;
            pairs.push(("path", Json::path(path)));
            pairs.push(("bytes", int(resp.body.len())));
            Ok(Json::Obj(own(pairs)))
        }
        None => Ok(with_text(pairs, &resp.body)),
    }
}

fn connect_any(addrs: &[SocketAddr], target: &str) -> Result<TcpStream, CmdError> {
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(addr, IO_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => Err(io_at(format!("connect failed: {e}"), target)),
        None => Err(io_at("connect failed".to_string(), target)),
    }
}

fn parse_http_url(url: &str) -> Result<ParsedUrl, CmdError> {
    if url
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        return Err(CmdError::Failed(Fail {
            code: Code::Unsupported,
            message: "https not supported".to_string(),
            hint: Some("https is not built yet; use an http:// URL".to_string()),
        }));
    }
    let Some(prefix) = url.get(..7) else {
        return Err(invalid(format!(
            "invalid URL '{url}': expected http://HOST[:PORT]/PATH"
        )));
    };
    if !prefix.eq_ignore_ascii_case("http://") {
        return Err(invalid(format!(
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
        return Err(invalid(format!("invalid host in URL '{url}'")));
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
        return Err(invalid(format!("invalid path in URL '{url}'")));
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
            return Err(invalid(format!("invalid IPv6 host in URL '{url}'")));
        };
        let host = &after_open[..close];
        if host.is_empty() || host.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(invalid(format!("invalid IPv6 host in URL '{url}'")));
        }
        let suffix = &after_open[close + 1..];
        if suffix.is_empty() {
            return Ok((host.to_string(), 80, format!("[{host}]")));
        }
        let Some(port_str) = suffix.strip_prefix(':') else {
            return Err(invalid(format!("invalid authority in URL '{url}'")));
        };
        let port = parse_port(port_str, url)?;
        Ok((host.to_string(), port, format!("[{host}]:{port}")))
    } else if let Some((host, port_str)) = authority.split_once(':') {
        if host.is_empty() || host.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(invalid(format!("invalid host in URL '{url}'")));
        }
        let port = parse_port(port_str, url)?;
        Ok((host.to_string(), port, format!("{host}:{port}")))
    } else {
        if authority.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(invalid(format!("invalid host in URL '{url}'")));
        }
        Ok((authority.to_string(), 80, authority.to_string()))
    }
}

fn parse_port(s: &str, url: &str) -> Result<u16, CmdError> {
    let port: u16 = s
        .parse()
        .map_err(|_| invalid(format!("invalid port in URL '{url}'")))?;
    if port == 0 {
        return Err(invalid(format!("invalid port 0 in URL '{url}'")));
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
        Error::AmbiguousFraming => "both Transfer-Encoding and Content-Length present",
        Error::BadMethod => "invalid HTTP method",
    }
}

fn version_json(minor: u8) -> Json {
    Json::Str(format!("HTTP/1.{minor}"))
}

/// Headers in order, each as `{name, text}`; values are read as text with invalid bytes replaced.
fn headers_json(headers: &[(String, Vec<u8>)]) -> Json {
    Json::Arr(
        headers
            .iter()
            .map(|(name, value)| {
                Json::obj([
                    ("name", Json::Str(name.clone())),
                    (
                        "text",
                        Json::Str(String::from_utf8_lossy(value).into_owned()),
                    ),
                ])
            })
            .collect(),
    )
}

/// An object from literal pairs, with the body added as capped text fields.
fn with_text(pairs: Vec<(&str, Json)>, body: &[u8]) -> Json {
    let mut fields = own(pairs);
    fields.extend(envelope::capped(body));
    Json::Obj(fields)
}

fn own(pairs: Vec<(&str, Json)>) -> Vec<(String, Json)> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn int(n: usize) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}
