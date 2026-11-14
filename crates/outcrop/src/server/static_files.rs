//! Static file serving with path-traversal protection and MIME type detection.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path};
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use crate::types::trim_ows;
use crate::Response;

/// Serve a static file under `dir` for relative target `rel_raw`, returning 404 on invalid paths.
pub(crate) fn serve_static(
    dir: &Path,
    rel_raw: &str,
    head_only: bool,
    range_hdr: Option<&[u8]>,
) -> Response {
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
    let file_len = meta.len();

    if let Some(raw_range) = range_hdr {
        match parse_single_range(raw_range, file_len) {
            RangeOutcome::Satisfiable { start, end } => {
                let slice_len = end - start + 1;
                let body = if head_only {
                    Vec::new()
                } else {
                    let Ok(mut f) = fs::File::open(&canon_target) else {
                        return not_found();
                    };
                    if f.seek(SeekFrom::Start(start)).is_err() {
                        return not_found();
                    }
                    let mut buf = Vec::new();
                    if f.take(slice_len).read_to_end(&mut buf).is_err() {
                        return not_found();
                    }
                    buf
                };
                return Response {
                    minor: 1,
                    status: 206,
                    reason: "Partial Content".to_string(),
                    headers: vec![
                        ("Content-Type".to_string(), mime.as_bytes().to_vec()),
                        ("Accept-Ranges".to_string(), b"bytes".to_vec()),
                        (
                            "Content-Range".to_string(),
                            std::format!("bytes {start}-{end}/{file_len}").into_bytes(),
                        ),
                        (
                            "Content-Length".to_string(),
                            slice_len.to_string().into_bytes(),
                        ),
                    ],
                    body,
                };
            }
            RangeOutcome::Unsatisfiable => {
                return Response {
                    minor: 1,
                    status: 416,
                    reason: "Range Not Satisfiable".to_string(),
                    headers: vec![
                        ("Accept-Ranges".to_string(), b"bytes".to_vec()),
                        (
                            "Content-Range".to_string(),
                            std::format!("bytes */{file_len}").into_bytes(),
                        ),
                        ("Content-Length".to_string(), b"0".to_vec()),
                    ],
                    body: Vec::new(),
                };
            }
            RangeOutcome::MalformedOrMultiple => {}
        }
    }

    let (body, len) = if head_only {
        (Vec::new(), file_len.to_string())
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
            ("Accept-Ranges".to_string(), b"bytes".to_vec()),
            ("Content-Length".to_string(), len.into_bytes()),
        ],
        body,
    }
}

enum RangeOutcome {
    Satisfiable { start: u64, end: u64 },
    Unsatisfiable,
    MalformedOrMultiple,
}

fn parse_single_range(raw: &[u8], file_len: u64) -> RangeOutcome {
    let Ok(s) = std::str::from_utf8(trim_ows(raw)) else {
        return RangeOutcome::MalformedOrMultiple;
    };
    let Some((unit, spec)) = s.split_once('=') else {
        return RangeOutcome::MalformedOrMultiple;
    };
    if !unit.eq_ignore_ascii_case("bytes") || spec.contains(',') {
        return RangeOutcome::MalformedOrMultiple;
    }
    let Some((start_s, end_s)) = spec.split_once('-') else {
        return RangeOutcome::MalformedOrMultiple;
    };
    let start_s = start_s.trim_matches([' ', '\t']);
    let end_s = end_s.trim_matches([' ', '\t']);

    match (start_s.is_empty(), end_s.is_empty()) {
        (true, true) => RangeOutcome::MalformedOrMultiple,
        (true, false) => {
            let Some(n) = parse_u64_digits(end_s) else {
                return RangeOutcome::MalformedOrMultiple;
            };
            if n == 0 || file_len == 0 {
                return RangeOutcome::Unsatisfiable;
            }
            RangeOutcome::Satisfiable {
                start: file_len.saturating_sub(n),
                end: file_len - 1,
            }
        }
        (false, true) => {
            let Some(a) = parse_u64_digits(start_s) else {
                return RangeOutcome::MalformedOrMultiple;
            };
            if a >= file_len {
                return RangeOutcome::Unsatisfiable;
            }
            RangeOutcome::Satisfiable {
                start: a,
                end: file_len - 1,
            }
        }
        (false, false) => {
            let (Some(a), Some(b)) = (parse_u64_digits(start_s), parse_u64_digits(end_s)) else {
                return RangeOutcome::MalformedOrMultiple;
            };
            if a > b {
                return RangeOutcome::MalformedOrMultiple;
            }
            if a >= file_len {
                return RangeOutcome::Unsatisfiable;
            }
            RangeOutcome::Satisfiable {
                start: a,
                end: b.min(file_len - 1),
            }
        }
    }
}

fn parse_u64_digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u64>().ok()
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
