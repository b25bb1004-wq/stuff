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
}

/// diff3-style line merge of `ours` and `theirs` against `base`, as `git merge-file -p` with
/// labels `"ours"`, `"base"`, `"theirs"` and the default (merge) conflict style.
pub fn merge3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Merged {
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
                out.extend_from_slice(b"<<<<<<< ours\n");
                emit_conflict_side(&mut out, &our_lines[o_start..o_end]);
                out.extend_from_slice(b"=======\n");
                emit_conflict_side(&mut out, &their_lines[t_start..t_end]);
                out.extend_from_slice(b">>>>>>> theirs\n");
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

impl<S: Store> Repo<S> {
    /// Merge branch or rev `other` into the current branch: fast-forward when possible, else a
    /// merge commit with parents `[HEAD, other]` when no file conflicts, else `Err(MergeError::Conflict(paths))`
    /// leaving `HEAD` and the index unchanged. Merge base = the newest common ancestor by committer time.
    pub fn merge(&mut self, other: &str, message: &str, who: &Signature) -> Result<Id, MergeError> {
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

        let Some(head_id) = head_opt else {
            return fast_forward(self, &other_id, &other_commit.tree).map_err(MergeError::Repo);
        };
        if head_id == other_id {
            return Err(MergeError::UpToDate);
        }

        let head_log = self.log(&head_id).map_err(MergeError::Repo)?;
        let other_log = self.log(&other_id).map_err(MergeError::Repo)?;
        let head_ancestors: BTreeSet<Id> = head_log.iter().copied().collect();
        let other_ancestors: BTreeSet<Id> = other_log.iter().copied().collect();

        if head_ancestors.contains(&other_id) {
            return Err(MergeError::UpToDate);
        }
        if other_ancestors.contains(&head_id) {
            return fast_forward(self, &other_id, &other_commit.tree).map_err(MergeError::Repo);
        }

        let base_id = head_log
            .iter()
            .find(|id| other_ancestors.contains(*id))
            .copied();
        let base_index = match base_id {
            Some(id) => tree_index_of(self, &id).map_err(MergeError::Repo)?,
            None => BTreeMap::new(),
        };
        let mut theirs_index = IndexMap::new();
        flatten_tree_to_index(self, &other_commit.tree, "", &mut theirs_index)
            .map_err(MergeError::Repo)?;

        let (merged_index, pending_blobs, conflicts) =
            merge_trees(self, &base_index, &head_index, &theirs_index).map_err(MergeError::Repo)?;
        if !conflicts.is_empty() {
            return Err(MergeError::Conflict(conflicts));
        }

        let mut final_index = merged_index;
        for (path, bytes, exec) in pending_blobs {
            let blob_id = self.put(Kind::Blob, &bytes);
            final_index.insert(path, (blob_id, exec));
        }
        let tree = build_tree_from_index(self, &final_index).map_err(MergeError::Repo)?;
        let commit = Commit {
            tree,
            parents: alloc::vec![head_id, other_id],
            author: who.clone(),
            committer: who.clone(),
            message: message.to_string(),
        };
        let commit_id = self.put(Kind::Commit, &encode_commit(&commit));
        write_index(&mut self.store, &final_index);
        update_head_target(self, &commit_id).map_err(MergeError::Repo)?;
        Ok(commit_id)
    }
}

type TreeMergePlan = (IndexMap, Vec<(String, Vec<u8>, bool)>, Vec<String>);

fn merge_trees<S: Store>(
    repo: &Repo<S>,
    base: &IndexMap,
    ours: &IndexMap,
    theirs: &IndexMap,
) -> Result<TreeMergePlan, Error> {
    let mut paths = BTreeSet::new();
    for p in base.keys().chain(ours.keys()).chain(theirs.keys()) {
        paths.insert(p);
    }

    let mut merged = IndexMap::new();
    let mut pending_blobs = Vec::new();
    let mut conflicts = BTreeSet::new();

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
                        let res = merge3(&b_bytes, &o_bytes, &t_bytes);
                        if res.conflicts > 0 {
                            conflicts.insert(path.clone());
                        } else {
                            pending_blobs.push((path.clone(), res.text, exec));
                        }
                    }
                }
                _ => {
                    conflicts.insert(path.clone());
                }
            }
        }
    }

    let all_surviving: Vec<String> = merged
        .keys()
        .cloned()
        .chain(pending_blobs.iter().map(|(p, _, _)| p.clone()))
        .collect();
    for i in 0..all_surviving.len() {
        let prefix = format!("{}/", all_surviving[i]);
        for p in &all_surviving {
            if p.starts_with(&prefix) {
                conflicts.insert(all_surviving[i].clone());
                conflicts.insert(p.clone());
            }
        }
    }

    Ok((merged, pending_blobs, conflicts.into_iter().collect()))
}

fn fast_forward<S: Store>(repo: &mut Repo<S>, target_id: &Id, tree: &Id) -> Result<Id, Error> {
    let mut next_index = IndexMap::new();
    flatten_tree_to_index(repo, tree, "", &mut next_index)?;
    write_index(&mut repo.store, &next_index);
    update_head_target(repo, target_id)?;
    Ok(*target_id)
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
