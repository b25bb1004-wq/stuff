//! Static web bundle HTTP server for the site, Athena, and `/healthz`.

#![forbid(unsafe_code)]

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use mercury::server::{Config as HttpConfig, Router, Server as HttpServer};
use mercury::Response;

const STOP_POLL_MS: u64 = 50;
const WEB_SERVER_THREADS: usize = 8;

/// Serve `root` over HTTP on `listen` with `GET /healthz` until `stop` becomes true.
pub fn run_web(listen: &str, root: &Path, stop: &AtomicBool) -> io::Result<()> {
    validate_bundle_root(root)?;

    let mut router = Router::new();
    router.route(
        "GET",
        "/healthz",
        Box::new(|_req| {
            let body = b"ok".to_vec();
            Response {
                minor: 1,
                status: 200,
                reason: "OK".to_string(),
                headers: vec![
                    (
                        "Content-Length".to_string(),
                        body.len().to_string().into_bytes(),
                    ),
                    ("Content-Type".to_string(), b"text/plain".to_vec()),
                ],
                body,
            }
        }),
    );
    router.static_dir("/", root.to_path_buf());

    let cfg = HttpConfig {
        addr: listen.to_string(),
        threads: WEB_SERVER_THREADS,
        ..HttpConfig::default()
    };
    let server = HttpServer::start(cfg, router)?;

    while !stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(STOP_POLL_MS));
    }
    server.stop();
    Ok(())
}

fn validate_bundle_root(root: &Path) -> io::Result<()> {
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("web root directory does not exist: {}", root.display()),
        ));
    }
    let root_index = root.join("index.html");
    if !root_index.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required site index: {}", root_index.display()),
        ));
    }
    let athena_index = root.join("athena").join("index.html");
    if !athena_index.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required athena index: {}", athena_index.display()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "deploy-web-unit-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validate_bundle_root_requires_site_and_athena_indexes() {
        let missing = unique_temp_dir("empty");
        let err = validate_bundle_root(&missing).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);

        fs::write(missing.join("index.html"), b"<html></html>").unwrap();
        let err = validate_bundle_root(&missing).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("athena"));

        fs::create_dir_all(missing.join("athena")).unwrap();
        fs::write(missing.join("athena").join("index.html"), b"<html></html>").unwrap();
        validate_bundle_root(&missing).unwrap();

        let _ = fs::remove_dir_all(&missing);
    }
}
