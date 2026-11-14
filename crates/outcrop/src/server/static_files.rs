//! Static file serving with path-traversal protection and MIME type detection.

use std::fs;
use std::path::{Component, Path};
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use crate::Response;

/// Serve a static file under `dir` for relative target `rel_raw`, returning 404 on invalid paths.
pub(crate) fn serve_static(dir: &Path, rel_raw: &str, head_only: bool) -> Response {
    let Some(decoded) = percent_decode(rel_raw) else {
        return not_found();
    };
    if decoded.contains('\0') || decoded.starts_with('/') || decoded.starts_with('\\') {
        return not_found();
    }
    for comp in Path::new(&decoded).components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return not_found();
            }
        }
    }

    let mut target = dir.to_path_buf();
    for seg in decoded.split(['/', '\\']) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." || seg.contains(':') {
            return not_found();
        }
        target.push(seg);
    }

    let Ok(canon_dir) = fs::canonicalize(dir) else {
        return not_found();
    };
    if fs::metadata(&target).map(|m| m.is_dir()).unwrap_or(false) {
        target.push("index.html");
    }
    let Ok(canon_target) = fs::canonicalize(&target) else {
        return not_found();
    };
    if !canon_target.starts_with(&canon_dir) {
        return not_found();
    }
    let Ok(meta) = fs::metadata(&canon_target) else {
        return not_found();
    };
    if !meta.is_file() {
        return not_found();
    }

    let mime = content_type_for(&canon_target);
    let (body, len) = if head_only {
        (Vec::new(), meta.len().to_string())
    } else {
        let Ok(bytes) = fs::read(&canon_target) else {
            return not_found();
        };
        let len = bytes.len().to_string();
        (bytes, len)
    };

    Response {
        minor: 1,
        status: 200,
        reason: "OK".to_string(),
        headers: vec![
            ("Content-Type".to_string(), mime.as_bytes().to_vec()),
            ("Content-Length".to_string(), len.into_bytes()),
        ],
        body,
    }
}

/// Percent-decode a URL path segment sequence; returns `None` on invalid escapes or UTF-8.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let h1 = *bytes.get(i + 1)?;
            let h2 = *bytes.get(i + 2)?;
            let val = (hex_val(h1)? << 4) | hex_val(h2)?;
            out.push(val);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Map a file extension to its `Content-Type` header value.
fn content_type_for(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Build a standard `404 Not Found` response.
pub(crate) fn not_found() -> Response {
    Response {
        minor: 1,
        status: 404,
        reason: "Not Found".to_string(),
        headers: vec![("Content-Length".to_string(), b"0".to_vec())],
        body: Vec::new(),
    }
}
