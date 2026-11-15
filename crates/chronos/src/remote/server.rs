//! HTTP/1.1 server endpoints (`GET /refs`, `POST /fetch`, `POST /push`) for `mercury::server::Router`.

use std::collections::BTreeSet;
use std::format;
use std::string::{String, ToString};
use std::sync::{Arc, Mutex};
use std::vec;
use std::vec::Vec;

use mercury::server::Router;
use mercury::Response;

use super::pack::{decode_pack, encode_pack, MAX_PACK_BYTES, MAX_PACK_OBJECTS};
use super::{
    collect_missing_objects, collect_reachable_from_known, is_ancestor, read_branch_id,
    validate_remote_branch_name, verify_history,
};
use crate::id::{from_hex, to_hex, Id};
use crate::repo::Repo;
use crate::store::Store;

/// Mount the chronos HTTP protocol under `prefix` (e.g. `"/repo"`) on an `mercury` router.
pub fn serve<S: Store + Send + 'static>(
    repo: Arc<Mutex<Repo<S>>>,
    router: &mut Router,
    prefix: &str,
) {
    let trimmed = prefix.trim_matches('/');
    let base = if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    };

    let refs_repo = Arc::clone(&repo);
    router.route(
        "GET",
        &format!("{base}/refs"),
        Box::new(move |_| handle_refs(&refs_repo)),
    );

    let fetch_repo = Arc::clone(&repo);
    router.route(
        "POST",
        &format!("{base}/fetch"),
        Box::new(move |req| handle_fetch(&fetch_repo, &req.body)),
    );

    let push_repo = Arc::clone(&repo);
    router.route(
        "POST",
        &format!("{base}/push"),
        Box::new(move |req| handle_push(&push_repo, &req.body)),
    );
}

fn text_response(status: u16, reason: &str, body: Vec<u8>) -> Response {
    Response {
        minor: 1,
        status,
        reason: reason.to_string(),
        headers: vec![(
            "Content-Length".to_string(),
            body.len().to_string().into_bytes(),
        )],
        body,
    }
}

fn handle_refs<S: Store>(repo: &Arc<Mutex<Repo<S>>>) -> Response {
    let Ok(guard) = repo.lock() else {
        return text_response(500, "Internal Server Error", Vec::new());
    };
    let mut branches = guard.branches();
    branches.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (name, id) in branches {
        out.push_str(&format!("{} {}\n", to_hex(&id), name));
    }
    text_response(200, "OK", out.into_bytes())
}

fn handle_fetch<S: Store>(repo: &Arc<Mutex<Repo<S>>>, body: &[u8]) -> Response {
    let Ok(text) = core::str::from_utf8(body) else {
        return text_response(400, "Bad Request", Vec::new());
    };
    let mut wants: Vec<Id> = Vec::new();
    let mut haves: Vec<Id> = Vec::new();
    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if let Some(hex) = line.strip_prefix("want ") {
            let Some(id) = from_hex(hex) else {
                return text_response(400, "Bad Request", Vec::new());
            };
            wants.push(id);
        } else if let Some(hex) = line.strip_prefix("have ") {
            let Some(id) = from_hex(hex) else {
                return text_response(400, "Bad Request", Vec::new());
            };
            haves.push(id);
        } else {
            return text_response(400, "Bad Request", Vec::new());
        }
    }

    let Ok(guard) = repo.lock() else {
        return text_response(500, "Internal Server Error", Vec::new());
    };
    for want in &wants {
        if guard.get(want).is_err() {
            return text_response(400, "Bad Request", Vec::new());
        }
    }
    let have_set: BTreeSet<Id> = collect_reachable_from_known(&guard, &haves);
    let Ok(objects) = collect_missing_objects(&guard, &wants, &have_set) else {
        return text_response(500, "Internal Server Error", Vec::new());
    };
    let pack = encode_pack(&objects);
    text_response(200, "OK", pack)
}

struct RefUpdate {
    old_id: Id,
    new_id: Id,
    branch: String,
}

fn parse_push_body(body: &[u8]) -> Result<(Vec<RefUpdate>, &[u8]), u16> {
    let mut updates = Vec::new();
    let mut pos = 0usize;
    let mut found_sep = false;

    while pos < body.len() {
        let Some(rel_nl) = body[pos..].iter().position(|&b| b == b'\n') else {
            return Err(400);
        };
        let line_bytes = &body[pos..pos + rel_nl];
        pos += rel_nl + 1;
        let line_bytes = line_bytes.strip_suffix(b"\r").unwrap_or(line_bytes);
        if line_bytes.is_empty() {
            found_sep = true;
            break;
        }
        let line = core::str::from_utf8(line_bytes).map_err(|_| 400u16)?;
        let mut parts = line.splitn(3, ' ');
        let old_hex = parts.next().ok_or(400u16)?;
        let new_hex = parts.next().ok_or(400u16)?;
        let branch = parts.next().ok_or(400u16)?;
        let old_id = from_hex(old_hex).ok_or(400u16)?;
        let new_id = from_hex(new_hex).ok_or(400u16)?;
        updates.push(RefUpdate {
            old_id,
            new_id,
            branch: branch.to_string(),
        });
    }

    if !found_sep || updates.is_empty() {
        return Err(400);
    }
    Ok((updates, &body[pos..]))
}

fn handle_push<S: Store>(repo: &Arc<Mutex<Repo<S>>>, body: &[u8]) -> Response {
    let (updates, pack_bytes) = match parse_push_body(body) {
        Ok(parsed) => parsed,
        Err(status) => return text_response(status, "Bad Request", Vec::new()),
    };

    if pack_bytes.len() > MAX_PACK_BYTES {
        return text_response(413, "Payload Too Large", Vec::new());
    }
    if pack_bytes.len() >= 8 && pack_bytes.get(..4) == Some(b"DPK1".as_slice()) {
        let count = u32::from_be_bytes([pack_bytes[4], pack_bytes[5], pack_bytes[6], pack_bytes[7]])
            as usize;
        if count > MAX_PACK_OBJECTS {
            return text_response(413, "Payload Too Large", Vec::new());
        }
    }

    let Ok(objects) = decode_pack(pack_bytes, MAX_PACK_OBJECTS) else {
        return text_response(400, "Bad Request", Vec::new());
    };

    let Ok(mut guard) = repo.lock() else {
        return text_response(500, "Internal Server Error", Vec::new());
    };

    for (kind, data) in objects {
        guard.put(kind, &data);
    }

    let zero_id = [0u8; 32];
    let mut reply = String::new();

    for u in updates {
        if !validate_remote_branch_name(&u.branch) {
            reply.push_str(&format!("ng {} bad-name\n", u.branch));
            continue;
        }
        let cur = read_branch_id(&guard, &u.branch);
        if u.old_id == zero_id {
            if cur.is_some() {
                reply.push_str(&format!("ng {} stale\n", u.branch));
                continue;
            }
        } else if cur != Some(u.old_id) {
            reply.push_str(&format!("ng {} stale\n", u.branch));
            continue;
        }

        if !verify_history(&guard, &u.new_id) {
            reply.push_str(&format!("ng {} missing\n", u.branch));
            continue;
        }

        if u.old_id != zero_id && !is_ancestor(&guard, &u.old_id, &u.new_id) {
            reply.push_str(&format!("ng {} non-fast-forward\n", u.branch));
            continue;
        }

        let hex_line = format!("{}\n", to_hex(&u.new_id));
        guard
            .store
            .write(&format!("refs/heads/{}", u.branch), hex_line.as_bytes());
        reply.push_str(&format!("ok {}\n", u.branch));
    }

    text_response(200, "OK", reply.into_bytes())
}
