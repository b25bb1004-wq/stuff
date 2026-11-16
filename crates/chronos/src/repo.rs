//! High-level repository operations: staging, commits, branches, checkout and log.

use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    format::{decode_commit, encode_commit, Commit, Signature},
    id::{decode_raw_object, encode_raw_object, from_hex, hash_object, to_hex, Error, Id, Kind},
    index::{build_tree_from_index, flatten_tree_to_index, read_index, write_index, IndexMap},
    store::{validate_branch_name, validate_path, Files, Store},
};

/// A Chronos repository backed by a `Store`.
pub struct Repo<S: Store> {
    /// Underlying key-value store.
    pub store: S,
}

fn object_path(id: &Id) -> String {
    let hex = to_hex(id);
    format!("objects/{}/{}", &hex[..2], &hex[2..])
}

/// True if `path` is `dir` or lies under it. The empty `dir` is the whole tree.
fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path == dir || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// What a switch would do, computed once and shared by `switch_conflicts` and `checkout_keep`.
struct SwitchPlan {
    is_branch: bool,
    target_id: Id,
    /// Index of the target commit.
    target: IndexMap,
    /// Index now.
    current: IndexMap,
    /// Paths whose local state survives the switch.
    keep: BTreeSet<String>,
    /// Kept paths that HEAD and the target disagree on.
    conflicts: Vec<String>,
}

fn parse_head_ref(raw: &[u8]) -> Result<HeadTarget, Error> {
    let s = core::str::from_utf8(raw).map_err(|_| Error::Corrupt)?;
    let trimmed = s.trim_end_matches(['\r', '\n']);
    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
        validate_branch_name(branch).map_err(|_| Error::Corrupt)?;
        Ok(HeadTarget::Branch(branch.to_string()))
    } else if let Some(id) = from_hex(trimmed) {
        Ok(HeadTarget::Detached(id))
    } else {
        Err(Error::Corrupt)
    }
}

enum HeadTarget {
    Branch(String),
    Detached(Id),
}

impl<S: Store> Repo<S> {
    /// Initialize a new repository with `HEAD` pointing to `refs/heads/main`.
    pub fn init(mut store: S) -> Repo<S> {
        store.write("HEAD", b"ref: refs/heads/main\n");
        Repo { store }
    }

    /// Open an existing repository, returning `Corrupt` if `HEAD` is missing or malformed.
    pub fn open(store: S) -> Result<Repo<S>, Error> {
        let head = store.read("HEAD").ok_or(Error::Corrupt)?;
        let _ = parse_head_ref(&head)?;
        Ok(Repo { store })
    }

    /// Store an object of `kind` and `data`, returning its SHA-256 `Id`.
    pub fn put(&mut self, kind: Kind, data: &[u8]) -> Id {
        let id = hash_object(kind, data);
        let raw = encode_raw_object(kind, data);
        self.store.write(&object_path(&id), &raw);
        id
    }

    /// Retrieve and verify an object by `id`, returning `Corrupt` on hash or format mismatch.
    pub fn get(&self, id: &Id) -> Result<(Kind, Vec<u8>), Error> {
        let raw = self.store.read(&object_path(id)).ok_or(Error::NotFound)?;
        let (kind, body) = decode_raw_object(&raw)?;
        if hash_object(kind, body) != *id {
            return Err(Error::Corrupt);
        }
        Ok((kind, body.to_vec()))
    }

    /// Stage a file at `path` with `data` and executable flag `exec`.
    pub fn add(&mut self, path: &str, data: &[u8], exec: bool) -> Result<(), Error> {
        validate_path(path)?;
        let mut index = read_index(&self.store)?;
        for existing in index.keys() {
            if existing != path {
                let prefix_slash = format!("{path}/");
                let existing_slash = format!("{existing}/");
                if existing.starts_with(&prefix_slash) || path.starts_with(&existing_slash) {
                    return Err(Error::BadName);
                }
            }
        }
        let blob_id = self.put(Kind::Blob, data);
        index.insert(path.to_string(), (blob_id, exec));
        write_index(&mut self.store, &index);
        Ok(())
    }

    /// Unstage and remove `path` from the index, returning `NotFound` if absent.
    pub fn rm(&mut self, path: &str) -> Result<(), Error> {
        validate_path(path)?;
        let mut index = read_index(&self.store)?;
        if index.remove(path).is_none() {
            return Err(Error::NotFound);
        }
        write_index(&mut self.store, &index);
        Ok(())
    }

    /// Return all files currently staged in the index.
    pub fn staged(&self) -> Files {
        let Ok(index) = read_index(&self.store) else {
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for (path, (id, exec)) in index {
            if let Ok((Kind::Blob, bytes)) = self.get(&id) {
                out.insert(path, (bytes, exec));
            }
        }
        out
    }

    /// Create a new commit from the current index with `author = committer = who`.
    pub fn commit(&mut self, message: &str, who: &Signature) -> Result<Id, Error> {
        let index = read_index(&self.store)?;
        let tree = build_tree_from_index(self, &index)?;
        let parents = match self.head() {
            Some(h) => alloc::vec![h],
            None => Vec::new(),
        };
        let commit = Commit {
            tree,
            parents,
            author: who.clone(),
            committer: who.clone(),
            message: message.to_string(),
        };
        let commit_bytes = encode_commit(&commit);
        let commit_id = self.put(Kind::Commit, &commit_bytes);

        let head_raw = self.store.read("HEAD").ok_or(Error::Corrupt)?;
        let target = parse_head_ref(&head_raw)?;
        let hex_line = format!("{}\n", to_hex(&commit_id));
        match target {
            HeadTarget::Branch(name) => {
                self.store
                    .write(&format!("refs/heads/{name}"), hex_line.as_bytes());
            }
            HeadTarget::Detached(_) => {
                self.store.write("HEAD", hex_line.as_bytes());
            }
        }
        Ok(commit_id)
    }

    /// Return the commit `Id` pointed to by `HEAD`, or `None` if unborn/missing.
    pub fn head(&self) -> Option<Id> {
        let head_raw = self.store.read("HEAD")?;
        match parse_head_ref(&head_raw).ok()? {
            HeadTarget::Branch(name) => {
                let ref_raw = self.store.read(&format!("refs/heads/{name}"))?;
                let s = core::str::from_utf8(&ref_raw).ok()?.trim();
                from_hex(s)
            }
            HeadTarget::Detached(id) => Some(id),
        }
    }

    /// Return the current branch name, or `None` when `HEAD` is detached.
    pub fn current_branch(&self) -> Option<String> {
        let head_raw = self.store.read("HEAD")?;
        match parse_head_ref(&head_raw).ok()? {
            HeadTarget::Branch(name) => Some(name),
            HeadTarget::Detached(_) => None,
        }
    }

    /// List all branches and their tip commit ids, sorted by branch name.
    pub fn branches(&self) -> Vec<(String, Id)> {
        let mut out = Vec::new();
        for path in self.store.list("refs/heads/") {
            let Some(name) = path.strip_prefix("refs/heads/") else {
                continue;
            };
            let Some(raw) = self.store.read(&path) else {
                continue;
            };
            let Ok(s) = core::str::from_utf8(&raw) else {
                continue;
            };
            if let Some(id) = from_hex(s.trim()) {
                out.push((name.to_string(), id));
            }
        }
        out
    }

    /// Create a new branch `name` pointing at commit `at`.
    pub fn branch(&mut self, name: &str, at: &Id) -> Result<(), Error> {
        validate_branch_name(name)?;
        let ref_path = format!("refs/heads/{name}");
        if self.store.read(&ref_path).is_some() {
            return Err(Error::Exists);
        }
        let (kind, _) = self.get(at)?;
        if kind != Kind::Commit {
            return Err(Error::Corrupt);
        }
        let hex_line = format!("{}\n", to_hex(at));
        self.store.write(&ref_path, hex_line.as_bytes());
        Ok(())
    }

    /// Delete branch `name` (refuses with `Dirty` if it is the current branch).
    pub fn delete_branch(&mut self, name: &str) -> Result<(), Error> {
        validate_branch_name(name)?;
        if self.current_branch().as_deref() == Some(name) {
            return Err(Error::Dirty);
        }
        let ref_path = format!("refs/heads/{name}");
        if self.store.read(&ref_path).is_none() {
            return Err(Error::NotFound);
        }
        self.store.remove(&ref_path);
        Ok(())
    }

    /// Check out a branch name or revision, updating `HEAD` and the index.
    pub fn checkout(&mut self, rev: &str) -> Result<Files, Error> {
        let (is_branch, target_id) = self.checkout_target(rev)?;

        let target_commit = self.read_commit(&target_id)?;
        let cur_index = read_index(&self.store)?;
        let head_index = self.head_index()?;
        if cur_index != head_index {
            return Err(Error::Dirty);
        }

        let mut next_index = BTreeMap::new();
        flatten_tree_to_index(self, &target_commit.tree, "", &mut next_index)?;
        let files = self.materialize_index(&next_index)?;
        write_index(&mut self.store, &next_index);

        if is_branch {
            self.store
                .write("HEAD", format!("ref: refs/heads/{rev}\n").as_bytes());
        } else {
            self.store
                .write("HEAD", format!("{}\n", to_hex(&target_id)).as_bytes());
        }
        Ok(files)
    }

    /// Like `checkout`, but local changes survive when they do not conflict. Paths in `dirty` (the
    /// caller's worktree or index changes) and paths whose index entry differs from `HEAD` are
    /// kept as they are, unless HEAD and `rev` differ on them (`Error::Dirty`). Returns the files
    /// the caller must write to the work tree: every path of `rev` that is not kept.
    pub fn checkout_keep(&mut self, rev: &str, dirty: &BTreeSet<String>) -> Result<Files, Error> {
        let plan = self.plan_switch(rev, dirty)?;
        if !plan.conflicts.is_empty() {
            return Err(Error::Dirty);
        }
        let mut to_write = IndexMap::new();
        for (path, entry) in &plan.target {
            if !plan.keep.contains(path) {
                to_write.insert(path.clone(), *entry);
            }
        }
        let files = self.materialize_index(&to_write)?;
        let mut index = plan.target;
        for path in &plan.keep {
            match plan.current.get(path) {
                Some(entry) => index.insert(path.clone(), *entry),
                None => index.remove(path),
            };
        }
        write_index(&mut self.store, &index);
        self.point_head(rev, plan.is_branch, &plan.target_id);
        Ok(files)
    }

    /// Paths with local changes (from `dirty`) that `checkout_keep(rev)` would overwrite.
    /// An empty list means the switch is allowed.
    pub fn switch_conflicts(
        &self,
        rev: &str,
        dirty: &BTreeSet<String>,
    ) -> Result<Vec<String>, Error> {
        Ok(self.plan_switch(rev, dirty)?.conflicts)
    }

    /// Reset index entries at or under `path` ("" for the whole index) to their `HEAD` version;
    /// entries that HEAD lacks are removed. The work tree is not touched.
    pub fn unstage(&mut self, path: &str) -> Result<(), Error> {
        if !path.is_empty() {
            validate_path(path)?;
        }
        let head = self.head_index()?;
        let mut index = read_index(&self.store)?;
        let matched: BTreeSet<String> = index
            .keys()
            .chain(head.keys())
            .filter(|p| under(p, path))
            .cloned()
            .collect();
        if matched.is_empty() && !path.is_empty() {
            return Err(Error::NotFound);
        }
        for p in matched {
            match head.get(&p) {
                Some(entry) => index.insert(p, *entry),
                None => index.remove(&p),
            };
        }
        write_index(&mut self.store, &index);
        Ok(())
    }

    fn checkout_target(&self, rev: &str) -> Result<(bool, Id), Error> {
        let branch_path = format!("refs/heads/{rev}");
        if let Some(raw) = self.store.read(&branch_path) {
            let s = core::str::from_utf8(&raw).map_err(|_| Error::Corrupt)?;
            Ok((true, from_hex(s.trim()).ok_or(Error::Corrupt)?))
        } else {
            Ok((false, self.resolve(rev).ok_or(Error::NotFound)?))
        }
    }

    fn point_head(&mut self, rev: &str, is_branch: bool, target_id: &Id) {
        if is_branch {
            self.store
                .write("HEAD", format!("ref: refs/heads/{rev}\n").as_bytes());
        } else {
            self.store
                .write("HEAD", format!("{}\n", to_hex(target_id)).as_bytes());
        }
    }

    fn plan_switch(&self, rev: &str, dirty: &BTreeSet<String>) -> Result<SwitchPlan, Error> {
        let (is_branch, target_id) = self.checkout_target(rev)?;
        let target_commit = self.read_commit(&target_id)?;
        let mut target = BTreeMap::new();
        flatten_tree_to_index(self, &target_commit.tree, "", &mut target)?;
        let current = read_index(&self.store)?;
        let head = self.head_index()?;
        let mut keep = dirty.clone();
        for path in current.keys().chain(head.keys()) {
            if current.get(path) != head.get(path) {
                keep.insert(path.clone());
            }
        }
        let conflicts = keep
            .iter()
            .filter(|p| head.get(*p) != target.get(*p))
            .cloned()
            .collect();
        Ok(SwitchPlan {
            is_branch,
            target_id,
            target,
            current,
            keep,
            conflicts,
        })
    }

    /// Resolve `HEAD`, branch name, full hex, unique hex prefix (`>= 4`), `X~n`, or `X^`.
    pub fn resolve(&self, rev: &str) -> Option<Id> {
        if let Some(base) = rev.strip_suffix('^') {
            let id = self.resolve(base)?;
            let c = self.read_commit(&id).ok()?;
            return c.parents.first().copied();
        }
        if let Some((base, n_str)) = rev.rsplit_once('~') {
            if !n_str.is_empty() && n_str.bytes().all(|b| b.is_ascii_digit()) {
                let n = n_str.parse::<usize>().ok()?;
                let mut cur = self.resolve(base)?;
                for _ in 0..n {
                    let c = self.read_commit(&cur).ok()?;
                    cur = *c.parents.first()?;
                }
                return Some(cur);
            }
        }

        if rev == "HEAD" {
            return self.head();
        }
        if let Some(raw) = self.store.read(&format!("refs/heads/{rev}")) {
            let s = core::str::from_utf8(&raw).ok()?.trim();
            return from_hex(s);
        }
        if let Some(id) = from_hex(rev) {
            return self.store.read(&object_path(&id)).map(|_| id);
        }
        if rev.len() >= 4 && rev.len() < 64 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
            let prefix = rev.to_ascii_lowercase();
            let mut matched: Option<Id> = None;
            for path in self.store.list("objects/") {
                let Some(rest) = path.strip_prefix("objects/") else {
                    continue;
                };
                let hex: String = rest.split('/').collect();
                if hex.len() == 64 && hex.starts_with(&prefix) {
                    let id = from_hex(&hex)?;
                    if matched.is_some() {
                        return None;
                    }
                    matched = Some(id);
                }
            }
            return matched;
        }
        None
    }

    /// Return all commits reachable from `from`, newest committer time first, ties by `Id`.
    pub fn log(&self, from: &Id) -> Result<Vec<Id>, Error> {
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::new();
        let mut items: Vec<(i64, Id)> = Vec::new();
        visited.insert(*from);
        queue.push_back(*from);

        while let Some(id) = queue.pop_front() {
            let commit = self.read_commit(&id)?;
            items.push((commit.committer.time, id));
            for p in commit.parents {
                if visited.insert(p) {
                    queue.push_back(p);
                }
            }
        }

        items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(items.into_iter().map(|(_, id)| id).collect())
    }

    /// Return all files in `commit`'s root tree.
    pub fn files(&self, commit: &Id) -> Result<Files, Error> {
        let c = self.read_commit(commit)?;
        let mut index = BTreeMap::new();
        flatten_tree_to_index(self, &c.tree, "", &mut index)?;
        self.materialize_index(&index)
    }

    fn read_commit(&self, id: &Id) -> Result<Commit, Error> {
        let (kind, data) = self.get(id)?;
        if kind != Kind::Commit {
            return Err(Error::Corrupt);
        }
        decode_commit(&data)
    }

    fn head_index(&self) -> Result<IndexMap, Error> {
        let Some(head_id) = self.head() else {
            return Ok(BTreeMap::new());
        };
        let c = self.read_commit(&head_id)?;
        let mut out = BTreeMap::new();
        flatten_tree_to_index(self, &c.tree, "", &mut out)?;
        Ok(out)
    }

    fn materialize_index(&self, index: &IndexMap) -> Result<Files, Error> {
        let mut files = BTreeMap::new();
        for (path, (id, exec)) in index {
            let (kind, bytes) = self.get(id)?;
            if kind != Kind::Blob {
                return Err(Error::Corrupt);
            }
            files.insert(path.clone(), (bytes, *exec));
        }
        Ok(files)
    }
}
