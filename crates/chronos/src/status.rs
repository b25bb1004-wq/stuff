//! Working-index status against HEAD.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};

use crate::{
    format::decode_commit,
    id::Kind,
    index::{flatten_tree_to_index, read_index},
    repo::Repo,
    store::Store,
};

/// Staged change relative to the `HEAD` commit tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Path exists in the index but not in `HEAD`.
    Added(String),
    /// Path exists in both with a different blob id or executable flag.
    Modified(String),
    /// Path exists in `HEAD` but was removed from the index.
    Deleted(String),
}

impl<S: Store> Repo<S> {
    /// Staged index vs HEAD tree, sorted by path; all Added when there is no commit.
    pub fn status(&self) -> Vec<Change> {
        let Ok(staged) = read_index(&self.store) else {
            return Vec::new();
        };
        let mut head_map = BTreeMap::new();
        if let Some(head_id) = self.head() {
            if let Ok((Kind::Commit, data)) = self.get(&head_id) {
                if let Ok(commit) = decode_commit(&data) {
                    let _ = flatten_tree_to_index(self, &commit.tree, "", &mut head_map);
                }
            }
        }

        let mut all_paths = BTreeSet::new();
        for p in staged.keys() {
            all_paths.insert(p);
        }
        for p in head_map.keys() {
            all_paths.insert(p);
        }

        let mut out = Vec::new();
        for path in all_paths {
            match (head_map.get(path), staged.get(path)) {
                (None, Some(_)) => out.push(Change::Added(path.clone())),
                (Some(_), None) => out.push(Change::Deleted(path.clone())),
                (Some(h), Some(s)) if h != s => out.push(Change::Modified(path.clone())),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{format::Signature, store::MemStore};

    fn sig(t: i64) -> Signature {
        Signature {
            name: "Alice".into(),
            email: "alice@example.com".into(),
            time: t,
            tz_minutes: 0,
        }
    }

    #[test]
    fn status_before_and_after_commit() {
        let mut repo = Repo::init(MemStore::new());
        assert!(repo.status().is_empty());
        repo.add("b.txt", b"b\n", false).unwrap();
        repo.add("a.txt", b"a\n", false).unwrap();
        assert_eq!(
            repo.status(),
            vec![Change::Added("a.txt".into()), Change::Added("b.txt".into())]
        );

        repo.commit("init", &sig(100)).unwrap();
        assert!(repo.status().is_empty());

        repo.add("a.txt", b"a2\n", false).unwrap();
        repo.rm("b.txt").unwrap();
        repo.add("c.txt", b"c\n", true).unwrap();
        assert_eq!(
            repo.status(),
            vec![
                Change::Modified("a.txt".into()),
                Change::Deleted("b.txt".into()),
                Change::Added("c.txt".into()),
            ]
        );
    }
}
