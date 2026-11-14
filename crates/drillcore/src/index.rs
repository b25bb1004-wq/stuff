//! Index serialization and recursive tree construction/flattening.

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    format::{decode_tree, encode_tree, Mode, TreeEntry},
    id::{Error, Id, Kind},
    repo::Repo,
    store::{validate_path, Store},
};

pub(crate) type IndexMap = BTreeMap<String, (Id, bool)>;

pub(crate) fn read_index(store: &impl Store) -> Result<IndexMap, Error> {
    let Some(raw) = store.read("index") else {
        return Ok(BTreeMap::new());
    };
    let mut data = raw.as_slice();
    let mut map = BTreeMap::new();
    while !data.is_empty() {
        let exec = match data[0] {
            b'0' => false,
            b'1' => true,
            _ => return Err(Error::Corrupt),
        };
        let rest = &data[1..];
        let id_bytes = rest.get(..32).ok_or(Error::Corrupt)?;
        let mut id = [0u8; 32];
        id.copy_from_slice(id_bytes);
        let after_id = &rest[32..];
        let nul = after_id
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::Corrupt)?;
        let path = core::str::from_utf8(&after_id[..nul]).map_err(|_| Error::Corrupt)?;
        validate_path(path).map_err(|_| Error::Corrupt)?;
        map.insert(path.to_string(), (id, exec));
        data = &after_id[nul + 1..];
    }
    Ok(map)
}

pub(crate) fn write_index(store: &mut impl Store, index: &IndexMap) {
    let mut out = Vec::new();
    for (path, (id, exec)) in index {
        out.push(if *exec { b'1' } else { b'0' });
        out.extend_from_slice(id);
        out.extend_from_slice(path.as_bytes());
        out.push(0);
    }
    store.write("index", &out);
}

pub(crate) fn build_tree_from_index<S: Store>(
    repo: &mut Repo<S>,
    index: &IndexMap,
) -> Result<Id, Error> {
    let mut files: BTreeMap<String, (Id, bool)> = BTreeMap::new();
    let mut dirs: BTreeMap<String, IndexMap> = BTreeMap::new();

    for (path, &(id, exec)) in index {
        if let Some((head, tail)) = path.split_once('/') {
            dirs.entry(head.to_string())
                .or_default()
                .insert(tail.to_string(), (id, exec));
        } else {
            files.insert(path.clone(), (id, exec));
        }
    }

    let mut entries = Vec::with_capacity(files.len().saturating_add(dirs.len()));
    for (name, (id, exec)) in &files {
        if dirs.contains_key(name) {
            return Err(Error::BadName);
        }
        entries.push(TreeEntry {
            mode: if *exec { Mode::Exec } else { Mode::File },
            name: name.clone(),
            id: *id,
        });
    }
    for (dir_name, sub_index) in dirs {
        let sub_id = build_tree_from_index(repo, &sub_index)?;
        entries.push(TreeEntry {
            mode: Mode::Dir,
            name: dir_name,
            id: sub_id,
        });
    }

    let encoded = encode_tree(&entries);
    Ok(repo.put(Kind::Tree, &encoded))
}

pub(crate) fn flatten_tree_to_index<S: Store>(
    repo: &Repo<S>,
    tree_id: &Id,
    prefix: &str,
    out: &mut IndexMap,
) -> Result<(), Error> {
    let (kind, data) = repo.get(tree_id)?;
    if kind != Kind::Tree {
        return Err(Error::Corrupt);
    }
    let entries = decode_tree(&data)?;
    for entry in entries {
        if entry.name == "." || entry.name == ".." {
            return Err(Error::Corrupt);
        }
        let full_path = if prefix.is_empty() {
            entry.name
        } else {
            format!("{prefix}/{}", entry.name)
        };
        match entry.mode {
            Mode::File => {
                out.insert(full_path, (entry.id, false));
            }
            Mode::Exec => {
                out.insert(full_path, (entry.id, true));
            }
            Mode::Dir => {
                flatten_tree_to_index(repo, &entry.id, &full_path, out)?;
            }
        }
    }
    Ok(())
}
