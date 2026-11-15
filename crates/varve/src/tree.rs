//! Slotted B+tree with overflow pages, range scans, and snapshot support.

mod insert;
mod overflow;
mod page;
mod read;
mod remove;
mod slotted;

use std::ops::Bound;

use crate::pager::{Pager, Snapshot};
use crate::{Error, Storage};

use self::insert::tree_insert;
use self::page::empty_leaf_page;
use self::read::{tree_get, tree_range, tree_scan};
use self::remove::{tree_destroy, tree_remove};

/// Persistent B+tree handle backed by pages in a `Pager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tree(u64);

impl Tree {
    /// Allocate and initialize an empty root leaf page for a new B+tree.
    pub fn create<S: Storage>(p: &mut Pager<S>) -> Result<Tree, Error> {
        p.ensure_tx()?;
        let root = p.alloc()?;
        let page = empty_leaf_page();
        p.write(root, &page)?;
        Ok(Tree(root))
    }

    /// Open an existing B+tree rooted at `root`.
    pub fn open(root: u64) -> Tree {
        Tree(root)
    }

    /// Return the fixed root page number of this B+tree.
    pub fn root(&self) -> u64 {
        self.0
    }

    /// Look up `key` at the current view or `at` snapshot, returning its value if present.
    pub fn get<S: Storage>(
        &self,
        p: &mut Pager<S>,
        at: Option<Snapshot>,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, Error> {
        tree_get(p, at, self.0, key)
    }

    /// Insert or replace `key` with `value`, returning the previous value if `key` existed.
    pub fn insert<S: Storage>(
        &self,
        p: &mut Pager<S>,
        key: &[u8],
        value: &[u8],
    ) -> Result<Option<Vec<u8>>, Error> {
        tree_insert(p, self.0, key, value)
    }

    /// Remove `key` from the tree, returning its previous value if present.
    pub fn remove<S: Storage>(
        &self,
        p: &mut Pager<S>,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, Error> {
        tree_remove(p, self.0, key)
    }

    /// Scan entries in ascending key order starting at `from` while `f` returns `true`.
    pub fn scan<S: Storage>(
        &self,
        p: &mut Pager<S>,
        at: Option<Snapshot>,
        from: Bound<&[u8]>,
        f: &mut dyn FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<(), Error> {
        tree_scan(p, at, self.0, from, f)
    }

    /// Collect all entries whose keys lie within `[lo, hi]`, returning empty if `lo > hi`.
    #[allow(clippy::type_complexity)]
    pub fn range<S: Storage>(
        &self,
        p: &mut Pager<S>,
        at: Option<Snapshot>,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error> {
        tree_range(p, at, self.0, lo, hi)
    }

    /// Free every page belonging to this B+tree (interior, leaf, overflow, and root).
    pub fn destroy<S: Storage>(self, p: &mut Pager<S>) -> Result<(), Error> {
        tree_destroy(p, self.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::page::{
        decode_varint, delete_cell_in_place, empty_leaf_page, encode_leaf_cell, encode_varint,
        insert_cell_in_place, read_u16, validate_node, varint_len, KIND_LEAF,
    };
    use super::*;
    use crate::{MemStorage, USABLE};

    #[test]
    fn varint_round_trip_and_reject_non_minimal() {
        let cases = [0u64, 1, 127, 128, 16383, 16384, 1 << 20, u64::MAX];
        for v in cases {
            let mut buf = Vec::new();
            encode_varint(v, &mut buf);
            assert_eq!(buf.len(), varint_len(v));
            let (dec, used) = decode_varint(&buf).unwrap();
            assert_eq!(dec, v);
            assert_eq!(used, buf.len());
        }
        assert_eq!(decode_varint(&[0x80, 0x00]), Err(Error::Corrupt));
        assert_eq!(decode_varint(&[0x81, 0x80, 0x00]), Err(Error::Corrupt));
        let mut too_long = [0x80u8; 10];
        too_long[9] = 0x02;
        assert_eq!(decode_varint(&too_long), Err(Error::Corrupt));
        let mut all_cont = [0x80u8; 10];
        all_cont[9] = 0x81;
        assert_eq!(decode_varint(&all_cont), Err(Error::Corrupt));
    }

    #[test]
    fn slotted_page_defragment_when_gap_insufficient() {
        let mut pg = empty_leaf_page();
        let big = vec![b'x'; 1100];
        for i in 0..3u8 {
            let mut node = validate_node(&pg, true, Some(KIND_LEAF), 10, None, None).unwrap();
            let cell = encode_leaf_cell(&[i], big.len(), &big, None);
            assert!(insert_cell_in_place(&mut pg, &mut node, i as usize, &cell));
        }
        // Delete the middle cell (index 1); content_start stays, fragmented grows.
        let mut node = validate_node(&pg, true, Some(KIND_LEAF), 10, None, None).unwrap();
        let cs_before = node.content_start;
        delete_cell_in_place(&mut pg, &mut node, 1);
        assert_eq!(node.content_start, cs_before);
        assert!(node.fragmented > 1100);

        // Now insert a 1100-byte cell: doesn't fit in gap, fits in gap + fragmented.
        let new_cell = encode_leaf_cell(&[9], big.len(), &big, None);
        assert!(insert_cell_in_place(&mut pg, &mut node, 2, &new_cell));
        let after = validate_node(&pg, true, Some(KIND_LEAF), 10, None, None).unwrap();
        assert_eq!(after.fragmented, 0);
        assert_eq!(after.cell_count, 3);
        assert_eq!(
            usize::from(read_u16(&pg, 16).unwrap()) + (after.cells[0].end - after.cells[0].off),
            USABLE
        );
    }

    #[test]
    fn create_open_root_smoke() {
        let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
        p.begin().unwrap();
        let t = Tree::create(&mut p).unwrap();
        let r = t.root();
        assert_eq!(Tree::open(r), t);
        p.commit().unwrap();
    }
}
