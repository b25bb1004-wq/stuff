//! 3-way line merge (diff3 with zealous conflict trimming) and repository merge.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    diff::{diff_hunks, lines},
    format::{decode_commit, encode_commit, Commit, Signature},
    id::{from_hex, to_hex, Error, Id, Kind},
    index::{build_tree_from_index, flatten_tree_to_index, read_index, write_index, IndexMap},
    repo::Repo,
    store::{validate_branch_name, Store},
};

/// Result of a 3-way line merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// Merged bytes, including conflict markers if `conflicts > 0`.
    pub text: Vec<u8>,
    /// Number of conflict marker blocks in `text`.
    pub conflicts: usize,
}

/// Error returned by [`Repo::merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    /// Underlying repository or object-store error.
    Repo(Error),
    /// Conflicting file paths sorted lexicographically; repository state is unchanged.
    Conflict(Vec<String>),
    /// `other` is already an ancestor of `HEAD` (including `HEAD == other`).
    UpToDate,
    /// Local changes to these paths would be overwritten; nothing was written.
    Local(Vec<String>),
}

/// diff3-style line merge of `ours` and `theirs` against `base`, as `git merge-file -p` with
/// labels `"ours"`, `"base"`, `"theirs"` and the default (merge) conflict style.
pub fn merge3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Merged {
    merge3_labeled(base, ours, theirs, "ours", "theirs")
}

/// As [`merge3`], with the conflict marker labels given by the caller (`HEAD`, a branch name).
pub fn merge3_labeled(
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    ours_label: &str,
    theirs_label: &str,
) -> Merged {
    if ours == theirs {
        return Merged {
            text: ours.to_vec(),
            conflicts: 0,
        };
    }
    if ours == base {
        return Merged {
            text: theirs.to_vec(),
            conflicts: 0,
        };
    }
    if theirs == base {
        return Merged {
            text: ours.to_vec(),
            conflicts: 0,
        };
    }

    let base_lines = lines(base);
    let our_lines = lines(ours);
    let their_lines = lines(theirs);
    let ours_hunks = diff_hunks(&base_lines, &our_lines);
    let theirs_hunks = diff_hunks(&base_lines, &their_lines);

    let mut segs: Vec<MergeSeg> = Vec::new();
    let mut b_pos = 0;
    let mut o_pos = 0;
    let mut t_pos = 0;
    let mut i = 0;
    let mut j = 0;

    while i < ours_hunks.len() || j < theirs_hunks.len() {
        let take_ours_only = j == theirs_hunks.len()
            || (i < ours_hunks.len() && ours_hunks[i].base_end < theirs_hunks[j].base_start);
        if take_ours_only {
            let h = ours_hunks[i];
            let gap = h.base_start - b_pos;
            push_unchanged(&mut segs, o_pos, o_pos + gap);
            if h.side_start < h.side_end {
                segs.push(MergeSeg::Ours(h.side_start, h.side_end));
            }
            t_pos += h.base_end - b_pos;
            b_pos = h.base_end;
            o_pos = h.side_end;
            i += 1;
            continue;
        }

        let take_theirs_only = i == ours_hunks.len()
            || (j < theirs_hunks.len() && theirs_hunks[j].base_end < ours_hunks[i].base_start);
        if take_theirs_only {
            let h = theirs_hunks[j];
            let gap = h.base_start - b_pos;
            push_unchanged(&mut segs, o_pos, o_pos + gap);
            if h.side_start < h.side_end {
                segs.push(MergeSeg::Theirs(h.side_start, h.side_end));
            }
            o_pos += h.base_end - b_pos;
            b_pos = h.base_end;
            t_pos = h.side_end;
            j += 1;
            continue;
        }

        let cur_start = ours_hunks[i].base_start.min(theirs_hunks[j].base_start);
        let mut cur_end = ours_hunks[i].base_end.max(theirs_hunks[j].base_end);
        let gap = cur_start - b_pos;
        push_unchanged(&mut segs, o_pos, o_pos + gap);
        let o_start = o_pos + gap;
        let t_start = t_pos + gap;
        i += 1;
        j += 1;

        loop {
            let mut extended = false;
            if i < ours_hunks.len() && ours_hunks[i].base_start <= cur_end {
                cur_end = cur_end.max(ours_hunks[i].base_end);
                i += 1;
                extended = true;
            }
            if j < theirs_hunks.len() && theirs_hunks[j].base_start <= cur_end {
                cur_end = cur_end.max(theirs_hunks[j].base_end);
                j += 1;
                extended = true;
            }
            if !extended {
                break;
            }
        }

        let last_o = ours_hunks[i - 1];
        let last_t = theirs_hunks[j - 1];
        let o_end = last_o.side_end + (cur_end - last_o.base_end);
        let t_end = last_t.side_end + (cur_end - last_t.base_end);

        refine_conflict(
            &our_lines,
            &their_lines,
            o_start,
            o_end,
            t_start,
            t_end,
            &mut segs,
        );
        b_pos = cur_end;
        o_pos = o_end;
        t_pos = t_end;
    }

    let tail = base_lines.len() - b_pos;
    push_unchanged(&mut segs, o_pos, o_pos + tail);

    let simplified = simplify_conflicts(&our_lines, segs);
    let mut out = Vec::new();
    let mut conflicts = 0;
    for seg in simplified {
        match seg {
            MergeSeg::Unchanged(s, e) | MergeSeg::Ours(s, e) => {
                emit_lines(&mut out, &our_lines[s..e])
            }
            MergeSeg::Theirs(s, e) => emit_lines(&mut out, &their_lines[s..e]),
            MergeSeg::Conflict {
                o_start,
                o_end,
                t_start,
                t_end,
            } => {
                out.extend_from_slice(format!("<<<<<<< {ours_label}\n").as_bytes());
                emit_conflict_side(&mut out, &our_lines[o_start..o_end]);
                out.extend_from_slice(b"=======\n");
                emit_conflict_side(&mut out, &their_lines[t_start..t_end]);
                out.extend_from_slice(format!(">>>>>>> {theirs_label}\n").as_bytes());
                conflicts += 1;
            }
        }
    }
    Merged {
        text: out,
        conflicts,
    }
}

#[derive(Debug, Clone, Copy)]
enum MergeSeg {
    Unchanged(usize, usize),
    Ours(usize, usize),
    Theirs(usize, usize),
    Conflict {
        o_start: usize,
        o_end: usize,
        t_start: usize,
        t_end: usize,
    },
}

fn push_unchanged(segs: &mut Vec<MergeSeg>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    if let Some(MergeSeg::Unchanged(_, prev_end)) = segs.last_mut() {
        if *prev_end == start {
            *prev_end = end;
            return;
        }
    }
    segs.push(MergeSeg::Unchanged(start, end));
}

fn refine_conflict(
    our_lines: &[&[u8]],
    their_lines: &[&[u8]],
    o_start: usize,
    o_end: usize,
    t_start: usize,
    t_end: usize,
    segs: &mut Vec<MergeSeg>,
) {
    let o_slice = &our_lines[o_start..o_end];
    let t_slice = &their_lines[t_start..t_end];
    if o_slice == t_slice {
        if o_start < o_end {
            segs.push(MergeSeg::Ours(o_start, o_end));
        }
        return;
    }

    let subs = diff_hunks(o_slice, t_slice);
    let mut o_cur = 0;
    for h in subs {
        push_unchanged(segs, o_start + o_cur, o_start + h.base_start);
        segs.push(MergeSeg::Conflict {
            o_start: o_start + h.base_start,
            o_end: o_start + h.base_end,
            t_start: t_start + h.side_start,
            t_end: t_start + h.side_end,
        });
        o_cur = h.base_end;
    }
    push_unchanged(segs, o_start + o_cur, o_end);
}

fn simplify_conflicts(our_lines: &[&[u8]], segs: Vec<MergeSeg>) -> Vec<MergeSeg> {
    let mut out: Vec<MergeSeg> = Vec::with_capacity(segs.len());
    for seg in segs {
        if let MergeSeg::Conflict { o_end, t_end, .. } = seg {
            let merge_prev = match out.as_slice() {
                [.., MergeSeg::Conflict { .. }] => Some(false),
                [.., MergeSeg::Conflict { .. }, MergeSeg::Unchanged(us, ue)] => {
                    let gap = &our_lines[*us..*ue];
                    let has_alnum = gap
                        .iter()
                        .any(|line| line.iter().any(|b| b.is_ascii_alphanumeric()));
                    (gap.len() <= 3 || !has_alnum).then_some(true)
                }
                _ => None,
            };
            if let Some(pop_gap) = merge_prev {
                if pop_gap {
                    out.pop();
                }
                if let Some(MergeSeg::Conflict {
                    o_end: pe,
                    t_end: te,
                    ..
                }) = out.last_mut()
                {
                    *pe = o_end;
                    *te = t_end;
                    continue;
                }
            }
        }
        out.push(seg);
    }
    out
}

fn emit_lines(out: &mut Vec<u8>, lines: &[&[u8]]) {
    for line in lines {
        out.extend_from_slice(line);
    }
}

fn emit_conflict_side(out: &mut Vec<u8>, lines: &[&[u8]]) {
    emit_lines(out, lines);
    if let Some(last) = lines.last() {
        if !last.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
}

/// Result of [`Repo::merge_branch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// `other` is already part of HEAD's history; nothing changed.
    UpToDate,
    /// HEAD moved to `other`; no commit was made.
    FastForward(Id),
    /// A merge commit with parents `[HEAD, other]` was made.
    Merged(Id),
    /// Paths that did not merge. The index keeps HEAD's entry for them and MERGE_HEAD is recorded.
    Conflicted(Vec<Conflict>),
}

/// One path that did not merge cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Work-tree path.
    pub path: String,
    /// True when both sides changed the file (markers in `text`); false for modify/delete.
    pub content: bool,
    /// Bytes to write to the work tree: the text with markers, or the surviving side.
    pub text: Vec<u8>,
    /// Executable bit of the file.
    pub exec: bool,
}

/// A merge computed without writing anything.
struct Plan {
    head: Option<Id>,
    other: Id,
    /// HEAD's index, which is also the current index.
    current: IndexMap,
    kind: PlanKind,
}

enum PlanKind {
    UpToDate,
    FastForward(IndexMap),
    Merge {
        index: IndexMap,
        pending: Vec<(String, Vec<u8>, bool)>,
        conflicts: Vec<Conflict>,
    },
}

/// Paths whose entry differs between two indexes (either side may lack the path).
pub(crate) fn changed_paths(a: &IndexMap, b: &IndexMap) -> BTreeSet<String> {
    a.keys()
        .chain(b.keys())
        .filter(|p| a.get(*p) != b.get(*p))
        .cloned()
        .collect()
}

/// `Err(Local)` when a path the operation would write also has local changes.
pub(crate) fn refuse_dirty(
    touched: &BTreeSet<String>,
    dirty: &BTreeSet<String>,
) -> Result<(), MergeError> {
    let hit: Vec<String> = touched.intersection(dirty).cloned().collect();
    if hit.is_empty() {
        Ok(())
    } else {
        Err(MergeError::Local(hit))
    }
}

impl<S: Store> Repo<S> {
    /// Merge branch or rev `other` into the current branch: fast-forward when possible, else a
    /// merge commit with parents `[HEAD, other]` when no file conflicts, else `Err(MergeError::Conflict(paths))`
    /// leaving `HEAD` and the index unchanged. Merge base = the newest common ancestor by committer time.
    pub fn merge(&mut self, other: &str, message: &str, who: &Signature) -> Result<Id, MergeError> {
        let plan = self.plan_merge(other)?;
        if let PlanKind::Merge { conflicts, .. } = &plan.kind {
            if !conflicts.is_empty() {
                let paths = conflicts.iter().map(|c| c.path.clone()).collect();
                return Err(MergeError::Conflict(paths));
            }
        }
        match self.apply_merge(plan, message, who, &BTreeSet::new())? {
            MergeOutcome::UpToDate => Err(MergeError::UpToDate),
            MergeOutcome::FastForward(id) | MergeOutcome::Merged(id) => Ok(id),
            MergeOutcome::Conflicted(_) => Err(MergeError::Repo(Error::Corrupt)),
        }
    }

    /// Merge `other` like [`Repo::merge`], but conflicts are recorded (MERGE_HEAD and
    /// MERGE_UNMERGED) instead of refused. Nothing is written when a path in `dirty` (local
    /// changes) would be touched.
    pub fn merge_branch(
        &mut self,
        other: &str,
        message: &str,
        who: &Signature,
        dirty: &BTreeSet<String>,
    ) -> Result<MergeOutcome, MergeError> {
        let plan = self.plan_merge(other)?;
        self.apply_merge(plan, message, who, dirty)
    }

    /// The commit an unfinished merge will join, if any.
    pub fn merge_head(&self) -> Option<Id> {
        let raw = self.store.read("MERGE_HEAD")?;
        from_hex(core::str::from_utf8(&raw).ok()?.trim())
    }

    /// Paths still unmerged after a conflicted merge.
    pub fn unmerged(&self) -> Vec<String> {
        let Some(raw) = self.store.read("MERGE_UNMERGED") else {
            return Vec::new();
        };
        core::str::from_utf8(&raw)
            .unwrap_or("")
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    }

    /// Forget an unfinished merge and put the index back to HEAD. The caller restores the work tree.
    pub fn abort_merge(&mut self) -> Result<(), Error> {
        self.store.remove("MERGE_HEAD");
        self.store.remove("MERGE_UNMERGED");
        self.unstage("")
    }

    fn plan_merge(&self, other: &str) -> Result<Plan, MergeError> {
        if self.store.read("MERGE_HEAD").is_some() {
            return Err(MergeError::Repo(Error::Dirty));
        }
        let cur_index = read_index(&self.store).map_err(MergeError::Repo)?;
        let head_opt = self.head();
        let head_index = match head_opt {
            Some(id) => tree_index_of(self, &id).map_err(MergeError::Repo)?,
            None => BTreeMap::new(),
        };
        if cur_index != head_index {
            return Err(MergeError::Repo(Error::Dirty));
        }

        let other_id = self
            .resolve(other)
            .ok_or(MergeError::Repo(Error::NotFound))?;
        let other_commit = read_commit_checked(self, &other_id).map_err(MergeError::Repo)?;
        let mut theirs_index = IndexMap::new();
        flatten_tree_to_index(self, &other_commit.tree, "", &mut theirs_index)
            .map_err(MergeError::Repo)?;
        let plan = |kind| Plan {
            head: head_opt,
            other: other_id,
            current: head_index.clone(),
            kind,
        };

        let Some(head_id) = head_opt else {
            return Ok(plan(PlanKind::FastForward(theirs_index)));
        };
        if head_id == other_id {
            return Ok(plan(PlanKind::UpToDate));
        }

        let head_log = self.log(&head_id).map_err(MergeError::Repo)?;
        let other_log = self.log(&other_id).map_err(MergeError::Repo)?;
        let head_ancestors: BTreeSet<Id> = head_log.iter().copied().collect();
        let other_ancestors: BTreeSet<Id> = other_log.iter().copied().collect();

        if head_ancestors.contains(&other_id) {
            return Ok(plan(PlanKind::UpToDate));
        }
        if other_ancestors.contains(&head_id) {
            return Ok(plan(PlanKind::FastForward(theirs_index)));
        }

        let base_id = head_log
            .iter()
            .find(|id| other_ancestors.contains(*id))
            .copied();
        let base_index = match base_id {
            Some(id) => tree_index_of(self, &id).map_err(MergeError::Repo)?,
            None => BTreeMap::new(),
        };
        let (index, pending, conflicts) =
            merge_trees(self, &base_index, &head_index, &theirs_index, other)
                .map_err(MergeError::Repo)?;
        Ok(plan(PlanKind::Merge {
            index,
            pending,
            conflicts,
        }))
    }

    fn apply_merge(
        &mut self,
        plan: Plan,
        message: &str,
        who: &Signature,
        dirty: &BTreeSet<String>,
    ) -> Result<MergeOutcome, MergeError> {
        let Plan {
            head,
            other,
            current,
            kind,
        } = plan;
        match kind {
            PlanKind::UpToDate => Ok(MergeOutcome::UpToDate),
            PlanKind::FastForward(index) => {
                refuse_dirty(&changed_paths(&current, &index), dirty)?;
                write_index(&mut self.store, &index);
                update_head_target(self, &other).map_err(MergeError::Repo)?;
                Ok(MergeOutcome::FastForward(other))
            }
            PlanKind::Merge {
                mut index,
                pending,
                conflicts,
            } => {
                let Some(head_id) = head else {
                    return Err(MergeError::Repo(Error::Corrupt));
                };
                let conflict_paths: Vec<String> =
                    conflicts.iter().map(|c| c.path.clone()).collect();
                let mut touched = changed_paths(&current, &index);
                touched.extend(conflict_paths.iter().cloned());
                refuse_dirty(&touched, dirty)?;

                for (path, bytes, exec) in pending {
                    let blob_id = self.put(Kind::Blob, &bytes);
                    index.insert(path, (blob_id, exec));
                }
                for path in &conflict_paths {
                    match current.get(path) {
                        Some(entry) => index.insert(path.clone(), *entry),
                        None => index.remove(path),
                    };
                }
                if conflicts.is_empty() {
                    let tree = build_tree_from_index(self, &index).map_err(MergeError::Repo)?;
                    let commit = Commit {
                        tree,
                        parents: alloc::vec![head_id, other],
                        author: who.clone(),
                        committer: who.clone(),
                        message: message.to_string(),
                    };
                    let commit_id = self.put(Kind::Commit, &encode_commit(&commit));
                    write_index(&mut self.store, &index);
                    update_head_target(self, &commit_id).map_err(MergeError::Repo)?;
                    return Ok(MergeOutcome::Merged(commit_id));
                }
                write_index(&mut self.store, &index);
                self.store
                    .write("MERGE_HEAD", format!("{}\n", to_hex(&other)).as_bytes());
                let mut unmerged = String::new();
                for path in &conflict_paths {
                    unmerged.push_str(path);
                    unmerged.push('\n');
                }
                self.store.write("MERGE_UNMERGED", unmerged.as_bytes());
                Ok(MergeOutcome::Conflicted(conflicts))
            }
        }
    }
}

type TreeMergePlan = (IndexMap, Vec<(String, Vec<u8>, bool)>, Vec<Conflict>);

/// Three-way merge of two indexes against `base`. `label` names `theirs` in conflict markers.
pub(crate) fn merge_trees<S: Store>(
    repo: &Repo<S>,
    base: &IndexMap,
    ours: &IndexMap,
    theirs: &IndexMap,
    label: &str,
) -> Result<TreeMergePlan, Error> {
    let mut paths = BTreeSet::new();
    for p in base.keys().chain(ours.keys()).chain(theirs.keys()) {
        paths.insert(p);
    }

    let mut merged = IndexMap::new();
    let mut pending_blobs = Vec::new();
    let mut conflicts: BTreeMap<String, Conflict> = BTreeMap::new();

    for path in paths {
        let b = base.get(path).copied();
        let o = ours.get(path).copied();
        let t = theirs.get(path).copied();

        if o == t {
            if let Some(entry) = o {
                merged.insert(path.clone(), entry);
            }
        } else if o == b {
            if let Some(entry) = t {
                merged.insert(path.clone(), entry);
            }
        } else if t == b {
            if let Some(entry) = o {
                merged.insert(path.clone(), entry);
            }
        } else {
            match (b, o, t) {
                (Some((b_id, b_exec)), Some((o_id, o_exec)), Some((t_id, t_exec))) => {
                    let exec = if o_exec == t_exec {
                        o_exec
                    } else if o_exec == b_exec {
                        t_exec
                    } else {
                        o_exec
                    };
                    if o_id == t_id {
                        merged.insert(path.clone(), (o_id, exec));
                    } else if o_id == b_id {
                        merged.insert(path.clone(), (t_id, exec));
                    } else if t_id == b_id {
                        merged.insert(path.clone(), (o_id, exec));
                    } else {
                        let b_bytes = read_blob(repo, &b_id)?;
                        let o_bytes = read_blob(repo, &o_id)?;
                        let t_bytes = read_blob(repo, &t_id)?;
                        let res = merge3_labeled(&b_bytes, &o_bytes, &t_bytes, "HEAD", label);
                        if res.conflicts > 0 {
                            conflicts.insert(path.clone(), content_conflict(path, res.text, exec));
                        } else {
                            pending_blobs.push((path.clone(), res.text, exec));
                        }
                    }
                }
                (None, Some((o_id, o_exec)), Some((t_id, _))) => {
                    // Added on both sides: merge against an empty base.
                    let o_bytes = read_blob(repo, &o_id)?;
                    let t_bytes = read_blob(repo, &t_id)?;
                    let res = merge3_labeled(&[], &o_bytes, &t_bytes, "HEAD", label);
                    if res.conflicts > 0 {
                        conflicts.insert(path.clone(), content_conflict(path, res.text, o_exec));
                    } else {
                        pending_blobs.push((path.clone(), res.text, o_exec));
                    }
                }
                _ => {
                    // Modify/delete: keep the side that still has the file.
                    if let Some((id, exec)) = o.or(t) {
                        let bytes = read_blob(repo, &id)?;
                        conflicts.insert(
                            path.clone(),
                            Conflict {
                                path: path.clone(),
                                content: false,
                                text: bytes,
                                exec,
                            },
                        );
                    }
                }
            }
        }
    }

    // A file and a directory cannot both survive at one path: name both sides as conflicts.
    let all_surviving: Vec<String> = merged
        .keys()
        .cloned()
        .chain(pending_blobs.iter().map(|(p, _, _)| p.clone()))
        .collect();
    let mut clashing = BTreeSet::new();
    for a in &all_surviving {
        let prefix = format!("{a}/");
        for p in &all_surviving {
            if p.starts_with(&prefix) {
                clashing.insert(a.clone());
                clashing.insert(p.clone());
            }
        }
    }
    for p in clashing {
        if conflicts.contains_key(&p) {
            continue;
        }
        let (text, exec) = match merged.get(&p).copied() {
            Some((id, exec)) => (read_blob(repo, &id)?, exec),
            None => match pending_blobs.iter().find(|(q, _, _)| *q == p) {
                Some((_, bytes, exec)) => (bytes.clone(), *exec),
                None => continue,
            },
        };
        conflicts.insert(
            p.clone(),
            Conflict {
                path: p,
                content: false,
                text,
                exec,
            },
        );
    }
    merged.retain(|p, _| !conflicts.contains_key(p));
    pending_blobs.retain(|(p, _, _)| !conflicts.contains_key(p));

    Ok((merged, pending_blobs, conflicts.into_values().collect()))
}

fn content_conflict(path: &str, text: Vec<u8>, exec: bool) -> Conflict {
    Conflict {
        path: path.to_string(),
        content: true,
        text,
        exec,
    }
}

fn read_commit_checked<S: Store>(repo: &Repo<S>, id: &Id) -> Result<Commit, Error> {
    let (kind, data) = repo.get(id)?;
    if kind != Kind::Commit {
        return Err(Error::Corrupt);
    }
    decode_commit(&data)
}

fn read_blob<S: Store>(repo: &Repo<S>, id: &Id) -> Result<Vec<u8>, Error> {
    let (kind, data) = repo.get(id)?;
    if kind != Kind::Blob {
        return Err(Error::Corrupt);
    }
    Ok(data)
}

fn tree_index_of<S: Store>(repo: &Repo<S>, commit_id: &Id) -> Result<IndexMap, Error> {
    let commit = read_commit_checked(repo, commit_id)?;
    let mut index = IndexMap::new();
    flatten_tree_to_index(repo, &commit.tree, "", &mut index)?;
    Ok(index)
}

fn update_head_target<S: Store>(repo: &mut Repo<S>, id: &Id) -> Result<(), Error> {
    let head_raw = repo.store.read("HEAD").ok_or(Error::Corrupt)?;
    let s = core::str::from_utf8(&head_raw).map_err(|_| Error::Corrupt)?;
    let trimmed = s.trim_end_matches(['\r', '\n']);
    let hex_line = format!("{}\n", to_hex(id));
    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
        validate_branch_name(branch).map_err(|_| Error::Corrupt)?;
        repo.store
            .write(&format!("refs/heads/{branch}"), hex_line.as_bytes());
    } else if from_hex(trimmed).is_some() {
        repo.store.write("HEAD", hex_line.as_bytes());
    } else {
        return Err(Error::Corrupt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn merge3_clean_and_zealous_conflict() {
        let base = b"a\nb\nc\n";
        let ours = b"a\nx\nc\n";
        let theirs = b"a\nx\nd\n";
        let m = merge3(base, ours, theirs);
        assert_eq!(m.conflicts, 1);
        assert_eq!(
            m.text,
            b"a\nx\n<<<<<<< ours\nc\n=======\nd\n>>>>>>> theirs\n"
        );
    }
}
