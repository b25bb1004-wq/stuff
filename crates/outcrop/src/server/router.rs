//! Request routing and `404`/`405` dispatch for [`crate::server::Server`].

use std::boxed::Box;
use std::path::PathBuf;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use super::static_files::{not_found, serve_static};
use crate::{Request, Response};

/// Request handler callback registered with [`Router::route`].
pub type Handler = Box<dyn Fn(&Request) -> Response + Send + Sync>;

struct ExactRoute {
    method: String,
    path: String,
    handler: Handler,
}

struct PrefixRoute {
    method: String,
    prefix: String,
    handler: Handler,
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
        if let Some(prefix) = path.strip_suffix('*') {
            self.prefixes.push(PrefixRoute {
                method: method.to_string(),
                prefix: prefix.to_string(),
                handler: h,
            });
        } else {
            self.exact.push(ExactRoute {
                method: method.to_string(),
                path: path.to_string(),
                handler: h,
            });
        }
    }

    /// Serve static files from `dir` under URL `prefix` for `GET` and `HEAD` requests.
    pub fn static_dir(&mut self, prefix: &str, dir: PathBuf) {
        let base = prefix.trim_end_matches('/').to_string();
        self.statics.push(StaticMount { base, dir });
    }

    /// Dispatch a parsed request through exact routes, prefix routes, and static mounts.
    pub(crate) fn dispatch(&self, req: &Request) -> Response {
        let path = strip_query_and_fragment(&req.target);

        // 1. Exact routes.
        let mut exact_methods: Vec<&str> = Vec::new();
        for r in &self.exact {
            if r.path == path {
                if r.method == req.method {
                    return (r.handler)(req);
                }
                if !exact_methods.contains(&r.method.as_str()) {
                    exact_methods.push(&r.method);
                }
            }
        }
        if !exact_methods.is_empty() {
            return method_not_allowed(&exact_methods);
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
                        return (r.handler)(req);
                    }
                    if !prefix_methods.contains(&r.method.as_str()) {
                        prefix_methods.push(&r.method);
                    }
                }
            }
            return method_not_allowed(&prefix_methods);
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
            return match req.method.as_str() {
                "GET" => serve_static(&mount.dir, rel, false),
                "HEAD" => serve_static(&mount.dir, rel, true),
                _ => method_not_allowed(&["GET", "HEAD"]),
            };
        }

        not_found()
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
