//! RFC 6962 Merkle tree: roots, audit paths, and RFC 9162 path verification.

use crate::Hash;
use onyx::sha256;

/// SHA-256 of `prefix || parts...`.
fn prefixed(prefix: u8, parts: &[&[u8]]) -> Hash {
    let mut buf = vec![prefix];
    for part in parts {
        buf.extend_from_slice(part);
    }
    sha256::digest(&buf)
}

/// The hash of one leaf: `sha256(0x00 || leaf)`.
fn leaf_hash(leaf: &[u8]) -> Hash {
    prefixed(0x00, &[leaf])
}

/// The hash of an inner node: `sha256(0x01 || left || right)`.
fn node(left: &Hash, right: &Hash) -> Hash {
    prefixed(0x01, &[left.as_slice(), right.as_slice()])
}

/// The largest power of two strictly below `n` (for `n >= 2`).
fn split(n: usize) -> usize {
    1usize << (usize::BITS - 1 - (n - 1).leading_zeros())
}

/// MTH over already-hashed leaves.
fn mth(hashes: &[Hash]) -> Hash {
    match hashes {
        [] => sha256::digest(&[]),
        [only] => *only,
        _ => {
            let (left, right) = hashes.split_at(split(hashes.len()));
            node(&mth(left), &mth(right))
        }
    }
}

/// The audit path for leaf `m`, deepest sibling first.
fn path(hashes: &[Hash], m: usize, out: &mut Vec<Hash>) {
    if let [_, _, ..] = hashes {
        let k = split(hashes.len());
        let (left, right) = hashes.split_at(k);
        if m < k {
            path(left, m, out);
            out.push(mth(right));
        } else {
            path(right, m - k, out);
            out.push(mth(left));
        }
    }
}

fn hash_leaves(leaves: &[Vec<u8>]) -> Vec<Hash> {
    leaves
        .iter()
        .map(|leaf| leaf_hash(leaf.as_slice()))
        .collect()
}

/// The RFC 6962 root of `leaves`; `sha256("")` for no leaves.
pub fn merkle_root(leaves: &[Vec<u8>]) -> Hash {
    mth(&hash_leaves(leaves))
}

/// The audit path proving leaf `index`, deepest sibling first. Empty if `index` is out of range.
pub fn merkle_proof(leaves: &[Vec<u8>], index: usize) -> Vec<Hash> {
    let mut out = Vec::new();
    if index < leaves.len() {
        path(&hash_leaves(leaves), index, &mut out);
    }
    out
}

/// RFC 9162 section 2.1.3.2: does `leaf` at `index` in a tree of `count` leaves with this `root`
/// follow from `proof`?
pub fn merkle_verify(root: &Hash, leaf: &[u8], index: usize, count: usize, proof: &[Hash]) -> bool {
    if index >= count {
        return false;
    }
    let mut fnum = index as u64;
    let mut snum = (count - 1) as u64;
    let mut r = leaf_hash(leaf);
    for c in proof {
        if snum == 0 {
            return false;
        }
        if fnum & 1 == 1 || fnum == snum {
            r = node(c, &r);
            if fnum & 1 == 0 {
                while fnum & 1 == 0 && fnum != 0 {
                    fnum >>= 1;
                    snum >>= 1;
                }
            }
        } else {
            r = node(&r, c);
        }
        fnum >>= 1;
        snum >>= 1;
    }
    snum == 0 && r == *root
}
