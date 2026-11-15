//! HTTP remote protocol (`outcrop`) for `clone`, `fetch`, `pull`, `push`, and `serve`.

#[path = "remote/http.rs"]
mod http;
#[path = "remote/pack.rs"]
mod pack;
#[path = "remote/server.rs"]
mod server;
#[cfg(test)]
#[path = "remote/tests.rs"]
mod tests;

use alloc::collections::{BTreeSet, VecDeque};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::format::{decode_commit, decode_tree, Mode, Signature};
use crate::id::{from_hex, to_hex, Error, Id, Kind};
use crate::merge::MergeError;
use crate::repo::Repo;
use crate::store::{validate_branch_name, Store};

use http::{http_get, http_post};
use pack::MAX_PACK_OBJECTS;
pub use pack::{decode_pack, encode_pack};
pub use server::serve;

/// Error returned by remote operations (`clone`, `fetch`, `pull`, `push`, and pack decoding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// Underlying TCP or I/O failure.
    Io(String),
    /// Non-200 HTTP response status code.
    Http(u16),
    /// Malformed pack, URL, or HTTP protocol payload.
    Protocol,
    /// Server rejected a ref update with the given reason.
    Rejected(String),
    /// Local repository or object-store error.
    Repo(Error),
    /// Merge failure during `pull`.
    Merge(MergeError),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::Io(msg) => write!(f, "remote io error: {msg}"),
            RemoteError::Http(code) => write!(f, "remote http status {code}"),
            RemoteError::Protocol => f.write_str("remote protocol error"),
            RemoteError::Rejected(reason) => write!(f, "push rejected: {reason}"),
            RemoteError::Repo(err) => write!(f, "repository error: {err}"),
            RemoteError::Merge(err) => write!(f, "merge error: {err:?}"),
        }
    }
}

impl std::error::Error for RemoteError {}

impl From<Error> for RemoteError {
    fn from(err: Error) -> Self {
        RemoteError::Repo(err)
    }
}

impl From<MergeError> for RemoteError {
    fn from(err: MergeError) -> Self {
        RemoteError::Merge(err)
    }
}

/// Clone a remote repository at `url` (`http://host:port/prefix`) into `store`.
/// Fetches all branches and checks out `"main"` if present.
pub fn clone<S: Store>(url: &str, store: S) -> Result<Repo<S>, RemoteError> {
    let mut repo = Repo::init(store);
    let remote_refs = repo.fetch(url)?;
    let mut has_main = false;
    for (branch, id) in &remote_refs {
        if validate_remote_branch_name(branch) {
            let hex_line = format!("{}\n", to_hex(id));
            repo.store
                .write(&format!("refs/heads/{branch}"), hex_line.as_bytes());
            if branch == "main" {
                has_main = true;
            }
        }
    }
    if has_main {
        // Temporarily point HEAD at an unborn ref so `checkout("main")` sees an empty head index.
        repo.store
            .write("HEAD", b"ref: refs/heads/__clone_init__\n");
        repo.checkout("main")?;
    }
    Ok(repo)
}

impl<S: Store> Repo<S> {
    /// Download missing objects for every remote branch; returns the remote refs. Local branches unchanged.
    pub fn fetch(&mut self, url: &str) -> Result<Vec<(String, Id)>, RemoteError> {
        let refs_bytes = http_get(url, "/refs")?;
        let remote_refs = parse_refs_body(&refs_bytes)?;

        let mut wants: Vec<Id> = Vec::new();
        let mut want_seen = BTreeSet::new();
        for (_, id) in &remote_refs {
            if self.get(id).is_err() && want_seen.insert(*id) {
                wants.push(*id);
            }
        }
        if wants.is_empty() {
            return Ok(remote_refs);
        }

        let mut haves: Vec<Id> = Vec::new();
        let mut have_seen = BTreeSet::new();
        for (_, id) in self.branches() {
            if have_seen.insert(id) {
                haves.push(id);
            }
        }
        if let Some(head_id) = self.head() {
            if have_seen.insert(head_id) {
                haves.push(head_id);
            }
        }

        let mut body = String::new();
        for id in &wants {
            body.push_str(&format!("want {}\n", to_hex(id)));
        }
        for id in &haves {
            body.push_str(&format!("have {}\n", to_hex(id)));
        }

        let pack_bytes = http_post(url, "/fetch", body.into_bytes())?;
        let objects = decode_pack(&pack_bytes, MAX_PACK_OBJECTS)?;
        for (kind, data) in objects {
            self.put(kind, &data);
        }
        Ok(remote_refs)
    }

    /// Fetch from `url`, then merge the remote `branch` tip into the current branch (fast-forward when possible).
    pub fn pull(&mut self, url: &str, branch: &str, who: &Signature) -> Result<Id, RemoteError> {
        let remote_refs = self.fetch(url)?;
        let (_, remote_id) = remote_refs
            .into_iter()
            .find(|(b, _)| b == branch)
            .ok_or(RemoteError::Repo(Error::NotFound))?;
        let msg = format!("Merge branch '{branch}'");
        let id = self.merge(&to_hex(&remote_id), &msg, who)?;
        Ok(id)
    }

    /// Send the local `branch` (objects the server lacks, using its refs as haves); fast-forward only.
    pub fn push(&mut self, url: &str, branch: &str) -> Result<(), RemoteError> {
        let local_id = read_branch_id(self, branch).ok_or(RemoteError::Repo(Error::NotFound))?;
        let refs_bytes = http_get(url, "/refs")?;
        let remote_refs = parse_refs_body(&refs_bytes)?;

        let old_id = remote_refs
            .iter()
            .find(|(b, _)| b == branch)
            .map(|(_, id)| *id)
            .unwrap_or([0u8; 32]);

        let haves: Vec<Id> = remote_refs.iter().map(|(_, id)| *id).collect();
        let have_set = collect_reachable_from_known(self, &haves);
        let objects = collect_missing_objects(self, &[local_id], &have_set)?;
        let pack = encode_pack(&objects);

        let header = format!("{} {} {}\n\n", to_hex(&old_id), to_hex(&local_id), branch);
        let mut body = Vec::with_capacity(header.len().saturating_add(pack.len()));
        body.extend_from_slice(header.as_bytes());
        body.extend_from_slice(&pack);

        let resp_bytes = http_post(url, "/push", body)?;
        let resp_text = core::str::from_utf8(&resp_bytes).map_err(|_| RemoteError::Protocol)?;
        for raw_line in resp_text.split('\n') {
            let line = raw_line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("ok ") {
                if rest == branch {
                    return Ok(());
                }
            } else if let Some(rest) = line.strip_prefix("ng ") {
                if let Some((b, reason)) = rest.split_once(' ') {
                    if b == branch {
                        return Err(RemoteError::Rejected(reason.to_string()));
                    }
                }
            } else {
                return Err(RemoteError::Protocol);
            }
        }
        Err(RemoteError::Protocol)
    }
}

fn parse_refs_body(bytes: &[u8]) -> Result<Vec<(String, Id)>, RemoteError> {
    let text = core::str::from_utf8(bytes).map_err(|_| RemoteError::Protocol)?;
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (hex, branch) = line.split_once(' ').ok_or(RemoteError::Protocol)?;
        let id = from_hex(hex).ok_or(RemoteError::Protocol)?;
        out.push((branch.to_string(), id));
    }
    Ok(out)
}

pub(crate) fn read_branch_id<S: Store>(repo: &Repo<S>, branch: &str) -> Option<Id> {
    let raw = repo.store.read(&format!("refs/heads/{branch}"))?;
    let s = core::str::from_utf8(&raw).ok()?.trim();
    from_hex(s)
}

pub(crate) fn validate_remote_branch_name(name: &str) -> bool {
    name.len() <= 200 && validate_branch_name(name).is_ok()
}

pub(crate) fn collect_reachable_from_known<S: Store>(repo: &Repo<S>, roots: &[Id]) -> BTreeSet<Id> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    for &id in roots {
        if repo.get(&id).is_ok() && visited.insert(id) {
            queue.push_back(id);
        }
    }
    while let Some(id) = queue.pop_front() {
        let Ok((kind, data)) = repo.get(&id) else {
            continue;
        };
        match kind {
            Kind::Commit => {
                if let Ok(c) = decode_commit(&data) {
                    if visited.insert(c.tree) {
                        queue.push_back(c.tree);
                    }
                    for p in c.parents {
                        if visited.insert(p) {
                            queue.push_back(p);
                        }
                    }
                }
            }
            Kind::Tree => {
                if let Ok(entries) = decode_tree(&data) {
                    for e in entries {
                        if visited.insert(e.id) {
                            queue.push_back(e.id);
                        }
                    }
                }
            }
            Kind::Blob => {}
        }
    }
    visited
}

pub(crate) fn collect_missing_objects<S: Store>(
    repo: &Repo<S>,
    roots: &[Id],
    stop_set: &BTreeSet<Id>,
) -> Result<Vec<(Kind, Vec<u8>)>, Error> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    for &r in roots {
        if !stop_set.contains(&r) && visited.insert(r) {
            queue.push_back(r);
        }
    }
    let mut out = Vec::new();
    while let Some(id) = queue.pop_front() {
        let (kind, data) = repo.get(&id)?;
        match kind {
            Kind::Commit => {
                let c = decode_commit(&data)?;
                if !stop_set.contains(&c.tree) && visited.insert(c.tree) {
                    queue.push_back(c.tree);
                }
                for p in c.parents {
                    if !stop_set.contains(&p) && visited.insert(p) {
                        queue.push_back(p);
                    }
                }
            }
            Kind::Tree => {
                let entries = decode_tree(&data)?;
                for e in entries {
                    if !stop_set.contains(&e.id) && visited.insert(e.id) {
                        queue.push_back(e.id);
                    }
                }
            }
            Kind::Blob => {}
        }
        out.push((kind, data));
    }
    Ok(out)
}

pub(crate) fn verify_history<S: Store>(repo: &Repo<S>, root: &Id) -> bool {
    fn kind_tag(k: Kind) -> u8 {
        match k {
            Kind::Blob => 1,
            Kind::Tree => 2,
            Kind::Commit => 3,
        }
    }

    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    if visited.insert((*root, kind_tag(Kind::Commit))) {
        queue.push_back((*root, Kind::Commit));
    }
    while let Some((id, expected_kind)) = queue.pop_front() {
        let Ok((kind, data)) = repo.get(&id) else {
            return false;
        };
        if kind != expected_kind {
            return false;
        }
        match kind {
            Kind::Commit => {
                let Ok(c) = decode_commit(&data) else {
                    return false;
                };
                if visited.insert((c.tree, kind_tag(Kind::Tree))) {
                    queue.push_back((c.tree, Kind::Tree));
                }
                for p in c.parents {
                    if visited.insert((p, kind_tag(Kind::Commit))) {
                        queue.push_back((p, Kind::Commit));
                    }
                }
            }
            Kind::Tree => {
                let Ok(entries) = decode_tree(&data) else {
                    return false;
                };
                for e in entries {
                    if e.name == "." || e.name == ".." {
                        return false;
                    }
                    let child_kind = match e.mode {
                        Mode::Dir => Kind::Tree,
                        Mode::File | Mode::Exec => Kind::Blob,
                    };
                    if visited.insert((e.id, kind_tag(child_kind))) {
                        queue.push_back((e.id, child_kind));
                    }
                }
            }
            Kind::Blob => {}
        }
    }
    true
}

pub(crate) fn is_ancestor<S: Store>(repo: &Repo<S>, ancestor: &Id, descendant: &Id) -> bool {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    visited.insert(*descendant);
    queue.push_back(*descendant);
    while let Some(id) = queue.pop_front() {
        if id == *ancestor {
            return true;
        }
        let Ok((Kind::Commit, data)) = repo.get(&id) else {
            continue;
        };
        let Ok(c) = decode_commit(&data) else {
            continue;
        };
        for p in c.parents {
            if visited.insert(p) {
                queue.push_back(p);
            }
        }
    }
    false
}
