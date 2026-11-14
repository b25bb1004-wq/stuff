//! Object identifiers, hex encoding, object kinds, hashing and errors.

use alloc::{format, string::String, vec::Vec};
use core::fmt;
use onyx::sha256::Sha256;

/// 32-byte SHA-256 object identifier.
pub type Id = [u8; 32];

/// Kind of a stored Git-compatible object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Raw file bytes.
    Blob,
    /// Directory listing of blobs and subtrees.
    Tree,
    /// Snapshot commit with tree, parents, signatures and message.
    Commit,
}

impl Kind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::Blob => "blob",
            Kind::Tree => "tree",
            Kind::Commit => "commit",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s {
            "blob" => Some(Kind::Blob),
            "tree" => Some(Kind::Tree),
            "commit" => Some(Kind::Commit),
            _ => None,
        }
    }
}

/// Errors returned by drillcore operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Requested object, branch, path or revision does not exist.
    NotFound,
    /// Stored object, index, ref or encoded structure is malformed or fails hash check.
    Corrupt,
    /// Path or branch name violates naming rules.
    BadName,
    /// Target branch already exists.
    Exists,
    /// Operation requires at least one commit.
    NoCommits,
    /// Working index has uncommitted changes or target is currently checked out.
    Dirty,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Error::NotFound => "not found",
            Error::Corrupt => "corrupt object or repository state",
            Error::BadName => "invalid path or branch name",
            Error::Exists => "already exists",
            Error::NoCommits => "no commits",
            Error::Dirty => "dirty index or active branch",
        };
        f.write_str(msg)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Format an `Id` as 64 lowercase hexadecimal characters.
pub fn to_hex(id: &Id) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in id {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Parse a 64-character hexadecimal string into an `Id`.
pub fn from_hex(s: &str) -> Option<Id> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Compute the Git SHA-256 object id for `kind` and `data`.
pub fn hash_object(kind: Kind, data: &[u8]) -> Id {
    let header = format!("{} {}\0", kind.as_str(), data.len());
    let mut hasher = Sha256::new();
    hasher.update(header.as_bytes());
    hasher.update(data);
    hasher.finalize()
}

pub(crate) fn encode_raw_object(kind: Kind, data: &[u8]) -> Vec<u8> {
    let header = format!("{} {}\0", kind.as_str(), data.len());
    let mut out = Vec::with_capacity(header.len().saturating_add(data.len()));
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(data);
    out
}

pub(crate) fn decode_raw_object(raw: &[u8]) -> Result<(Kind, &[u8]), Error> {
    let nul = raw.iter().position(|&b| b == 0).ok_or(Error::Corrupt)?;
    let header = core::str::from_utf8(&raw[..nul]).map_err(|_| Error::Corrupt)?;
    let (kind_str, len_str) = header.split_once(' ').ok_or(Error::Corrupt)?;
    let kind = Kind::from_str(kind_str).ok_or(Error::Corrupt)?;
    let len = len_str.parse::<usize>().map_err(|_| Error::Corrupt)?;
    let body = &raw[nul + 1..];
    if body.len() != len {
        return Err(Error::Corrupt);
    }
    Ok((kind, body))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn hex_roundtrip_and_invalid() {
        let mut id = [0u8; 32];
        for (i, b) in id.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(17);
        }
        let hex = to_hex(&id);
        assert_eq!(hex.len(), 64);
        assert_eq!(from_hex(&hex), Some(id));
        assert_eq!(from_hex(&hex.to_uppercase()), Some(id));
        assert_eq!(from_hex("abcd"), None);
        assert_eq!(from_hex(&"z".repeat(64)), None);
    }

    #[test]
    fn known_git_sha256_ids() {
        // Git SHA-256 empty blob: sha256("blob 0\0")
        let empty_blob = hash_object(Kind::Blob, b"");
        assert_eq!(
            to_hex(&empty_blob),
            "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813"
        );
        // Git SHA-256 "hello\n" blob: sha256("blob 6\0hello\n")
        let hello_blob = hash_object(Kind::Blob, b"hello\n");
        assert_eq!(
            to_hex(&hello_blob),
            "2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4"
        );
        // Git SHA-256 empty tree: sha256("tree 0\0")
        let empty_tree = hash_object(Kind::Tree, b"");
        assert_eq!(
            to_hex(&empty_tree),
            "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321"
        );
    }
}
