//! `athena serve`: the packed Athena web app on 127.0.0.1, with a file API scoped to one folder.
//!
//! Mercury serves the web bundle from its directory and the `/api/fs/*` routes. Each API request
//! must come from a local host name and carry the per-run token in `X-Athena-Token`; the page
//! gets the token from the `?t=` part of the URL that `athena serve` prints.

use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mercury::server::{Config, Router, Server};
use mercury::{Limits, Request, Response};

use crate::fsscope::{self, Denied, MAX_FILE_BYTES};

/// Port used when `--port` is not given.
pub const DEFAULT_PORT: u16 = 8480;
/// Folder holding the packed web app, relative to the working directory.
pub const DEFAULT_WEB: &str = "target/athena-web";
/// Request header that carries the token on every `/api/fs` call.
pub const TOKEN_HEADER: &str = "x-athena-token";

/// Options for `athena serve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeArgs {
    /// Folder to open (default: the current folder).
    pub folder: String,
    /// Port on 127.0.0.1.
    pub port: u16,
    /// Folder holding the packed web app, when `--web` was given.
    pub web: Option<String>,
}

/// Parse the arguments after `serve`. `Ok(None)` means help was asked for.
pub fn parse_serve_args(args: &[String]) -> Result<Option<ServeArgs>, String> {
    let mut out = ServeArgs {
        folder: String::from("."),
        port: DEFAULT_PORT,
        web: None,
    };
    let mut folder: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--port" => {
                let value = it.next().ok_or("--port needs a number")?;
                out.port = value.parse().map_err(|_| format!("bad port '{value}'"))?;
            }
            "--web" => out.web = Some(it.next().ok_or("--web needs a folder")?.clone()),
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => {
                if folder.is_some() {
                    return Err(format!("unexpected argument '{other}'"));
                }
                folder = Some(other.to_string());
            }
        }
    }
    if let Some(f) = folder {
        out.folder = f;
    }
    Ok(Some(out))
}

/// A running `athena serve`: the server, the URL to open and the token the page must send.
pub struct Served {
    /// The running HTTP server. Stop it with `Server::stop`.
    pub server: Server,
    /// The address to open, with the token in `?t=`.
    pub url: String,
    /// The per-run token that every `/api/fs` request must carry.
    pub token: String,
}

struct State {
    root: PathBuf,
    token: String,
}

type Op = fn(&State, &Request) -> Result<Response, Denied>;

/// Where the packed web app may be, in the order `athena serve` looks: `--web`, then the install
/// layout beside the binary, then `target/athena-web` under the working directory.
pub fn web_candidates(explicit: Option<&str>, exe: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = explicit.map(PathBuf::from).into_iter().collect();
    if let Some(bin_dir) = exe.and_then(Path::parent) {
        out.push(bin_dir.join("..").join("share").join("athena-web"));
    }
    out.push(PathBuf::from(DEFAULT_WEB));
    out
}

/// The first candidate that holds an `index.html`, or an error that names every place tried.
pub fn find_web(candidates: &[PathBuf]) -> Result<PathBuf, String> {
    if let Some(found) = candidates.iter().find(|c| c.join("index.html").is_file()) {
        return Ok(found.clone());
    }
    let looked: Vec<String> = candidates.iter().map(|c| c.display().to_string()).collect();
    Err(format!(
        "no web app; looked in: {} (install with tools/install.sh, or build it with: cargo run -q --release -p athena --example pack)",
        looked.join(", ")
    ))
}

/// Serve `folder` through the API and `web` as static files on `127.0.0.1:port`. Port 0 picks a
/// free port, which tests use.
pub fn start(folder: &Path, web: &Path, port: u16) -> Result<Served, String> {
    let root = fs::canonicalize(folder).map_err(|_| not_a_folder(folder))?;
    if !root.is_dir() {
        return Err(not_a_folder(folder));
    }
    if !web.join("index.html").is_file() {
        return Err(format!("no web app in {}", web.display()));
    }
    let token = new_token();
    let state = Arc::new(State {
        root,
        token: token.clone(),
    });

    let mut router = Router::new();
    api_route(&mut router, &state, "GET", "/api/fs/list", op_list);
    api_route(&mut router, &state, "GET", "/api/fs/read", op_read);
    api_route(&mut router, &state, "PUT", "/api/fs/write", op_write);
    api_route(&mut router, &state, "POST", "/api/fs/create", op_create);
    api_route(&mut router, &state, "POST", "/api/fs/rename", op_rename);
    api_route(&mut router, &state, "POST", "/api/fs/trash", op_trash);
    router.static_dir("", web.to_path_buf());

    let addr = format!("127.0.0.1:{port}");
    let cfg = Config {
        addr: addr.clone(),
        limits: Limits {
            max_body: MAX_FILE_BYTES,
            ..Limits::default()
        },
        ..Config::default()
    };
    let server = Server::start(cfg, router).map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    let url = format!("http://127.0.0.1:{}/?t={token}", server.local_addr().port());
    Ok(Served { server, url, token })
}

fn not_a_folder(folder: &Path) -> String {
    format!("not a folder: {}", folder.display())
}

/// A 256-bit token from SipHash keys that the OS seeds at random. Shortcut: not a CSPRNG, so it
/// rests on SipHash being unpredictable under an unknown key; upgrade to the OS random source.
fn new_token() -> String {
    let mut out = String::with_capacity(64);
    for i in 0u64..4 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(i);
        let _ = write!(out, "{:016x}", hasher.finish());
    }
    out
}

fn api_route(router: &mut Router, state: &Arc<State>, method: &str, path: &str, op: Op) {
    let state = Arc::clone(state);
    router.route(
        method,
        path,
        Box::new(move |req: &Request| {
            match authorise(&state, req).and_then(|()| op(&state, req)) {
                Ok(resp) => resp,
                Err(denied) => reply_json(
                    denied.status,
                    format!("{{\"error\":{}}}", json_string(&denied.message)),
                ),
            }
        }),
    );
}

fn authorise(state: &State, req: &Request) -> Result<(), Denied> {
    let host = req
        .header("host")
        .and_then(|h| std::str::from_utf8(h).ok())
        .unwrap_or("");
    let host_name = host.split(':').next().unwrap_or("");
    if host_name != "127.0.0.1" && host_name != "localhost" {
        return Err(Denied::new(403, "only local host names may call the API"));
    }
    let given = req
        .header(TOKEN_HEADER)
        .and_then(|v| std::str::from_utf8(v).ok())
        .unwrap_or("");
    if !same_token(given, &state.token) {
        return Err(Denied::new(401, "missing or wrong token"));
    }
    Ok(())
}

/// Compare without stopping at the first differing byte.
fn same_token(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn op_list(state: &State, req: &Request) -> Result<Response, Denied> {
    let rel = param(req, "path")?.unwrap_or_default();
    let entries = fsscope::list(&state.root, &rel)?;
    let mut json = String::from("[");
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let kind = if entry.is_dir { "dir" } else { "file" };
        let _ = write!(
            json,
            "{{\"name\":{},\"kind\":\"{kind}\",\"size\":{}}}",
            json_string(&entry.name),
            entry.size
        );
    }
    json.push(']');
    Ok(reply_json(200, json))
}

fn op_read(state: &State, req: &Request) -> Result<Response, Denied> {
    let rel = param(req, "path")?.unwrap_or_default();
    let text = fsscope::read_text(&state.root, &rel)?;
    Ok(reply(200, "text/plain; charset=utf-8", text.into_bytes()))
}

fn op_write(state: &State, req: &Request) -> Result<Response, Denied> {
    let rel = required(req, "path")?;
    fsscope::write_file(&state.root, &rel, &req.body)?;
    Ok(reply_json(200, String::from("{\"ok\":true}")))
}

fn op_create(state: &State, req: &Request) -> Result<Response, Denied> {
    let rel = required(req, "path")?;
    let is_dir = match param(req, "kind")?.as_deref() {
        None | Some("file") => false,
        Some("dir") => true,
        Some(_) => return Err(Denied::new(400, "kind must be file or dir")),
    };
    fsscope::create(&state.root, &rel, is_dir)?;
    Ok(reply_json(200, String::from("{\"ok\":true}")))
}

fn op_rename(state: &State, req: &Request) -> Result<Response, Denied> {
    let from = required(req, "from")?;
    let to = required(req, "to")?;
    fsscope::rename(&state.root, &from, &to)?;
    Ok(reply_json(200, String::from("{\"ok\":true}")))
}

fn op_trash(state: &State, req: &Request) -> Result<Response, Denied> {
    let rel = required(req, "path")?;
    let entry = fsscope::trash(&state.root, &rel)?;
    Ok(reply_json(
        200,
        format!("{{\"trashed\":{}}}", json_string(&entry)),
    ))
}

/// Value of query parameter `key`, percent-decoded. A missing key is `None`.
fn param(req: &Request, key: &str) -> Result<Option<String>, Denied> {
    let Some((_, query)) = req.target.split_once('?') else {
        return Ok(None);
    };
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == key {
            return percent_decode(value)
                .map(Some)
                .ok_or_else(|| Denied::new(400, format!("bad percent-encoding in {key}")));
        }
    }
    Ok(None)
}

fn required(req: &Request, key: &str) -> Result<String, Denied> {
    param(req, key)?.ok_or_else(|| Denied::new(400, format!("missing {key}")))
}

/// Decode `%XX` escapes as UTF-8. `+` stays a plus sign, because paths are not form data.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A JSON string literal with the characters JSON requires escaped.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn reply_json(status: u16, body: String) -> Response {
    reply(status, "application/json; charset=utf-8", body.into_bytes())
}

fn reply(status: u16, content_type: &str, body: Vec<u8>) -> Response {
    Response {
        minor: 1,
        status,
        reason: reason_phrase(status).to_string(),
        headers: vec![
            ("Content-Type".to_string(), content_type.as_bytes().to_vec()),
            ("Cache-Control".to_string(), b"no-store".to_vec()),
        ],
        body,
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_escape_quotes_slashes_and_controls() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000a\"");
    }

    #[test]
    fn percent_decoding_accepts_utf8_and_rejects_broken_escapes() {
        assert_eq!(
            percent_decode("src%2Fa%20b.logos").unwrap(),
            "src/a b.logos"
        );
        assert_eq!(percent_decode("a+b").unwrap(), "a+b");
        assert_eq!(percent_decode("%E2%82%AC").unwrap(), "\u{20ac}");
        assert!(percent_decode("%").is_none());
        assert!(percent_decode("%zz").is_none());
        assert!(percent_decode("%FF").is_none());
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abc", "abd"));
        assert!(!same_token("", "abc"));
        assert!(!same_token("abcd", "abc"));
    }

    #[test]
    fn serve_arguments_default_and_reject_bad_input() {
        let a = parse_serve_args(&[]).unwrap().unwrap();
        assert_eq!(a.port, 8480);
        assert_eq!(a.folder, ".");
        let args: Vec<String> = ["notes", "--port", "9000", "--web", "w"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let b = parse_serve_args(&args).unwrap().unwrap();
        assert_eq!(
            (b.folder.as_str(), b.port, b.web.as_deref()),
            ("notes", 9000, Some("w"))
        );
        assert!(parse_serve_args(&["--port".to_string(), "70000".to_string()]).is_err());
        assert!(parse_serve_args(&["--host".to_string(), "0.0.0.0".to_string()]).is_err());
        assert!(parse_serve_args(&["a".to_string(), "b".to_string()]).is_err());
        assert!(parse_serve_args(&["--help".to_string()]).unwrap().is_none());
    }

    #[test]
    fn web_dir_is_searched_in_order_and_errors_list_every_place() {
        let base = std::env::temp_dir().join(format!("athena_web_order_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let explicit = base.join("explicit");
        let bin = base.join("bin").join("athena.exe");
        let install = base.join("share").join("athena-web");
        for dir in [&explicit, &install] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join("index.html"), "x").unwrap();
        }
        let list = web_candidates(explicit.to_str(), Some(&bin));
        assert_eq!(list[0], explicit);
        assert_eq!(
            list[1],
            base.join("bin").join("..").join("share").join("athena-web")
        );
        assert_eq!(list.len(), 3);
        assert_eq!(find_web(&list).unwrap(), explicit);
        // Without --web the install layout wins over the working folder.
        let no_flag = web_candidates(None, Some(&bin));
        assert_eq!(find_web(&no_flag).unwrap(), no_flag[0]);
        let err = find_web(&[base.join("missing-a"), base.join("missing-b")]).unwrap_err();
        assert!(
            err.contains("missing-a") && err.contains("missing-b"),
            "{err}"
        );
        let _ = fs::remove_dir_all(&base);
    }
}
