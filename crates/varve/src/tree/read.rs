//! B+tree traversal, point lookups, scans, and range queries.

use std::ops::Bound;

use crate::pager::{Pager, Snapshot};
use crate::tree::overflow::read_leaf_value;
use crate::tree::page::{validate_node, CellMeta, ValidatedNode, KIND_LEAF, MAX_KEY_LEN};
use crate::{Error, Page, Storage};

pub const MAX_DEPTH: usize = 32;

pub struct PathStep {
    pub page_no: u64,
    pub slot: usize,
    pub page: Page,
    pub node: ValidatedNode,
}

pub enum DescentTarget<'a> {
    Key(&'a [u8]),
    Leftmost,
}

pub fn check_bound_len(bound: Bound<&[u8]>) -> Result<(), Error> {
    match bound {
        Bound::Included(k) | Bound::Excluded(k) if k.len() > MAX_KEY_LEN => Err(Error::TooLarge),
        _ => Ok(()),
    }
}

pub fn descend<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    root: u64,
    target: DescentTarget<'_>,
    page_count: u64,
) -> Result<(Vec<PathStep>, u64, Page, ValidatedNode), Error> {
    let mut path = Vec::new();
    let mut cur_no = root;
    let mut lo_bound: Option<Vec<u8>> = None;
    let mut hi_bound: Option<Vec<u8>> = None;

    for depth in 1..=MAX_DEPTH {
        let page = p.read_view(at, cur_no)?;
        let node = validate_node(
            &page,
            depth == 1,
            None,
            page_count,
            lo_bound.as_deref(),
            hi_bound.as_deref(),
        )?;
        if node.kind == KIND_LEAF {
            return Ok((path, cur_no, page, node));
        }
        if depth == MAX_DEPTH {
            return Err(Error::Corrupt);
        }
        let slot = match target {
            DescentTarget::Leftmost => 0,
            DescentTarget::Key(k) => node.child_slot(&page, k),
        };
        if slot > 0 {
            lo_bound = Some(node.key(&page, slot - 1).to_vec());
        }
        if slot < node.cell_count {
            hi_bound = Some(node.key(&page, slot).to_vec());
        }
        let next_no = node.child(slot);
        path.push(PathStep {
            page_no: cur_no,
            slot,
            page,
            node,
        });
        cur_no = next_no;
    }
    Err(Error::Corrupt)
}

pub fn tree_get<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    root: u64,
    key: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    if key.len() > MAX_KEY_LEN {
        return Err(Error::TooLarge);
    }
    let page_count = p.view_page_count(at)?;
    let (_, _, page, node) = descend(p, at, root, DescentTarget::Key(key), page_count)?;
    match node.search(&page, key) {
        Ok(idx) => Ok(Some(read_leaf_value(
            p,
            at,
            &page,
            &node.cells[idx],
            page_count,
        )?)),
        Err(_) => Ok(None),
    }
}

fn scan_cells<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    root: u64,
    from: Bound<&[u8]>,
    page_count: u64,
    mut on_cell: impl FnMut(&mut Pager<S>, &[u8], &Page, &CellMeta) -> Result<bool, Error>,
) -> Result<(), Error> {
    let target = match from {
        Bound::Unbounded => DescentTarget::Leftmost,
        Bound::Included(k) | Bound::Excluded(k) => DescentTarget::Key(k),
    };
    let (_, _, mut page, mut node) = descend(p, at, root, target, page_count)?;
    let mut idx = match from {
        Bound::Unbounded => 0,
        Bound::Included(k) => match node.search(&page, k) {
            Ok(i) | Err(i) => i,
        },
        Bound::Excluded(k) => match node.search(&page, k) {
            Ok(i) => i + 1,
            Err(i) => i,
        },
    };
    let mut visited_leaves = 1u64;
    let mut prev_last_key = if node.cell_count > 0 {
        Some(node.key(&page, node.cell_count - 1).to_vec())
    } else {
        None
    };

    loop {
        for i in idx..node.cell_count {
            let key = node.key(&page, i);
            let keep_going = on_cell(p, key, &page, &node.cells[i])?;
            if !keep_going {
                return Ok(());
            }
        }
        let next = node.link;
        if next == 0 {
            return Ok(());
        }
        visited_leaves += 1;
        if visited_leaves >= page_count {
            return Err(Error::Corrupt);
        }
        let next_page = p.read_view(at, next)?;
        let next_node = validate_node(&next_page, false, Some(KIND_LEAF), page_count, None, None)?;
        if let Some(prev) = &prev_last_key {
            if next_node.key(&next_page, 0) <= prev.as_slice() {
                return Err(Error::Corrupt);
            }
        }
        prev_last_key = Some(next_node.key(&next_page, next_node.cell_count - 1).to_vec());
        page = next_page;
        node = next_node;
        idx = 0;
    }
}

pub fn tree_scan<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    root: u64,
    from: Bound<&[u8]>,
    f: &mut dyn FnMut(&[u8], &[u8]) -> bool,
) -> Result<(), Error> {
    check_bound_len(from)?;
    let page_count = p.view_page_count(at)?;
    scan_cells(p, at, root, from, page_count, |p, key, page, cell| {
        let val = read_leaf_value(p, at, page, cell, page_count)?;
        Ok(f(key, &val))
    })
}

#[allow(clippy::type_complexity)]
pub fn tree_range<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    root: u64,
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error> {
    check_bound_len(lo)?;
    check_bound_len(hi)?;
    let page_count = p.view_page_count(at)?;
    let is_empty = match (lo, hi) {
        (Bound::Included(a), Bound::Included(b)) => a > b,
        (Bound::Included(a), Bound::Excluded(b))
        | (Bound::Excluded(a), Bound::Included(b))
        | (Bound::Excluded(a), Bound::Excluded(b)) => a >= b,
        _ => false,
    };
    if is_empty {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    scan_cells(p, at, root, lo, page_count, |p, key, page, cell| {
        let in_hi = match hi {
            Bound::Unbounded => true,
            Bound::Included(b) => key <= b,
            Bound::Excluded(b) => key < b,
        };
        if !in_hi {
            return Ok(false);
        }
        let val = read_leaf_value(p, at, page, cell, page_count)?;
        out.push((key.to_vec(), val));
        Ok(true)
    })?;
    Ok(out)
}

pub fn raw_cell_bytes(page: &Page, node: &ValidatedNode, idx: usize) -> Vec<u8> {
    let c = &node.cells[idx];
    page[c.off..c.end].to_vec()
}

pub fn extract_raw_cells(page: &Page, node: &ValidatedNode) -> Vec<Vec<u8>> {
    (0..node.cell_count)
        .map(|i| raw_cell_bytes(page, node, i))
        .collect()
}

pub fn validate_child_kind<S: Storage>(
    p: &mut Pager<S>,
    page_no: u64,
    expected_kind: u8,
    page_count: u64,
) -> Result<(Page, ValidatedNode), Error> {
    if page_no == 0 || page_no >= page_count {
        return Err(Error::Corrupt);
    }
    let page = p.read(page_no)?;
    let node = validate_node(&page, false, Some(expected_kind), page_count, None, None)?;
    Ok((page, node))
}
