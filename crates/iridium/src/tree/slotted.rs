//! In-place slotted-page mutation, defragmentation, and page building.

use crate::tree::page::{write_header, ValidatedNode, HEADER_SIZE, KIND_LEAF, MAX_PAYLOAD};
use crate::{Error, Page, PAGE, USABLE};

pub fn defragment_page(page: &mut Page, node: &mut ValidatedNode) {
    let old = *page;
    let mut new_page = [0u8; PAGE];
    let mut cursor = USABLE;
    for (i, c) in node.cells.iter_mut().enumerate() {
        let len = c.end - c.off;
        let new_off = cursor - len;
        new_page[new_off..cursor].copy_from_slice(&old[c.off..c.end]);
        new_page[HEADER_SIZE + 2 * i..HEADER_SIZE + 2 * i + 2]
            .copy_from_slice(&(new_off as u16).to_le_bytes());
        let shift = new_off as isize - c.off as isize;
        c.off = new_off;
        c.end = cursor;
        c.key_start = (c.key_start as isize + shift) as usize;
        c.key_end = (c.key_end as isize + shift) as usize;
        if node.kind == KIND_LEAF {
            c.inline_start = (c.inline_start as isize + shift) as usize;
            c.inline_end = (c.inline_end as isize + shift) as usize;
        }
        cursor = new_off;
    }
    node.content_start = cursor;
    node.fragmented = 0;
    write_header(
        &mut new_page,
        node.kind,
        node.cell_count as u16,
        cursor as u16,
        0,
        node.link,
    );
    *page = new_page;
}

pub fn insert_cell_in_place(
    page: &mut Page,
    node: &mut ValidatedNode,
    idx: usize,
    cell: &[u8],
) -> bool {
    let needed = cell.len() + 2;
    let gap = node.content_start - (HEADER_SIZE + 2 * node.cell_count);
    if gap < needed {
        if gap + node.fragmented < needed {
            return false;
        }
        defragment_page(page, node);
    }
    let new_cs = node.content_start - cell.len();
    page[new_cs..node.content_start].copy_from_slice(cell);
    let old_count = node.cell_count;
    for i in (idx..old_count).rev() {
        let slot_off = HEADER_SIZE + 2 * i;
        let b0 = page[slot_off];
        let b1 = page[slot_off + 1];
        page[slot_off + 2] = b0;
        page[slot_off + 3] = b1;
    }
    page[HEADER_SIZE + 2 * idx..HEADER_SIZE + 2 * idx + 2]
        .copy_from_slice(&(new_cs as u16).to_le_bytes());
    node.cell_count += 1;
    node.content_start = new_cs;
    node.total_cell_bytes += cell.len();
    write_header(
        page,
        node.kind,
        node.cell_count as u16,
        node.content_start as u16,
        node.fragmented as u16,
        node.link,
    );
    true
}

pub fn delete_cell_in_place(page: &mut Page, node: &mut ValidatedNode, idx: usize) {
    let c = node.cells.remove(idx);
    let cell_len = c.end - c.off;
    page[c.off..c.end].fill(0);
    let new_count = node.cell_count - 1;
    for i in idx..new_count {
        let src = HEADER_SIZE + 2 * (i + 1);
        let dst = HEADER_SIZE + 2 * i;
        page[dst] = page[src];
        page[dst + 1] = page[src + 1];
    }
    page[HEADER_SIZE + 2 * new_count..HEADER_SIZE + 2 * (new_count + 1)].fill(0);
    node.cell_count = new_count;
    node.total_cell_bytes -= cell_len;
    if new_count == 0 {
        node.content_start = USABLE;
        node.fragmented = 0;
    } else if c.off == node.content_start {
        node.content_start += cell_len;
    } else {
        node.fragmented += cell_len;
    }
    write_header(
        page,
        node.kind,
        node.cell_count as u16,
        node.content_start as u16,
        node.fragmented as u16,
        node.link,
    );
}

pub fn build_page(kind: u8, link: u64, raw_cells: &[Vec<u8>]) -> Result<Page, Error> {
    let total: usize = raw_cells.iter().map(|c| c.len() + 2).sum();
    if total > MAX_PAYLOAD {
        return Err(Error::Corrupt);
    }
    let mut page = [0u8; PAGE];
    let mut cursor = USABLE;
    for (i, cell) in raw_cells.iter().enumerate() {
        let new_off = cursor - cell.len();
        page[new_off..cursor].copy_from_slice(cell);
        page[HEADER_SIZE + 2 * i..HEADER_SIZE + 2 * i + 2]
            .copy_from_slice(&(new_off as u16).to_le_bytes());
        cursor = new_off;
    }
    write_header(
        &mut page,
        kind,
        raw_cells.len() as u16,
        cursor as u16,
        0,
        link,
    );
    Ok(page)
}
