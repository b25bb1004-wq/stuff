//! Request routing and `404`/`405` dispatch for [`crate::server::Server`].

use std::boxed::Box;
use std::path::PathBuf;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use super::static_files::{not_found, serve_static};
use crate::types::trim_ows;
use crate::ws::{accept_key, base64_decode, WebSocket};
use crate::{Request, Response};

/// Request handler callback registered with [`Router::route`].
pub type Handler = Box<dyn Fn(&Request) -> Response + Send + Sync>;

/// Streaming response handler callback registered with [`Router::stream`].
pub(crate) type StreamHandler =
    Box<dyn Fn(&Request, &mut dyn FnMut(&[u8]) -> std::io::Result<()>) -> u16 + Send + Sync>;

/// WebSocket handler callback registered with [`Router::websocket`].
pub(crate) type WsHandler = Box<dyn Fn(WebSocket) + Send + Sync>;

enum RouteKind {
    Normal(Handler),
    Stream(StreamHandler),
    WebSocket(WsHandler),
}

pub(crate) enum Dispatch<'a> {
    Response(Response),
    Stream(&'a StreamHandler),
    WebSocket(&'a WsHandler, String),
}

struct ExactRoute {
    method: String,
    path: String,
    kind: RouteKind,
}

struct PrefixRoute {
    method: String,
    prefix: String,
    kind: RouteKind,
}

struct StaticMount {
    base: String,
    dir: PathBuf,
}

/// HTTP request router mapping method + path patterns and static directories to handlers.
pub struct Router {
    exact: Vec<ExactRoute>,
    prefixes: Vec<PrefixRoute>,
    statics: Vec<StaticMount>,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    /// Create an empty router.
    pub fn new() -> Router {
        Router {
            exact: Vec::new(),
            prefixes: Vec::new(),
            statics: Vec::new(),
        }
    }

    /// Register a handler for `method` and `path` (exact path, or prefix ending in `/*`).
    pub fn route(&mut self, method: &str, path: &str, h: Handler) {
        self.insert_route(method, path, RouteKind::Normal(h));
    }

    /// Register a WebSocket handler at exact `path` (upgraded from `GET`).
    pub fn websocket(&mut self, path: &str, h: Box<dyn Fn(WebSocket) + Send + Sync>) {
        self.exact.push(ExactRoute {
            method: "GET".to_string(),
            path: path.to_string(),
            kind: RouteKind::WebSocket(h),
        });
    }

    /// Handler writes the body in pieces; sent as `Transfer-Encoding: chunked` to HTTP/1.1
    /// clients (empty pieces skipped, terminating 0 chunk at the end) and close-delimited to HTTP/1.0.
    #[allow(clippy::type_complexity)]
    pub fn stream(
        &mut self,
        method: &str,
        path: &str,
        h: Box<dyn Fn(&Request, &mut dyn FnMut(&[u8]) -> std::io::Result<()>) -> u16 + Send + Sync>,
    ) {
        self.insert_route(method, path, RouteKind::Stream(h));
    }

    fn insert_route(&mut self, method: &str, path: &str, kind: RouteKind) {
        if let Some(prefix) = path.strip_suffix('*') {
            self.prefixes.push(PrefixRoute {
                method: method.to_string(),
                prefix: prefix.to_string(),
                kind,
            });
        } else {
            self.exact.push(ExactRoute {
                method: method.to_string(),
                path: path.to_string(),
                kind,
            });
        }
    }

    /// Serve static files from `dir` under URL `prefix` for `GET` and `HEAD` requests.
    pub fn static_dir(&mut self, prefix: &str, dir: PathBuf) {
        let base = prefix.trim_end_matches('/').to_string();
        self.statics.push(StaticMount { base, dir });
    }

    /// Dispatch a parsed request through exact routes, prefix routes, and static mounts.
    pub(crate) fn dispatch(&self, req: &Request) -> Dispatch<'_> {
        let path = strip_query_and_fragment(&req.target);

        // 1. Exact routes.
        let mut exact_methods: Vec<&str> = Vec::new();
        for r in &self.exact {
            if r.path == path {
                if r.method == req.method {
                    return dispatch_kind(&r.kind, req);
                }
                if !exact_methods.contains(&r.method.as_str()) {
                    exact_methods.push(&r.method);
                }
            }
        }
        if !exact_methods.is_empty() {
            return Dispatch::Response(method_not_allowed(&exact_methods));
        }

        // 2. Prefix routes (longest matching prefix wins).
        let mut best_len: Option<usize> = None;
        for r in &self.prefixes {
            if path.starts_with(&r.prefix) {
                let len = r.prefix.len();
                if best_len.is_none_or(|b| len > b) {
                    best_len = Some(len);
                }
            }
        }
        if let Some(len) = best_len {
            let mut prefix_methods: Vec<&str> = Vec::new();
            for r in &self.prefixes {
                if r.prefix.len() == len && path.starts_with(&r.prefix) {
                    if r.method == req.method {
                        return dispatch_kind(&r.kind, req);
                    }
                    if !prefix_methods.contains(&r.method.as_str()) {
                        prefix_methods.push(&r.method);
                    }
                }
            }
            return Dispatch::Response(method_not_allowed(&prefix_methods));
        }

        // 3. Static directory mounts (longest matching base wins).
        let mut best_static: Option<(&StaticMount, &str)> = None;
        for m in &self.statics {
            if let Some(rel) = match_static_base(&m.base, path) {
                let replace = best_static.is_none_or(|(prev, _)| m.base.len() > prev.base.len());
                if replace {
                    best_static = Some((m, rel));
                }
            }
        }
        if let Some((mount, rel)) = best_static {
            let range_hdr = req.header("range");
            let resp = match req.method.as_str() {
                "GET" => serve_static(&mount.dir, rel, false, range_hdr),
                "HEAD" => serve_static(&mount.dir, rel, true, range_hdr),
                _ => method_not_allowed(&["GET", "HEAD"]),
            };
            return Dispatch::Response(resp);
        }

        Dispatch::Response(not_found())
    }
}

fn dispatch_kind<'a>(kind: &'a RouteKind, req: &Request) -> Dispatch<'a> {
    match kind {
        RouteKind::Normal(h) => Dispatch::Response(h(req)),
        RouteKind::Stream(h) => Dispatch::Stream(h),
        RouteKind::WebSocket(h) => match validate_ws_handshake(req) {
            Ok(accept) => Dispatch::WebSocket(h, accept),
            Err(resp) => Dispatch::Response(resp),
        },
    }
}

fn validate_ws_handshake(req: &Request) -> Result<String, Response> {
    let has_upgrade = req
        .header("upgrade")
        .is_some_and(|v| trim_ows(v).eq_ignore_ascii_case(b"websocket"));
    let has_conn_upgrade = req.headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("connection")
            && v.split(|&b| b == b',')
                .any(|tok| trim_ows(tok).eq_ignore_ascii_case(b"upgrade"))
    });
    if !has_upgrade || !has_conn_upgrade {
        return Err(ws_bad_request());
    }

    let Some(ver) = req.header("sec-websocket-version") else {
        return Err(ws_bad_request());
    };
    if trim_ows(ver) != b"13" {
        return Err(Response {
            minor: 1,
            status: 426,
            reason: "Upgrade Required".to_string(),
            headers: vec![
                ("Sec-WebSocket-Version".to_string(), b"13".to_vec()),
                ("Content-Length".to_string(), b"0".to_vec()),
            ],
            body: Vec::new(),
        });
    }

    let Some(raw_key) = req.header("sec-websocket-key") else {
        return Err(ws_bad_request());
    };
    let Ok(key_str) = std::str::from_utf8(trim_ows(raw_key)) else {
        return Err(ws_bad_request());
    };
    let Some(decoded) = base64_decode(key_str) else {
        return Err(ws_bad_request());
    };
    if decoded.len() != 16 {
        return Err(ws_bad_request());
    }

    Ok(accept_key(key_str))
}

fn ws_bad_request() -> Response {
    Response {
        minor: 1,
        status: 400,
        reason: "Bad Request".to_string(),
        headers: vec![("Content-Length".to_string(), b"0".to_vec())],
        body: Vec::new(),
    }
}

fn strip_query_and_fragment(target: &str) -> &str {
    let no_frag = target.split('#').next().unwrap_or(target);
    no_frag.split('?').next().unwrap_or(no_frag)
}

fn match_static_base<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        path.strip_prefix('/')
    } else if path == base {
        Some("")
    } else {
        path.strip_prefix(base)
            .and_then(|rest| rest.strip_prefix('/'))
    }
}

fn method_not_allowed(allowed: &[&str]) -> Response {
    Response {
        minor: 1,
        status: 405,
        reason: "Method Not Allowed".to_string(),
        headers: vec![
            ("Allow".to_string(), allowed.join(", ").into_bytes()),
            ("Content-Length".to_string(), b"0".to_vec()),
        ],
        body: Vec::new(),
    }
}
