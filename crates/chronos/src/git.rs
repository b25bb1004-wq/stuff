//! Import and export through git's own fast-export and fast-import streams.
//!
//! Spawns the `git` binary (no crates). `import` turns a git repository's history into chronos
//! objects; `export` appends chronos commits to a git repository. The store file `git-map` records
//! chronos id <-> git sha for every commit and the last synced value of each branch and tag, so a
//! second run only touches what changed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::format::{decode_commit, encode_commit, Commit, Signature};
use crate::id::{from_hex, to_hex, Error as RepoError, Id, Kind};
use crate::index::{build_tree_from_index, flatten_tree_to_index, write_index, IndexMap};
use crate::repo::Repo;
use crate::store::{validate_branch_name, validate_path, Store};

const MAP_PATH: &str = "git-map";
/// Temporary ref that carries every new commit in one fast-import run; deleted afterwards.
const TMP_REF: &str = "refs/chronos/export-tmp";

/// Failure during import or export. The message names the step that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError(String);

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GitError {}

impl From<io::Error> for GitError {
    fn from(e: io::Error) -> Self {
        GitError(e.to_string())
    }
}

impl From<RepoError> for GitError {
    fn from(e: RepoError) -> Self {
        GitError(format!("repository: {e}"))
    }
}

fn fail<T>(msg: impl Into<String>) -> Result<T, GitError> {
    Err(GitError(msg.into()))
}

fn eof() -> GitError {
    GitError("unexpected end of fast-export stream".to_string())
}

/// What an import or export did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Commits written: new chronos commits on import, new git commits on export.
    pub commits: usize,
    /// Branch and tag refs written.
    pub refs: usize,
    /// Things skipped or kept in a weaker form, one line each.
    pub warnings: Vec<String>,
}

fn git_command(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    cmd
}

/// Run git and return trimmed stdout, or `None` when git exits non-zero.
fn git_opt(dir: &Path, args: &[&str]) -> Result<Option<String>, GitError> {
    let out = git_command(dir)
        .args(args)
        .output()
        .map_err(|e| GitError(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// Run git and return trimmed stdout, failing with git's stderr when it exits non-zero.
fn git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = git_command(dir)
        .args(args)
        .output()
        .map_err(|e| GitError(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return fail(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Read a child's stderr on a thread so a full pipe cannot stall the stdout stream.
fn drain_stderr(err: Option<std::process::ChildStderr>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut e) = err {
            let _ = e.read_to_string(&mut text);
        }
        text
    })
}

/// chronos id <-> git sha for commits, plus the last synced value of each branch and tag.
#[derive(Default)]
struct GitMap {
    git_to_chronos: HashMap<String, Id>,
    chronos_to_git: HashMap<Id, String>,
    refs: BTreeMap<String, Id>,
}

impl GitMap {
    fn load(store: &impl Store) -> Result<Self, GitError> {
        let mut map = Self::default();
        let Some(raw) = store.read(MAP_PATH) else {
            return Ok(map);
        };
        let text = String::from_utf8(raw).map_err(|_| corrupt_map())?;
        for line in text.lines() {
            let parts: Vec<&str> = line.split(' ').collect();
            match parts.as_slice() {
                ["commit", sha, hex] => {
                    let id = from_hex(hex).ok_or_else(corrupt_map)?;
                    map.link(sha.to_string(), id);
                }
                ["ref", name, hex] => {
                    let id = from_hex(hex).ok_or_else(corrupt_map)?;
                    map.refs.insert(name.to_string(), id);
                }
                _ => return Err(corrupt_map()),
            }
        }
        Ok(map)
    }

    fn link(&mut self, sha: String, id: Id) {
        self.chronos_to_git.insert(id, sha.clone());
        self.git_to_chronos.insert(sha, id);
    }

    fn save(&self, store: &mut impl Store) -> Result<(), GitError> {
        let mut commits: Vec<(&String, &Id)> = self.git_to_chronos.iter().collect();
        commits.sort();
        let mut out = String::new();
        for (sha, id) in commits {
            let _ = writeln!(out, "commit {sha} {}", to_hex(id));
        }
        for (name, id) in &self.refs {
            let _ = writeln!(out, "ref {name} {}", to_hex(id));
        }
        store.write(MAP_PATH, out.as_bytes());
        if store.read(MAP_PATH).as_deref() != Some(out.as_bytes()) {
            return fail("cannot write .chronos/git-map");
        }
        Ok(())
    }
}

fn corrupt_map() -> GitError {
    GitError("corrupt git-map file in the chronos repository".to_string())
}

/// Import git's full history (all branches and tags) into `repo`, then check out HEAD.
///
/// Writes objects and refs into `repo` and fills the index with HEAD's tree. Remote-tracking refs,
/// notes and stashes are not imported. Annotated tags become plain refs to their commit.
pub fn import<S: Store>(repo: &mut Repo<S>, git_dir: &Path) -> Result<Summary, GitError> {
    let mut map = GitMap::load(&repo.store)?;
    let head_sha = git_opt(git_dir, &["rev-parse", "--verify", "-q", "HEAD"])?;
    let head_ref = git_opt(git_dir, &["symbolic-ref", "-q", "HEAD"])?;

    let mut child = git_command(git_dir)
        .args([
            "fast-export",
            "--all",
            "--signed-tags=strip",
            "--reencode=yes",
            "--show-original-ids",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError(format!("cannot run git fast-export: {e}")))?;
    let errors = drain_stderr(child.stderr.take());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitError("no stdout from git fast-export".to_string()))?;

    let mut stream = Stream {
        input: BufReader::new(stdout),
        pending: None,
    };
    let mut st = ImportState::default();
    let parsed = parse_stream(&mut stream, repo, &mut map, &mut st);
    drop(stream);
    let status = child.wait()?;
    let stderr_text = errors.join().unwrap_or_default();
    parsed?;
    if !status.success() {
        return fail(format!("git fast-export failed: {}", stderr_text.trim()));
    }

    let mut summary = Summary {
        commits: st.new_commits,
        refs: 0,
        warnings: st.warnings,
    };
    if st.saw_notes {
        summary
            .warnings
            .push("git notes were not imported".to_string());
    }
    let mut kept = BTreeMap::new();
    for (name, id) in st.refs {
        let valid = match name.strip_prefix("refs/heads/") {
            Some(branch) => validate_branch_name(branch).is_ok(),
            None => match name.strip_prefix("refs/tags/") {
                Some(tag) => validate_branch_name(tag).is_ok(),
                None => false,
            },
        };
        if valid {
            repo.store
                .write(&name, format!("{}\n", to_hex(&id)).as_bytes());
            kept.insert(name, id);
        } else {
            summary.warnings.push(format!(
                "ref {name} has a name chronos cannot store; skipped"
            ));
        }
    }
    summary.refs = kept.len();

    let head_line = match (head_ref, head_sha) {
        (Some(r), _) if kept.contains_key(&r) => Some(format!("ref: {r}\n")),
        (None, Some(sha)) => map
            .git_to_chronos
            .get(&sha)
            .map(|id| format!("{}\n", to_hex(id))),
        _ => None,
    };
    if let Some(line) = head_line {
        repo.store.write("HEAD", line.as_bytes());
    }

    let mut index = IndexMap::new();
    if let Some(head) = repo.head() {
        let commit = read_commit(repo, &head)?;
        flatten_tree_to_index(repo, &commit.tree, "", &mut index)?;
    }
    write_index(&mut repo.store, &index);

    map.refs = kept;
    map.save(&mut repo.store)?;
    Ok(summary)
}

#[derive(Default)]
struct ImportState {
    /// fast-export blob mark -> chronos blob id.
    blobs: HashMap<u64, Id>,
    /// fast-export commit mark -> (chronos commit id, full file index at that commit).
    commits: HashMap<u64, (Id, IndexMap)>,
    /// Full ref name -> chronos commit id, in stream order (last value wins).
    refs: BTreeMap<String, Id>,
    new_commits: usize,
    saw_notes: bool,
    warnings: Vec<String>,
}

/// Reader over the fast-export stream with one line of pushback.
struct Stream<R: BufRead> {
    input: R,
    pending: Option<Vec<u8>>,
}

impl<R: BufRead> Stream<R> {
    fn line(&mut self) -> Result<Option<Vec<u8>>, GitError> {
        if let Some(line) = self.pending.take() {
            return Ok(Some(line));
        }
        let mut buf = Vec::new();
        if self.input.read_until(b'\n', &mut buf)? == 0 {
            return Ok(None);
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        Ok(Some(buf))
    }

    fn unread(&mut self, line: Vec<u8>) {
        self.pending = Some(line);
    }

    /// Read the body of a counted `data <n>` command whose header line was already read.
    fn data(&mut self, header: &[u8]) -> Result<Vec<u8>, GitError> {
        let n: usize = header
            .strip_prefix(b"data ")
            .and_then(|rest| std::str::from_utf8(rest).ok())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| GitError("unsupported data header in fast-export stream".to_string()))?;
        let mut body = vec![0u8; n];
        self.input.read_exact(&mut body)?;
        if self.input.fill_buf()?.first() == Some(&b'\n') {
            self.input.consume(1);
        }
        Ok(body)
    }
}

fn is_command(line: &[u8]) -> bool {
    [
        &b"commit "[..],
        b"blob",
        b"tag ",
        b"reset ",
        b"done",
        b"progress",
        b"feature",
        b"option",
        b"checkpoint",
    ]
    .iter()
    .any(|c| line.starts_with(c))
}

fn parse_stream<R: BufRead, S: Store>(
    input: &mut Stream<R>,
    repo: &mut Repo<S>,
    map: &mut GitMap,
    st: &mut ImportState,
) -> Result<(), GitError> {
    while let Some(line) = input.line()? {
        if line.is_empty() {
            continue;
        }
        if line == b"blob" {
            read_blob(input, repo, st)?;
        } else if let Some(name) = line.strip_prefix(b"commit ") {
            read_commit_cmd(input, repo, map, st, &utf8(name)?)?;
        } else if let Some(name) = line.strip_prefix(b"tag ") {
            read_tag(input, map, st, &utf8(name)?)?;
        } else if let Some(name) = line.strip_prefix(b"reset ") {
            read_reset(input, map, st, &utf8(name)?)?;
        } else if line == b"done" {
            break;
        } else if !is_command(&line) {
            return fail(format!(
                "unexpected line in fast-export stream: {}",
                String::from_utf8_lossy(&line)
            ));
        }
    }
    Ok(())
}

fn utf8(raw: &[u8]) -> Result<String, GitError> {
    String::from_utf8(raw.to_vec())
        .map_err(|_| GitError("git history has a name or message that is not UTF-8".to_string()))
}

fn parse_mark(token: &[u8]) -> Result<u64, GitError> {
    token
        .strip_prefix(b":")
        .and_then(|d| std::str::from_utf8(d).ok())
        .and_then(|d| d.parse().ok())
        .ok_or_else(|| GitError("bad mark in fast-export stream".to_string()))
}

fn read_blob<R: BufRead, S: Store>(
    input: &mut Stream<R>,
    repo: &mut Repo<S>,
    st: &mut ImportState,
) -> Result<(), GitError> {
    let mut mark = None;
    loop {
        let line = input.line()?.ok_or_else(eof)?;
        if let Some(m) = line.strip_prefix(b"mark ") {
            mark = Some(parse_mark(m)?);
        } else if line.starts_with(b"original-oid ") {
            continue;
        } else if line.starts_with(b"data ") {
            let data = input.data(&line)?;
            let id = repo.put(Kind::Blob, &data);
            if let Some(m) = mark {
                st.blobs.insert(m, id);
            }
            return Ok(());
        } else {
            return fail("blob without data in fast-export stream");
        }
    }
}

/// Resolve a `from`/`merge` token (`:mark` in this stream, or a git sha already mapped).
fn resolve(token: &str, map: &GitMap, st: &ImportState) -> Result<Id, GitError> {
    if let Some(mark) = token.strip_prefix(':') {
        let n: u64 = mark
            .parse()
            .map_err(|_| GitError(format!("bad mark {token}")))?;
        return st
            .commits
            .get(&n)
            .map(|(id, _)| *id)
            .ok_or_else(|| GitError(format!("mark {token} is not a known commit")));
    }
    map.git_to_chronos
        .get(token)
        .copied()
        .ok_or_else(|| GitError(format!("parent {token} is not in the stream or git-map")))
}

fn read_commit_cmd<R: BufRead, S: Store>(
    input: &mut Stream<R>,
    repo: &mut Repo<S>,
    map: &mut GitMap,
    st: &mut ImportState,
    refname: &str,
) -> Result<(), GitError> {
    let mut mark = None;
    let mut sha = None;
    let mut author = None;
    let mut committer = None;
    let mut message = None;
    let mut parents: Vec<String> = Vec::new();
    let mut state = IndexMap::new();

    while let Some(line) = input.line()? {
        if line.is_empty() {
            break;
        }
        if let Some(m) = line.strip_prefix(b"mark ") {
            mark = Some(parse_mark(m)?);
        } else if let Some(s) = line.strip_prefix(b"original-oid ") {
            sha = Some(utf8(s)?);
        } else if let Some(who) = line.strip_prefix(b"author ") {
            author = Some(parse_ident(who)?);
        } else if let Some(who) = line.strip_prefix(b"committer ") {
            committer = Some(parse_ident(who)?);
        } else if line.starts_with(b"encoding ") {
            return fail("commit encoding header in stream; expected --reencode=yes output");
        } else if line.starts_with(b"data ") {
            message = Some(input.data(&line)?);
        } else if let Some(tok) = line.strip_prefix(b"from ") {
            let tok = utf8(tok)?;
            if parents.is_empty() {
                // File changes below are relative to the first parent: start from its state.
                let parent_mark = parse_mark(tok.as_bytes())?;
                let (_, parent_state) = st
                    .commits
                    .get(&parent_mark)
                    .ok_or_else(|| GitError(format!("parent {tok} not seen before commit")))?;
                state = parent_state.clone();
            }
            parents.push(tok);
        } else if let Some(tok) = line.strip_prefix(b"merge ") {
            parents.push(utf8(tok)?);
        } else if let Some(rest) = line.strip_prefix(b"M ") {
            let (blob_id, exec, path) = read_modify(input, repo, st, rest)?;
            state.insert(path, (blob_id, exec));
        } else if let Some(rest) = line.strip_prefix(b"D ") {
            let path = unquote(rest)?;
            let prefix = format!("{path}/");
            state.retain(|k, _| *k != path && !k.starts_with(&prefix));
        } else if let Some(rest) = line.strip_prefix(b"R ") {
            rename_paths(&mut state, rest, false)?;
        } else if let Some(rest) = line.strip_prefix(b"C ") {
            rename_paths(&mut state, rest, true)?;
        } else if line == b"deleteall" {
            state.clear();
        } else if line.starts_with(b"N ") {
            st.saw_notes = true;
        } else if is_command(&line) {
            input.unread(line);
            break;
        } else {
            return fail(format!(
                "unexpected line in commit: {}",
                String::from_utf8_lossy(&line)
            ));
        }
    }

    let mark = mark.ok_or_else(|| GitError("commit without mark".to_string()))?;
    let sha = sha.ok_or_else(|| {
        GitError("fast-export stream lacks original-oid; git is too old".to_string())
    })?;
    let parent_ids = parents
        .iter()
        .map(|p| resolve(p, map, st))
        .collect::<Result<Vec<Id>, GitError>>()?;

    let id = match map.git_to_chronos.get(&sha) {
        Some(&known) => known,
        None => {
            let author = author.ok_or_else(|| GitError("commit without author".to_string()))?;
            let committer =
                committer.ok_or_else(|| GitError("commit without committer".to_string()))?;
            let message = utf8(&message.unwrap_or_default())?;
            let tree = build_tree_from_index(repo, &state)?;
            let commit = Commit {
                tree,
                parents: parent_ids,
                author,
                committer,
                message,
            };
            let id = repo.put(Kind::Commit, &encode_commit(&commit));
            map.link(sha, id);
            st.new_commits += 1;
            id
        }
    };
    st.commits.insert(mark, (id, state));
    if refname.starts_with("refs/heads/") || refname.starts_with("refs/tags/") {
        st.refs.insert(refname.to_string(), id);
    }
    Ok(())
}

/// Parse `M <mode> <dataref> <path>`. `rest` is everything after `M `.
fn read_modify<R: BufRead, S: Store>(
    input: &mut Stream<R>,
    repo: &mut Repo<S>,
    st: &ImportState,
    rest: &[u8],
) -> Result<(Id, bool, String), GitError> {
    let mut parts = rest.splitn(3, |b| *b == b' ');
    let mode = parts.next().unwrap_or_default();
    let dataref = parts
        .next()
        .ok_or_else(|| GitError("M line without data reference".to_string()))?;
    let path_raw = parts
        .next()
        .ok_or_else(|| GitError("M line without path".to_string()))?;
    let exec = match mode {
        b"100644" => false,
        b"100755" => true,
        other => {
            return fail(format!(
                "unsupported file mode {} in git history (only regular files)",
                String::from_utf8_lossy(other)
            ))
        }
    };
    let blob_id = if dataref == b"inline" {
        let header = input.line()?.ok_or_else(eof)?;
        if !header.starts_with(b"data ") {
            return fail("inline M without data");
        }
        let data = input.data(&header)?;
        repo.put(Kind::Blob, &data)
    } else {
        let mark = parse_mark(dataref)?;
        *st.blobs
            .get(&mark)
            .ok_or_else(|| GitError("M refers to an unknown blob mark".to_string()))?
    };
    let path = unquote(path_raw)?;
    validate_path(&path).map_err(|_| {
        GitError(format!(
            "path {path} cannot be stored in chronos (Windows-unsafe name)"
        ))
    })?;
    Ok((blob_id, exec, path))
}

/// Apply `R old new` (move) or `C old new` (copy) to a file or a directory prefix.
fn rename_paths(state: &mut IndexMap, rest: &[u8], copy: bool) -> Result<(), GitError> {
    let text = utf8(rest)?;
    let (old_raw, new_raw) = match text.split_once(' ') {
        Some(pair) => pair,
        None => return fail("R/C line needs two paths"),
    };
    let old = unquote(old_raw.as_bytes())?;
    let new = unquote(new_raw.as_bytes())?;
    validate_path(&new).map_err(|_| GitError(format!("path {new} cannot be stored in chronos")))?;
    let prefix = format!("{old}/");
    let moved: Vec<(String, (Id, bool))> = state
        .iter()
        .filter(|(k, _)| **k == old || k.starts_with(&prefix))
        .map(|(k, v)| (format!("{new}{}", &k[old.len()..]), *v))
        .collect();
    if !copy {
        state.retain(|k, _| *k != old && !k.starts_with(&prefix));
    }
    state.extend(moved);
    Ok(())
}

fn read_tag<R: BufRead>(
    input: &mut Stream<R>,
    map: &GitMap,
    st: &mut ImportState,
    name: &str,
) -> Result<(), GitError> {
    let mut target = None;
    let mut annotated = false;
    while let Some(line) = input.line()? {
        if line.is_empty() {
            break;
        }
        if let Some(tok) = line.strip_prefix(b"from ") {
            target = Some(utf8(tok)?);
        } else if line.starts_with(b"tagger ") {
            annotated = true;
        } else if line.starts_with(b"data ") {
            input.data(&line)?;
        } else if line.starts_with(b"mark ") || line.starts_with(b"original-oid ") {
            continue;
        } else if is_command(&line) {
            input.unread(line);
            break;
        } else {
            return fail("unexpected line in tag");
        }
    }
    let Some(tok) = target else {
        return Ok(());
    };
    let id = resolve(&tok, map, st)?;
    if annotated {
        st.warnings.push(format!(
            "annotated tag {name} stored as a plain ref (tagger and message not kept)"
        ));
    }
    st.refs.insert(format!("refs/tags/{name}"), id);
    Ok(())
}

fn read_reset<R: BufRead>(
    input: &mut Stream<R>,
    map: &GitMap,
    st: &mut ImportState,
    refname: &str,
) -> Result<(), GitError> {
    let mut target = None;
    while let Some(line) = input.line()? {
        if line.is_empty() {
            break;
        }
        if let Some(tok) = line.strip_prefix(b"from ") {
            target = Some(utf8(tok)?);
        } else if is_command(&line) {
            input.unread(line);
            break;
        } else {
            return fail("unexpected line in reset");
        }
    }
    if let Some(tok) = target {
        if refname.starts_with("refs/heads/") || refname.starts_with("refs/tags/") {
            let id = resolve(&tok, map, st)?;
            st.refs.insert(refname.to_string(), id);
        }
    }
    Ok(())
}

fn read_commit<S: Store>(repo: &Repo<S>, id: &Id) -> Result<Commit, GitError> {
    let (kind, raw) = repo.get(id)?;
    if kind != Kind::Commit {
        return fail("expected a commit object");
    }
    Ok(decode_commit(&raw)?)
}

/// Parse `Name <email> <unix-time> <+hhmm>`, the part of a fast-export line after the keyword.
fn parse_ident(raw: &[u8]) -> Result<Signature, GitError> {
    let s = utf8(raw)?;
    let bad = || GitError(format!("bad identity in fast-export stream: {s}"));
    let gt = s.rfind('>').ok_or_else(bad)?;
    let lt = s[..gt].rfind('<').ok_or_else(bad)?;
    let name = s[..lt].strip_suffix(' ').unwrap_or(&s[..lt]);
    let email = &s[lt + 1..gt];
    let mut rest = s[gt + 1..].split_whitespace();
    let time: i64 = rest.next().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let tz_minutes = parse_tz(rest.next().unwrap_or(""))?;
    Ok(Signature {
        name: name.to_string(),
        email: email.to_string(),
        time,
        tz_minutes,
    })
}

fn parse_tz(s: &str) -> Result<i32, GitError> {
    let b = s.as_bytes();
    if b.len() != 5 || !matches!(b[0], b'+' | b'-') || !b[1..].iter().all(u8::is_ascii_digit) {
        return fail(format!("bad timezone {s}"));
    }
    let hh: i32 = s[1..3]
        .parse()
        .map_err(|_| GitError(format!("bad timezone {s}")))?;
    let mm: i32 = s[3..5]
        .parse()
        .map_err(|_| GitError(format!("bad timezone {s}")))?;
    let total = hh * 60 + mm;
    Ok(if b[0] == b'-' { -total } else { total })
}

fn fmt_tz(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.abs();
    format!("{sign}{:02}{:02}", abs / 60, abs % 60)
}

/// Decode a fast-import path: plain, or C-style quoted when it starts with `"`.
fn unquote(raw: &[u8]) -> Result<String, GitError> {
    if raw.first() != Some(&b'"') {
        return utf8(raw);
    }
    let bad = || GitError("bad quoted path in fast-export stream".to_string());
    let mut out = Vec::new();
    let mut i = 1;
    while i < raw.len() {
        match raw[i] {
            b'"' => break,
            b'\\' => {
                i += 1;
                let c = *raw.get(i).ok_or_else(bad)?;
                match c {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'v' => out.push(11),
                    b'"' | b'\\' => out.push(c),
                    b'0'..=b'7' => {
                        let mut value = u32::from(c - b'0');
                        for _ in 0..2 {
                            match raw.get(i + 1) {
                                Some(d @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(d - b'0');
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(u8::try_from(value).map_err(|_| bad())?);
                    }
                    _ => return Err(bad()),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| bad())
}

/// Quote a path for fast-import when it has characters that would otherwise be ambiguous.
fn quote(path: &str) -> String {
    if !path.contains(['\n', '"', '\\']) {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for ch in path.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn ident(sig: &Signature) -> String {
    format!(
        "{} <{}> {} {}",
        sig.name,
        sig.email,
        sig.time,
        fmt_tz(sig.tz_minutes)
    )
}

fn write_data<W: Write>(w: &mut W, bytes: &[u8]) -> io::Result<()> {
    writeln!(w, "data {}", bytes.len())?;
    w.write_all(bytes)?;
    w.write_all(b"\n")
}

/// Append chronos history to the git repository at `git_dir` as new git commits.
///
/// Only commits and refs that changed since the last import or export are sent to
/// `git fast-import`. A repeat run with no new chronos work spawns nothing. If the checked-out
/// branch moves, the working tree and index are updated with `read-tree -m -u`; a dirty tree is
/// refused before anything is written.
pub fn export<S: Store>(repo: &mut Repo<S>, git_dir: &Path) -> Result<Summary, GitError> {
    let mut map = GitMap::load(&repo.store)?;
    let mut wanted: BTreeMap<String, Id> = BTreeMap::new();
    for (name, id) in repo.branches() {
        wanted.insert(format!("refs/heads/{name}"), id);
    }
    for path in repo.store.list("refs/tags/") {
        if let Some(id) = repo.store.read(&path).and_then(|raw| {
            std::str::from_utf8(&raw)
                .ok()
                .and_then(|s| from_hex(s.trim()))
        }) {
            wanted.insert(path, id);
        }
    }
    let changed: Vec<(String, Id)> = wanted
        .iter()
        .filter(|(name, id)| map.refs.get(*name) != Some(*id))
        .map(|(name, id)| (name.clone(), *id))
        .collect();
    if changed.is_empty() {
        return Ok(Summary::default());
    }

    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for (_, tip) in &changed {
        collect_new(repo, &map, *tip, &mut seen, &mut order)?;
    }

    let head_ref = git_opt(git_dir, &["symbolic-ref", "-q", "HEAD"])?;
    let moving = head_ref.filter(|r| changed.iter().any(|(name, _)| name == r));
    let mut old_tip = None;
    if let Some(r) = &moving {
        if !git(git_dir, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
            return fail(format!(
                "{r} is checked out in {} with uncommitted changes; commit or stash them, then export again",
                git_dir.display()
            ));
        }
        old_tip = git_opt(git_dir, &["rev-parse", "--verify", "-q", r])?;
    }

    if !order.is_empty() {
        let shas = run_fast_import(repo, &map, git_dir, &order, &changed)?;
        for (id, sha) in order.iter().zip(shas) {
            map.link(sha, *id);
        }
        let _ = git(git_dir, &["update-ref", "-d", TMP_REF]);
    }

    if let Some(r) = &moving {
        let new_sha = git_opt(git_dir, &["rev-parse", "--verify", "-q", r])?;
        if let Some(new_sha) = new_sha.filter(|n| Some(n) != old_tip.as_ref()) {
            let base = match &old_tip {
                Some(old) => old.clone(),
                None => git(git_dir, &["mktree"])?,
            };
            if let Err(e) = git(git_dir, &["read-tree", "-m", "-u", &base, &new_sha]) {
                match &old_tip {
                    Some(old) => {
                        let _ = git(git_dir, &["update-ref", r, old, &new_sha]);
                    }
                    None => {
                        let _ = git(git_dir, &["update-ref", "-d", r]);
                    }
                }
                return fail(format!(
                    "could not update the working tree of {} ({e}); {r} was put back",
                    git_dir.display()
                ));
            }
        }
    }

    map.refs = wanted;
    map.save(&mut repo.store)?;
    Ok(Summary {
        commits: order.len(),
        refs: changed.len(),
        warnings: Vec::new(),
    })
}

/// Walk back from `tip` and push commits that git does not have yet, parents first.
fn collect_new<S: Store>(
    repo: &Repo<S>,
    map: &GitMap,
    tip: Id,
    seen: &mut BTreeSet<Id>,
    order: &mut Vec<Id>,
) -> Result<(), GitError> {
    let mut stack = vec![(tip, false)];
    while let Some((id, expanded)) = stack.pop() {
        if expanded {
            order.push(id);
            continue;
        }
        if map.chronos_to_git.contains_key(&id) || !seen.insert(id) {
            continue;
        }
        let commit = read_commit(repo, &id)?;
        stack.push((id, true));
        for parent in commit.parents.iter().rev() {
            stack.push((*parent, false));
        }
    }
    Ok(())
}

fn ref_token(id: &Id, marks: &HashMap<Id, usize>, map: &GitMap) -> Result<String, GitError> {
    if let Some(n) = marks.get(id) {
        return Ok(format!(":{n}"));
    }
    map.chronos_to_git.get(id).cloned().ok_or_else(|| {
        GitError(format!(
            "commit {} is neither new nor in git-map",
            to_hex(id)
        ))
    })
}

/// Feed the export stream to `git fast-import` and return the git sha of each commit in `order`.
fn run_fast_import<S: Store>(
    repo: &Repo<S>,
    map: &GitMap,
    git_dir: &Path,
    order: &[Id],
    changed: &[(String, Id)],
) -> Result<Vec<String>, GitError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let marks_path = env::temp_dir().join(format!("chronos-marks-{}-{nanos}", std::process::id()));
    let marks_arg = format!(
        "--export-marks={}",
        marks_path.to_string_lossy().replace('\\', "/")
    );
    let mut child = git_command(git_dir)
        .args(["fast-import", "--quiet", &marks_arg])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError(format!("cannot run git fast-import: {e}")))?;
    let errors = drain_stderr(child.stderr.take());
    let written: Result<(), GitError> = match child.stdin.take() {
        Some(stdin) => {
            let mut w = BufWriter::new(stdin);
            write_stream(&mut w, repo, map, order, changed)
                .and_then(|()| w.flush().map_err(GitError::from))
        }
        None => fail("no stdin for git fast-import"),
    };
    let status = child.wait()?;
    let stderr_text = errors.join().unwrap_or_default();
    let marks_text = fs::read_to_string(&marks_path);
    let _ = fs::remove_file(&marks_path);
    if let Err(e) = written {
        return fail(format!("git fast-import: {e} {}", stderr_text.trim()));
    }
    if !status.success() {
        return fail(format!("git fast-import failed: {}", stderr_text.trim()));
    }
    let mut shas = vec![String::new(); order.len()];
    for line in marks_text?.lines() {
        let Some((mark, sha)) = line.split_once(' ') else {
            return fail("bad line in fast-import marks");
        };
        let n: usize = mark
            .strip_prefix(':')
            .and_then(|d| d.parse().ok())
            .ok_or_else(|| GitError("bad mark in fast-import marks".to_string()))?;
        if let Some(slot) = n.checked_sub(1).and_then(|i| shas.get_mut(i)) {
            *slot = sha.to_string();
        }
    }
    if shas.iter().any(String::is_empty) {
        return fail("git fast-import did not report every new commit");
    }
    Ok(shas)
}

fn write_stream<S: Store, W: Write>(
    w: &mut W,
    repo: &Repo<S>,
    map: &GitMap,
    order: &[Id],
    changed: &[(String, Id)],
) -> Result<(), GitError> {
    let marks: HashMap<Id, usize> = order
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i + 1))
        .collect();
    for (i, id) in order.iter().enumerate() {
        let commit = read_commit(repo, id)?;
        writeln!(w, "commit {TMP_REF}")?;
        writeln!(w, "mark :{}", i + 1)?;
        writeln!(w, "author {}", ident(&commit.author))?;
        writeln!(w, "committer {}", ident(&commit.committer))?;
        write_data(w, commit.message.as_bytes())?;
        for (j, parent) in commit.parents.iter().enumerate() {
            let keyword = if j == 0 { "from" } else { "merge" };
            writeln!(w, "{keyword} {}", ref_token(parent, &marks, map)?)?;
        }
        let mut base = IndexMap::new();
        if let Some(parent) = commit.parents.first() {
            let parent_commit = read_commit(repo, parent)?;
            flatten_tree_to_index(repo, &parent_commit.tree, "", &mut base)?;
        }
        let mut next = IndexMap::new();
        flatten_tree_to_index(repo, &commit.tree, "", &mut next)?;
        for (path, (blob, exec)) in &next {
            if base.get(path) != Some(&(*blob, *exec)) {
                let (_, bytes) = repo.get(blob)?;
                let mode = if *exec { "100755" } else { "100644" };
                writeln!(w, "M {mode} inline {}", quote(path))?;
                write_data(w, &bytes)?;
            }
        }
        for path in base.keys() {
            if !next.contains_key(path) {
                writeln!(w, "D {}", quote(path))?;
            }
        }
        writeln!(w)?;
    }
    for (name, id) in changed {
        writeln!(w, "reset {name}")?;
        writeln!(w, "from {}", ref_token(id, &marks, map)?)?;
        writeln!(w)?;
    }
    Ok(())
}
