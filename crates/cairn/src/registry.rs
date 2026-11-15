//! In-memory HTTP/1.1 package registry backed by [`outcrop::server::Server`].

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use outcrop::server::{Config, Router, Server};
use outcrop::{Limits, Request, Response};

use crate::pkg::{self, PkgError};
use crate::version::{Req, Version};

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

struct StoredPkg {
    blob: Vec<u8>,
    deps: Vec<(String, Req)>,
}

type Store = BTreeMap<(String, Version), StoredPkg>;

/// In-memory signed package registry serving HTTP/1.1.
pub struct Registry {
    server: Server,
}

impl Registry {
    /// Start serving the registry protocol on `addr`, accepting uploads signed by `publishers`.
    pub fn start(addr: &str, publishers: Vec<[u8; 32]>) -> io::Result<Registry> {
        let store: Arc<RwLock<Store>> = Arc::new(RwLock::new(BTreeMap::new()));
        let publishers: Arc<Vec<[u8; 32]>> = Arc::new(publishers);

        let mut router = Router::new();

        {
            let store = Arc::clone(&store);
            let publishers = Arc::clone(&publishers);
            router.route(
                "PUT",
                "/pkg/*",
                Box::new(move |req| handle_put(req, &store, &publishers)),
            );
        }

        {
            let store = Arc::clone(&store);
            router.route(
                "GET",
                "/pkg/*",
                Box::new(move |req| handle_get_pkg(req, &store)),
            );
        }

        {
            let store = Arc::clone(&store);
            router.route(
                "GET",
                "/index",
                Box::new(move |_req| handle_get_index(&store)),
            );
        }

        let cfg = Config {
            addr: addr.to_string(),
            limits: Limits {
                max_body: MAX_BODY_BYTES,
                ..Limits::default()
            },
            ..Config::default()
        };
        let server = Server::start(cfg, router)?;
        Ok(Registry { server })
    }

    /// Return the local socket address the registry is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// Stop the registry server gracefully.
    pub fn stop(self) {
        self.server.stop();
    }
}

fn parse_pkg_target(target: &str) -> Option<(&str, &str)> {
    let no_frag = target.split('#').next().unwrap_or(target);
    let path = no_frag.split('?').next().unwrap_or(no_frag);
    let rest = path.strip_prefix("/pkg/")?;
    let (name, ver) = rest.split_once('/')?;
    if name.is_empty() || ver.is_empty() || ver.contains('/') {
        return None;
    }
    Some((name, ver))
}

fn handle_put(req: &Request, store: &RwLock<Store>, publishers: &[[u8; 32]]) -> Response {
    let Some((name, ver_str)) = parse_pkg_target(&req.target) else {
        return text_response(404, "Not Found", b"not found");
    };

    let package = match pkg::open(&req.body, publishers) {
        Ok(p) => p,
        Err(PkgError::UntrustedKey) | Err(PkgError::BadSignature) => {
            return text_response(403, "Forbidden", b"forbidden");
        }
        Err(PkgError::Malformed(_)) | Err(PkgError::BadPath(_)) => {
            return text_response(400, "Bad Request", b"bad package");
        }
    };

    let Ok(url_ver) = Version::parse(ver_str) else {
        return text_response(400, "Bad Request", b"invalid version in URL");
    };
    if package.manifest.name != name || package.manifest.version != url_ver {
        return text_response(
            400,
            "Bad Request",
            b"URL name/version does not match manifest",
        );
    }

    let mut guard = store
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (name.to_string(), url_ver);
    if guard.contains_key(&key) {
        return text_response(409, "Conflict", b"package version already exists");
    }
    guard.insert(
        key,
        StoredPkg {
            blob: req.body.clone(),
            deps: package.manifest.dependencies,
        },
    );
    text_response(201, "Created", b"")
}

fn handle_get_pkg(req: &Request, store: &RwLock<Store>) -> Response {
    let Some((name, ver_str)) = parse_pkg_target(&req.target) else {
        return text_response(404, "Not Found", b"not found");
    };
    let Ok(ver) = Version::parse(ver_str) else {
        return text_response(404, "Not Found", b"not found");
    };

    let guard = store
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.get(&(name.to_string(), ver)) {
        Some(stored) => Response {
            minor: 1,
            status: 200,
            reason: "OK".to_string(),
            headers: vec![
                (
                    "Content-Length".to_string(),
                    stored.blob.len().to_string().into_bytes(),
                ),
                (
                    "Content-Type".to_string(),
                    b"application/octet-stream".to_vec(),
                ),
            ],
            body: stored.blob.clone(),
        },
        None => text_response(404, "Not Found", b"not found"),
    }
}

fn handle_get_index(store: &RwLock<Store>) -> Response {
    let guard = store
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = String::new();
    for ((name, ver), stored) in guard.iter() {
        let _ = write!(out, "{name}\t{ver}");
        for (dep, req) in &stored.deps {
            let _ = write!(out, "\t{dep}\t{req}");
        }
        out.push('\n');
    }
    let body = out.into_bytes();
    Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![
            (
                "Content-Length".to_string(),
                body.len().to_string().into_bytes(),
            ),
            (
                "Content-Type".to_string(),
                b"text/plain; charset=utf-8".to_vec(),
            ),
        ],
        body,
    }
}

fn text_response(status: u16, reason: &str, body: &[u8]) -> Response {
    Response {
        minor: 1,
        status,
        reason: reason.to_string(),
        headers: vec![(
            "Content-Length".to_string(),
            body.len().to_string().into_bytes(),
        )],
        body: body.to_vec(),
    }
}
