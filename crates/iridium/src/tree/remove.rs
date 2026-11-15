//! B+tree removal, sibling merging, interior borrowing, root collapse, and destruction.

use std::collections::BTreeSet;

use crate::pager::Pager;
use crate::tree::insert::{choose_interior_split, extract_keys, propagate_split};
use crate::tree::overflow::{collect_overflow_chain, read_leaf_value};
use crate::tree::page::{
    build_page, delete_cell_in_place, encode_interior_cell, insert_cell_in_place, read_u64,
    validate_node, write_header, ValidatedNode, KIND_INTERIOR, KIND_LEAF, MAX_KEY_LEN, MAX_PAYLOAD,
};
use crate::tree::read::{
    descend, extract_raw_cells, validate_child_kind, DescentTarget, PathStep, MAX_DEPTH,
};
use crate::{Error, Page, Storage};

pub fn tree_remove<S: Storage>(
    p: &mut Pager<S>,
    root: u64,
    key: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    p.ensure_tx()?;
    if key.len() > MAX_KEY_LEN {
        return Err(Error::TooLarge);
    }

    let page_count = p.page_count();
    let (mut path, leaf_no, mut leaf_page, mut leaf_node) =
        descend(p, None, root, DescentTarget::Key(key), page_count)?;

    let pos = match leaf_node.search(&leaf_page, key) {
        Ok(i) => i,
        Err(_) => return Ok(None),
    };

    let cell = &leaf_node.cells[pos];
    let old_val = read_leaf_value(p, None, &leaf_page, cell, page_count)?;
    let old_ovs = if let Some(head) = cell.overflow {
        let rem = cell.val_len - (cell.inline_end - cell.inline_start);
        collect_overflow_chain(p, None, head, rem, page_count)?
    } else {
        Vec::new()
    };

    for ov in old_ovs {
        p.free(ov)?;
    }
    delete_cell_in_place(&mut leaf_page, &mut leaf_node, pos);

    let Some(mut parent) = path.pop() else {
        p.write(leaf_no, &leaf_page)?;
        return Ok(Some(old_val));
    };

    if try_merge_leaf(p, &mut parent, leaf_no, &leaf_page, &leaf_node, page_count)? {
        rebalance_interior(p, root, &mut path, parent, page_count)?;
    } else {
        p.write(leaf_no, &leaf_page)?;
    }

    Ok(Some(old_val))
}

fn try_merge_leaf<S: Storage>(
    p: &mut Pager<S>,
    parent: &mut PathStep,
    leaf_no: u64,
    leaf_page: &Page,
    leaf_node: &ValidatedNode,
    page_count: u64,
) -> Result<bool, Error> {
    if parent.slot > 0 {
        let left_no = parent.node.child(parent.slot - 1);
        let (left_page, left_node) = validate_child_kind(p, left_no, KIND_LEAF, page_count)?;
        if left_node.used_bytes() + leaf_node.used_bytes() <= MAX_PAYLOAD {
            let mut merged = extract_raw_cells(&left_page, &left_node);
            merged.extend(extract_raw_cells(leaf_page, leaf_node));
            let merged_page = build_page(KIND_LEAF, leaf_node.link, &merged)?;
            p.write(left_no, &merged_page)?;
            p.free(leaf_no)?;
            drop_right_child(&mut parent.page, &mut parent.node, parent.slot - 1);
            return Ok(true);
        }
    }

    if parent.slot < parent.node.cell_count {
        let right_no = parent.node.child(parent.slot + 1);
        let (right_page, right_node) = validate_child_kind(p, right_no, KIND_LEAF, page_count)?;
        if leaf_node.used_bytes() + right_node.used_bytes() <= MAX_PAYLOAD {
            let mut merged = extract_raw_cells(leaf_page, leaf_node);
            merged.extend(extract_raw_cells(&right_page, &right_node));
            let merged_page = build_page(KIND_LEAF, right_node.link, &merged)?;
            p.write(leaf_no, &merged_page)?;
            p.free(right_no)?;
            drop_right_child(&mut parent.page, &mut parent.node, parent.slot);
            return Ok(true);
        }
    }

    Ok(false)
}

fn rebalance_interior<S: Storage>(
    p: &mut Pager<S>,
    root: u64,
    path: &mut Vec<PathStep>,
    mut cur: PathStep,
    page_count: u64,
) -> Result<(), Error> {
    loop {
        let Some(mut parent) = path.pop() else {
            if cur.node.cell_count == 0 {
                let only_child = cur.node.link;
                if only_child == 0 || only_child >= page_count {
                    return Err(Error::Corrupt);
                }
                let child_page = p.read(only_child)?;
                let _ = validate_node(&child_page, true, None, page_count, None, None)?;
                p.write(root, &child_page)?;
                p.free(only_child)?;
            } else {
                p.write(root, &cur.page)?;
            }
            return Ok(());
        };

        if parent.slot > 0 {
            let left_no = parent.node.child(parent.slot - 1);
            let (left_page, left_node) =
                validate_child_kind(p, left_no, KIND_INTERIOR, page_count)?;
            let sep_key = parent.node.key(&parent.page, parent.slot - 1);
            let sep_cell = encode_interior_cell(left_node.link, sep_key);
            if left_node.used_bytes() + sep_cell.len() + 2 + cur.node.used_bytes() <= MAX_PAYLOAD {
                let mut merged = extract_raw_cells(&left_page, &left_node);
                merged.push(sep_cell);
                merged.extend(extract_raw_cells(&cur.page, &cur.node));
                let merged_page = build_page(KIND_INTERIOR, cur.node.link, &merged)?;
                p.write(left_no, &merged_page)?;
                p.free(cur.page_no)?;
                drop_right_child(&mut parent.page, &mut parent.node, parent.slot - 1);
                cur = parent;
                continue;
            }
        }

        if parent.slot < parent.node.cell_count {
            let right_no = parent.node.child(parent.slot + 1);
            let (right_page, right_node) =
                validate_child_kind(p, right_no, KIND_INTERIOR, page_count)?;
            let sep_key = parent.node.key(&parent.page, parent.slot);
            let sep_cell = encode_interior_cell(cur.node.link, sep_key);
            if cur.node.used_bytes() + sep_cell.len() + 2 + right_node.used_bytes() <= MAX_PAYLOAD {
                let mut merged = extract_raw_cells(&cur.page, &cur.node);
                merged.push(sep_cell);
                merged.extend(extract_raw_cells(&right_page, &right_node));
                let merged_page = build_page(KIND_INTERIOR, right_node.link, &merged)?;
                p.write(cur.page_no, &merged_page)?;
                p.free(right_no)?;
                drop_right_child(&mut parent.page, &mut parent.node, parent.slot);
                cur = parent;
                continue;
            }
        }

        if cur.node.cell_count >= 1 {
            p.write(cur.page_no, &cur.page)?;
            return Ok(());
        }

        return borrow_interior(p, root, path, parent, cur, page_count);
    }
}

fn borrow_interior<S: Storage>(
    p: &mut Pager<S>,
    root: u64,
    path: &mut Vec<PathStep>,
    mut parent: PathStep,
    cur: PathStep,
    page_count: u64,
) -> Result<(), Error> {
    let (sep_idx, new_sep) = if parent.slot > 0 {
        let left_no = parent.node.child(parent.slot - 1);
        let (mut left_page, mut left_node) =
            validate_child_kind(p, left_no, KIND_INTERIOR, page_count)?;
        if left_node.cell_count < 2 {
            return Err(Error::Corrupt);
        }
        let last_idx = left_node.cell_count - 1;
        let new_sep = left_node.key(&left_page, last_idx).to_vec();
        let new_left_link = left_node.cells[last_idx].child;
        let sep = parent.node.key(&parent.page, parent.slot - 1);
        let cur_cell = encode_interior_cell(left_node.link, sep);
        let new_cur = build_page(KIND_INTERIOR, cur.node.link, &[cur_cell])?;
        delete_cell_in_place(&mut left_page, &mut left_node, last_idx);
        left_node.link = new_left_link;
        write_header(
            &mut left_page,
            KIND_INTERIOR,
            left_node.cell_count as u16,
            left_node.content_start as u16,
            left_node.fragmented as u16,
            new_left_link,
        );
        p.write(left_no, &left_page)?;
        p.write(cur.page_no, &new_cur)?;
        (parent.slot - 1, new_sep)
    } else {
        let right_no = parent.node.child(1);
        let (mut right_page, mut right_node) =
            validate_child_kind(p, right_no, KIND_INTERIOR, page_count)?;
        if right_node.cell_count < 2 {
            return Err(Error::Corrupt);
        }
        let new_sep = right_node.key(&right_page, 0).to_vec();
        let new_cur_link = right_node.cells[0].child;
        let sep = parent.node.key(&parent.page, 0);
        let cur_cell = encode_interior_cell(cur.node.link, sep);
        let new_cur = build_page(KIND_INTERIOR, new_cur_link, &[cur_cell])?;
        delete_cell_in_place(&mut right_page, &mut right_node, 0);
        p.write(right_no, &right_page)?;
        p.write(cur.page_no, &new_cur)?;
        (0, new_sep)
    };

    let child_kept = parent.node.child(sep_idx);
    let updated_cell = encode_interior_cell(child_kept, &new_sep);
    delete_cell_in_place(&mut parent.page, &mut parent.node, sep_idx);
    if insert_cell_in_place(&mut parent.page, &mut parent.node, sep_idx, &updated_cell) {
        p.write(parent.page_no, &parent.page)?;
        return Ok(());
    }

    let mut keys = extract_keys(&parent.page, &parent.node);
    let mut raw = extract_raw_cells(&parent.page, &parent.node);
    keys.insert(sep_idx, new_sep);
    raw.insert(sep_idx, updated_cell);

    let mid = choose_interior_split(&raw)?;
    let promoted = keys[mid].clone();
    let left_link = read_u64(&raw[mid], 0)?;
    let right_cells = raw.split_off(mid + 1);
    raw.pop();
    let left_cells = raw;
    let right_link = parent.node.link;

    if path.is_empty() {
        let p_left = p.alloc()?;
        let p_right = p.alloc()?;
        let left_page = build_page(KIND_INTERIOR, left_link, &left_cells)?;
        let right_page = build_page(KIND_INTERIOR, right_link, &right_cells)?;
        p.write(p_left, &left_page)?;
        p.write(p_right, &right_page)?;
        let root_cell = encode_interior_cell(p_left, &promoted);
        let new_root = build_page(KIND_INTERIOR, p_right, &[root_cell])?;
        p.write(root, &new_root)?;
        return Ok(());
    }

    let p_right = p.alloc()?;
    let left_page = build_page(KIND_INTERIOR, left_link, &left_cells)?;
    let right_page = build_page(KIND_INTERIOR, right_link, &right_cells)?;
    p.write(parent.page_no, &left_page)?;
    p.write(p_right, &right_page)?;
    propagate_split(p, root, path, parent.page_no, promoted, p_right)
}

fn drop_right_child(page: &mut Page, node: &mut ValidatedNode, keep_slot: usize) {
    let keep_no = node.child(keep_slot);
    if keep_slot + 1 < node.cell_count {
        let off = node.cells[keep_slot + 1].off;
        page[off..off + 8].copy_from_slice(&keep_no.to_le_bytes());
        node.cells[keep_slot + 1].child = keep_no;
    } else {
        node.link = keep_no;
        page[8..16].copy_from_slice(&keep_no.to_le_bytes());
    }
    delete_cell_in_place(page, node, keep_slot);
}

pub fn tree_destroy<S: Storage>(p: &mut Pager<S>, root: u64) -> Result<(), Error> {
    p.ensure_tx()?;
    let page_count = p.page_count();
    let mut stack = vec![(root, 1usize)];
    let mut seen = BTreeSet::new();
    let mut to_free = Vec::new();
    let mut leaf_depth: Option<usize> = None;
    let mut expected_next_leaf: Option<u64> = None;

    while let Some((page_no, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(Error::Corrupt);
        }
        if !seen.insert(page_no) {
            return Err(Error::Corrupt);
        }
        let page = p.read(page_no)?;
        let node = validate_node(&page, depth == 1, None, page_count, None, None)?;
        to_free.push(page_no);

        if node.kind == KIND_LEAF {
            match leaf_depth {
                Some(d) if d != depth => return Err(Error::Corrupt),
                None => leaf_depth = Some(depth),
                _ => {}
            }
            if let Some(exp) = expected_next_leaf {
                if exp != page_no {
                    return Err(Error::Corrupt);
                }
            }
            expected_next_leaf = Some(node.link);

            for c in &node.cells {
                if let Some(head) = c.overflow {
                    let rem = c.val_len - (c.inline_end - c.inline_start);
                    let ovs = collect_overflow_chain(p, None, head, rem, page_count)?;
                    for ov in ovs {
                        if !seen.insert(ov) {
                            return Err(Error::Corrupt);
                        }
                        to_free.push(ov);
                    }
                }
            }
        } else {
            if depth == MAX_DEPTH {
                return Err(Error::Corrupt);
            }
            stack.push((node.link, depth + 1));
            for c in node.cells.iter().rev() {
                stack.push((c.child, depth + 1));
            }
        }
    }

    if expected_next_leaf != Some(0) {
        return Err(Error::Corrupt);
    }

    for pg in to_free {
        p.free(pg)?;
    }
    Ok(())
}
