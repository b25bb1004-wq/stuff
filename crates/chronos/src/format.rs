//! Git-compatible tree and commit binary/text encoding and decoding.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::cmp::Ordering;

use crate::id::{from_hex, to_hex, Error, Id};

/// File or directory mode in a Git tree entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// Regular non-executable file (`100644`).
    File,
    /// Executable file (`100755`).
    Exec,
    /// Subdirectory tree (`40000`).
    Dir,
}

impl Mode {
    fn as_octal(self) -> &'static str {
        match self {
            Mode::File => "100644",
            Mode::Exec => "100755",
            Mode::Dir => "40000",
        }
    }

    fn from_octal(s: &[u8]) -> Result<Self, Error> {
        match s {
            b"100644" => Ok(Mode::File),
            b"100755" => Ok(Mode::Exec),
            b"40000" => Ok(Mode::Dir),
            _ => Err(Error::Corrupt),
        }
    }
}

/// One entry inside a Git tree object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Entry mode (`File`, `Exec`, or `Dir`).
    pub mode: Mode,
    /// Entry filename (single path component).
    pub name: String,
    /// Referenced blob or subtree object id.
    pub id: Id,
}

pub(crate) fn cmp_tree_entries(a: &TreeEntry, b: &TreeEntry) -> Ordering {
    let a_iter = a
        .name
        .as_bytes()
        .iter()
        .copied()
        .chain((a.mode == Mode::Dir).then_some(b'/'));
    let b_iter = b
        .name
        .as_bytes()
        .iter()
        .copied()
        .chain((b.mode == Mode::Dir).then_some(b'/'));
    a_iter.cmp(b_iter).then_with(|| a.mode.cmp(&b.mode))
}

/// Encode `entries` into Git's binary tree format, sorted in Git tree order.
pub fn encode_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(cmp_tree_entries);
    let mut out = Vec::new();
    for entry in &sorted {
        out.extend_from_slice(entry.mode.as_octal().as_bytes());
        out.push(b' ');
        out.extend_from_slice(entry.name.as_bytes());
        out.push(0);
        out.extend_from_slice(&entry.id);
    }
    out
}

/// Decode a Git binary tree object into its entries.
pub fn decode_tree(mut data: &[u8]) -> Result<Vec<TreeEntry>, Error> {
    let mut entries = Vec::new();
    while !data.is_empty() {
        let sp = data.iter().position(|&b| b == b' ').ok_or(Error::Corrupt)?;
        let mode = Mode::from_octal(&data[..sp])?;
        let rest = &data[sp + 1..];
        let nul = rest.iter().position(|&b| b == 0).ok_or(Error::Corrupt)?;
        let name_bytes = &rest[..nul];
        if name_bytes.is_empty() || name_bytes.contains(&b'/') {
            return Err(Error::Corrupt);
        }
        let name = core::str::from_utf8(name_bytes)
            .map_err(|_| Error::Corrupt)?
            .to_string();
        let after_nul = &rest[nul + 1..];
        let id_bytes = after_nul.get(..32).ok_or(Error::Corrupt)?;
        let mut id = [0u8; 32];
        id.copy_from_slice(id_bytes);
        entries.push(TreeEntry { mode, name, id });
        data = &after_nul[32..];
    }
    Ok(entries)
}

/// Author or committer identity and timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
    /// Unix timestamp in seconds.
    pub time: i64,
    /// Timezone offset east of UTC in minutes.
    pub tz_minutes: i32,
}

/// Git commit object payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Root tree object id.
    pub tree: Id,
    /// Parent commit ids (empty for root commit).
    pub parents: Vec<Id>,
    /// Author signature.
    pub author: Signature,
    /// Committer signature.
    pub committer: Signature,
    /// Commit message stored verbatim.
    pub message: String,
}

fn format_signature(sig: &Signature) -> String {
    let sign = if sig.tz_minutes < 0 { '-' } else { '+' };
    let abs_mins = sig.tz_minutes.unsigned_abs();
    let hh = abs_mins / 60;
    let mm = abs_mins % 60;
    format!(
        "{} <{}> {} {}{:02}{:02}",
        sig.name, sig.email, sig.time, sign, hh, mm
    )
}

fn parse_tz(tz: &str) -> Result<i32, Error> {
    let b = tz.as_bytes();
    if b.len() != 5 {
        return Err(Error::Corrupt);
    }
    let neg = match b[0] {
        b'+' => false,
        b'-' => true,
        _ => return Err(Error::Corrupt),
    };
    let hh: i32 = tz[1..3].parse().map_err(|_| Error::Corrupt)?;
    let mm: i32 = tz[3..5].parse().map_err(|_| Error::Corrupt)?;
    if !(0..60).contains(&mm) {
        return Err(Error::Corrupt);
    }
    let total = hh
        .checked_mul(60)
        .and_then(|v| v.checked_add(mm))
        .ok_or(Error::Corrupt)?;
    Ok(if neg { -total } else { total })
}

fn parse_signature(s: &str) -> Result<Signature, Error> {
    let (left, right) = s.rsplit_once("> ").ok_or(Error::Corrupt)?;
    let (name, email) = left.rsplit_once(" <").ok_or(Error::Corrupt)?;
    let (time_s, tz_s) = right.split_once(' ').ok_or(Error::Corrupt)?;
    let time = time_s.parse::<i64>().map_err(|_| Error::Corrupt)?;
    let tz_minutes = parse_tz(tz_s)?;
    Ok(Signature {
        name: name.to_string(),
        email: email.to_string(),
        time,
        tz_minutes,
    })
}

/// Encode a `Commit` into Git's commit text format.
pub fn encode_commit(c: &Commit) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("tree ");
    out.push_str(&to_hex(&c.tree));
    out.push('\n');
    for p in &c.parents {
        out.push_str("parent ");
        out.push_str(&to_hex(p));
        out.push('\n');
    }
    out.push_str("author ");
    out.push_str(&format_signature(&c.author));
    out.push('\n');
    out.push_str("committer ");
    out.push_str(&format_signature(&c.committer));
    out.push_str("\n\n");
    out.push_str(&c.message);
    out.into_bytes()
}

/// Decode a Git commit object payload into a `Commit`.
pub fn decode_commit(data: &[u8]) -> Result<Commit, Error> {
    let sep = data
        .windows(2)
        .position(|w| w == b"\n\n")
        .ok_or(Error::Corrupt)?;
    let header_str = core::str::from_utf8(&data[..sep]).map_err(|_| Error::Corrupt)?;
    let message = core::str::from_utf8(&data[sep + 2..])
        .map_err(|_| Error::Corrupt)?
        .to_string();

    let mut tree: Option<Id> = None;
    let mut parents = Vec::new();
    let mut author: Option<Signature> = None;
    let mut committer: Option<Signature> = None;

    for line in header_str.split('\n') {
        let (key, val) = line.split_once(' ').ok_or(Error::Corrupt)?;
        match key {
            "tree" => {
                if tree.is_some() {
                    return Err(Error::Corrupt);
                }
                tree = Some(from_hex(val).ok_or(Error::Corrupt)?);
            }
            "parent" => {
                parents.push(from_hex(val).ok_or(Error::Corrupt)?);
            }
            "author" => {
                if author.is_some() {
                    return Err(Error::Corrupt);
                }
                author = Some(parse_signature(val)?);
            }
            "committer" => {
                if committer.is_some() {
                    return Err(Error::Corrupt);
                }
                committer = Some(parse_signature(val)?);
            }
            _ => return Err(Error::Corrupt),
        }
    }

    Ok(Commit {
        tree: tree.ok_or(Error::Corrupt)?,
        parents,
        author: author.ok_or(Error::Corrupt)?,
        committer: committer.ok_or(Error::Corrupt)?,
        message,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use assay::{forall, Rng};

    fn rand_ident(rng: &mut Rng, max_len: usize) -> String {
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789._-";
        let len = rng.range(1, max_len as u64 + 1) as usize;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            s.push(*rng.pick(CHARS) as char);
        }
        s
    }

    fn rand_id(rng: &mut Rng) -> Id {
        let mut id = [0u8; 32];
        rng.fill(&mut id);
        id
    }

    #[test]
    fn tree_git_sort_order_dir_vs_file() {
        let id = [0x11u8; 32];
        let entries = vec![
            TreeEntry {
                mode: Mode::Dir,
                name: "foo".to_string(),
                id,
            },
            TreeEntry {
                mode: Mode::File,
                name: "foo-bar".to_string(),
                id,
            },
            TreeEntry {
                mode: Mode::File,
                name: "foo.txt".to_string(),
                id,
            },
        ];
        let encoded = encode_tree(&entries);
        let decoded = decode_tree(&encoded).unwrap();
        let names: Vec<&str> = decoded.iter().map(|e| e.name.as_str()).collect();
        // '-' (0x2d) < '.' (0x2e) < '/' (0x2f, implicit on Dir "foo")
        assert_eq!(names, vec!["foo-bar", "foo.txt", "foo"]);
    }

    #[test]
    fn tree_roundtrip_forall_300() {
        forall(300, 0x7472_6565, |rng| {
            let n = rng.below(16);
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let mode = *rng.pick(&[Mode::File, Mode::Exec, Mode::Dir]);
                let name = rand_ident(rng, 14);
                let id = rand_id(rng);
                entries.push(TreeEntry { mode, name, id });
            }
            let encoded = encode_tree(&entries);
            let decoded = decode_tree(&encoded).unwrap();
            let mut expected = entries;
            expected.sort_by(cmp_tree_entries);
            assert_eq!(decoded, expected);
        });
    }

    #[test]
    fn commit_roundtrip_forall_300() {
        forall(300, 0x636f_6d6d, |rng| {
            let parent_count = rng.below(4);
            let parents = (0..parent_count).map(|_| rand_id(rng)).collect();
            let tz_sign = if rng.chance(1, 2) { 1 } else { -1 };
            let tz_hh = rng.range(0, 14) as i32;
            let tz_mm = *rng.pick(&[0, 15, 30, 45]);
            let make_sig = |rng: &mut Rng| Signature {
                name: format!("{} {}", rand_ident(rng, 8), rand_ident(rng, 8)),
                email: format!("{}@example.com", rand_ident(rng, 10)),
                time: rng.range(0, 2_000_000_000) as i64,
                tz_minutes: tz_sign * (tz_hh * 60 + tz_mm),
            };
            let author = make_sig(rng);
            let committer = make_sig(rng);
            let msg_lines = rng.range(0, 4);
            let mut message = String::new();
            for i in 0..msg_lines {
                if i > 0 {
                    message.push('\n');
                }
                message.push_str(&rand_ident(rng, 20));
            }
            let commit = Commit {
                tree: rand_id(rng),
                parents,
                author,
                committer,
                message,
            };
            let encoded = encode_commit(&commit);
            let decoded = decode_commit(&encoded).unwrap();
            assert_eq!(decoded, commit);
        });
    }

    #[test]
    fn corrupt_tree_and_commit_return_err() {
        assert_eq!(decode_tree(b"100644 a\0short"), Err(Error::Corrupt));
        assert_eq!(decode_tree(b"999999 a\0"), Err(Error::Corrupt));
        assert_eq!(decode_commit(b"not a commit"), Err(Error::Corrupt));
        assert_eq!(decode_commit(b"tree 00\n\nhi"), Err(Error::Corrupt));
    }
}
