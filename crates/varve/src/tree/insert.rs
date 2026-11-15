//! B+tree insertion, page splitting, and split propagation.

use crate::pager::Pager;
use crate::tree::overflow::{collect_overflow_chain, read_leaf_value, write_overflow_chain};
use crate::tree::page::{
    build_page, delete_cell_in_place, encode_interior_cell, encode_leaf_cell, insert_cell_in_place,
    read_u64, ValidatedNode, INLINE_LIMIT, KIND_INTERIOR, KIND_LEAF, MAX_KEY_LEN, MAX_PAYLOAD,
    MAX_VAL_LEN,
};
use crate::tree::read::{descend, extract_raw_cells, DescentTarget, PathStep};
use crate::{Error, Page, Storage};

pub fn tree_insert<S: Storage>(
    p: &mut Pager<S>,
    root: u64,
    key: &[u8],
    value: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    p.ensure_tx()?;
    if key.len() > MAX_KEY_LEN || value.len() > MAX_VAL_LEN {
        return Err(Error::TooLarge);
    }

    let page_count = p.page_count();
    let (mut path, leaf_no, mut leaf_page, mut leaf_node) =
        descend(p, None, root, DescentTarget::Key(key), page_count)?;

    let (pos, old_val, old_ov_pages) = match leaf_node.search(&leaf_page, key) {
        Ok(i) => {
            let cell = &leaf_node.cells[i];
            let old = read_leaf_value(p, None, &leaf_page, cell, page_count)?;
            let ovs = if let Some(head) = cell.overflow {
                let rem = cell.val_len - (cell.inline_end - cell.inline_start);
                collect_overflow_chain(p, None, head, rem, page_count)?
            } else {
                Vec::new()
            };
            (i, Some(old), Some(ovs))
        }
        Err(i) => (i, None, None),
    };

    if let Some(ovs) = old_ov_pages {
        for ov in ovs {
            p.free(ov)?;
        }
    }
    if old_val.is_some() {
        delete_cell_in_place(&mut leaf_page, &mut leaf_node, pos);
    }

    let (inline, ov_head) = if key.len() + value.len() <= INLINE_LIMIT {
        (value, None)
    } else {
        let il = INLINE_LIMIT - key.len();
        let head = write_overflow_chain(p, &value[il..])?;
        (&value[..il], Some(head))
    };
    let new_cell = encode_leaf_cell(key, value.len(), inline, ov_head);

    if insert_cell_in_place(&mut leaf_page, &mut leaf_node, pos, &new_cell) {
        p.write(leaf_no, &leaf_page)?;
        return Ok(old_val);
    }

    let mut keys = extract_keys(&leaf_page, &leaf_node);
    let mut raw = extract_raw_cells(&leaf_page, &leaf_node);
    keys.insert(pos, key.to_vec());
    raw.insert(pos, new_cell);

    let split_idx = choose_leaf_split(&raw)?;
    let sep_key = keys[split_idx].clone();
    let right_cells = raw.split_off(split_idx);
    let left_cells = raw;

    if path.is_empty() {
        let p_left = p.alloc()?;
        let p_right = p.alloc()?;
        let left_page = build_page(KIND_LEAF, p_right, &left_cells)?;
        let right_page = build_page(KIND_LEAF, 0, &right_cells)?;
        p.write(p_left, &left_page)?;
        p.write(p_right, &right_page)?;
        let root_cell = encode_interior_cell(p_left, &sep_key);
        let new_root = build_page(KIND_INTERIOR, p_right, &[root_cell])?;
        p.write(root, &new_root)?;
    } else {
        let p_right = p.alloc()?;
        let left_page = build_page(KIND_LEAF, p_right, &left_cells)?;
        let right_page = build_page(KIND_LEAF, leaf_node.link, &right_cells)?;
        p.write(leaf_no, &left_page)?;
        p.write(p_right, &right_page)?;
        propagate_split(p, root, &mut path, leaf_no, sep_key, p_right)?;
    }

    Ok(old_val)
}

pub fn propagate_split<S: Storage>(
    p: &mut Pager<S>,
    root: u64,
    path: &mut Vec<PathStep>,
    mut left_no: u64,
    mut sep_key: Vec<u8>,
    mut right_no: u64,
) -> Result<(), Error> {
    while let Some(mut step) = path.pop() {
        if step.slot < step.node.cell_count {
            let off = step.node.cells[step.slot].off;
            step.page[off..off + 8].copy_from_slice(&right_no.to_le_bytes());
            step.node.cells[step.slot].child = right_no;
        } else {
            step.node.link = right_no;
            step.page[8..16].copy_from_slice(&right_no.to_le_bytes());
        }

        let new_cell = encode_interior_cell(left_no, &sep_key);
        if insert_cell_in_place(&mut step.page, &mut step.node, step.slot, &new_cell) {
            p.write(step.page_no, &step.page)?;
            return Ok(());
        }

        let mut keys = extract_keys(&step.page, &step.node);
        let mut raw = extract_raw_cells(&step.page, &step.node);
        keys.insert(step.slot, sep_key);
        raw.insert(step.slot, new_cell);

        let mid = choose_interior_split(&raw)?;
        let promoted_key = keys[mid].clone();
        let left_link = read_u64(&raw[mid], 0)?;
        let right_cells = raw.split_off(mid + 1);
        raw.pop();
        let left_cells = raw;
        let right_link = step.node.link;

        if path.is_empty() {
            let p_left = p.alloc()?;
            let p_right = p.alloc()?;
            let left_page = build_page(KIND_INTERIOR, left_link, &left_cells)?;
            let right_page = build_page(KIND_INTERIOR, right_link, &right_cells)?;
            p.write(p_left, &left_page)?;
            p.write(p_right, &right_page)?;
            let root_cell = encode_interior_cell(p_left, &promoted_key);
            let new_root = build_page(KIND_INTERIOR, p_right, &[root_cell])?;
            p.write(root, &new_root)?;
            return Ok(());
        }

        let p_right = p.alloc()?;
        let left_page = build_page(KIND_INTERIOR, left_link, &left_cells)?;
        let right_page = build_page(KIND_INTERIOR, right_link, &right_cells)?;
        p.write(step.page_no, &left_page)?;
        p.write(p_right, &right_page)?;
        left_no = step.page_no;
        sep_key = promoted_key;
        right_no = p_right;
    }
    Ok(())
}

pub fn extract_keys(page: &Page, node: &ValidatedNode) -> Vec<Vec<u8>> {
    (0..node.cell_count)
        .map(|i| node.key(page, i).to_vec())
        .collect()
}

fn choose_leaf_split(raw: &[Vec<u8>]) -> Result<usize, Error> {
    let total: usize = raw.iter().map(|c| c.len() + 2).sum();
    let mut best_idx = None;
    let mut best_max = usize::MAX;
    let mut left_sum = 0usize;
    for i in 1..raw.len() {
        left_sum += raw[i - 1].len() + 2;
        let right_sum = total - left_sum;
        if left_sum <= MAX_PAYLOAD && right_sum <= MAX_PAYLOAD {
            let m = left_sum.max(right_sum);
            if m < best_max {
                best_max = m;
                best_idx = Some(i);
            }
        }
    }
    best_idx.ok_or(Error::Corrupt)
}

pub fn choose_interior_split(raw: &[Vec<u8>]) -> Result<usize, Error> {
    if raw.len() < 3 {
        return Err(Error::Corrupt);
    }
    let total: usize = raw.iter().map(|c| c.len() + 2).sum();
    let mut best_idx = None;
    let mut best_max = usize::MAX;
    let mut left_sum = 0usize;
    for i in 1..raw.len() - 1 {
        left_sum += raw[i - 1].len() + 2;
        let mid_sz = raw[i].len() + 2;
        let right_sum = total - left_sum - mid_sz;
        if left_sum <= MAX_PAYLOAD && right_sum <= MAX_PAYLOAD {
            let m = left_sum.max(right_sum);
            if m < best_max {
                best_max = m;
                best_idx = Some(i);
            }
        }
    }
    best_idx.ok_or(Error::Corrupt)
}
