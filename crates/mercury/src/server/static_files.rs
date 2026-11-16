//! Static file serving with path-traversal protection and MIME type detection.

use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path};
use std::string::{String, ToString};
use std::time::UNIX_EPOCH;
use std::vec;
use std::vec::Vec;

use crate::types::trim_ows;
use crate::Response;

/// Serve a static file under `dir` for relative target `rel_raw`, returning 404 on invalid paths.
/// `ims_hdr` is the `If-Modified-Since` value; a file not newer than it gets `304`.
pub(crate) fn serve_static(
    dir: &Path,
    rel_raw: &str,
    head_only: bool,
    range_hdr: Option<&[u8]>,
    ims_hdr: Option<&[u8]>,
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
    } else if decoded.ends_with('/') {
        return not_found(); // a file named with a trailing slash is not a directory
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
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    let last_modified = mtime.map(|t| ("Last-Modified".to_string(), http_date(t).into_bytes()));

    if let (Some(m), Some(ims)) = (mtime, ims_hdr.and_then(parse_http_date)) {
        if m <= ims {
            return Response {
                minor: 1,
                status: 304,
                reason: "Not Modified".to_string(),
                headers: last_modified.into_iter().collect(),
                body: Vec::new(),
            };
        }
    }

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
                    headers: [
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
                    ]
                    .into_iter()
                    .chain(last_modified.clone())
                    .collect(),
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
        headers: [
            ("Content-Type".to_string(), mime.as_bytes().to_vec()),
            ("Accept-Ranges".to_string(), b"bytes".to_vec()),
            ("Content-Length".to_string(), len.into_bytes()),
        ]
        .into_iter()
        .chain(last_modified)
        .collect(),
        body,
    }
}

/// Format Unix seconds as an IMF-fixdate, e.g. `Sun, 06 Nov 1994 08:49:37 GMT`.
fn http_date(secs: i64) -> String {
    const WDAY: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let wday = WDAY[days.rem_euclid(7) as usize];
    let mut out = String::new();
    let _ = write!(
        out,
        "{wday}, {d:02} {} {y:04} {:02}:{:02}:{:02} GMT",
        MON[(m - 1) as usize],
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );
    out
}

/// Parse an IMF-fixdate into Unix seconds.
/// shortcut: IMF-fixdate only; RFC 850 and asctime dates are ignored (the file is sent as 200), upgrade if a client needs them.
fn parse_http_date(v: &[u8]) -> Option<i64> {
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let s = std::str::from_utf8(trim_ows(v)).ok()?;
    let mut it = s.split_ascii_whitespace();
    let _wday = it.next()?;
    let d = it.next()?.parse::<i64>().ok()?;
    let mon_s = it.next()?;
    let mon = MON.iter().position(|&x| x == mon_s)? as i64 + 1;
    let y = it.next()?.parse::<i64>().ok()?;
    let mut hms = it.next()?.split(':');
    let h = hms.next()?.parse::<i64>().ok()?;
    let mi = hms.next()?.parse::<i64>().ok()?;
    let se = hms.next()?.parse::<i64>().ok()?;
    if it.next()? != "GMT" || it.next().is_some() || hms.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 || y < 1970 {
        return None;
    }
    Some(days_from_civil(y, mon, d) * 86_400 + h * 3600 + mi * 60 + se)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`: (year, month, day).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Build the `404 Not Found` body `404 not found: <path>` (control bytes escaped).
pub(crate) fn not_found_with_body(path: &str) -> Response {
    let mut text = String::from("404 not found: ");
    for c in path.chars() {
        if c.is_control() {
            let _ = write!(text, "\\x{:02x}", c as u32);
        } else {
            text.push(c);
        }
    }
    text.push('\n');
    let body = text.into_bytes();
    Response {
        minor: 1,
        status: 404,
        reason: "Not Found".to_string(),
        headers: vec![
            (
                "Content-Type".to_string(),
                b"text/plain; charset=utf-8".to_vec(),
            ),
            (
                "Content-Length".to_string(),
                body.len().to_string().into_bytes(),
            ),
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
