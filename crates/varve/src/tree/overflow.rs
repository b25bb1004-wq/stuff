//! Overflow page chain reading, writing, and collection.

use crate::pager::{Pager, Snapshot};
use crate::tree::page::{read_u64, CellMeta};
use crate::{Error, Page, Storage, PAGE, USABLE};

pub const KIND_OVERFLOW: u8 = 4;
pub const OVERFLOW_HEADER: usize = 24;
pub const OVERFLOW_DATA_CAP: usize = USABLE - OVERFLOW_HEADER;

pub fn write_overflow_chain<S: Storage>(p: &mut Pager<S>, tail: &[u8]) -> Result<u64, Error> {
    let n = tail.len().div_ceil(OVERFLOW_DATA_CAP);
    let mut pages = Vec::with_capacity(n);
    for _ in 0..n {
        pages.push(p.alloc()?);
    }
    for (i, chunk) in tail.chunks(OVERFLOW_DATA_CAP).enumerate() {
        let mut page = [0u8; PAGE];
        page[0] = KIND_OVERFLOW;
        let next = if i + 1 < n { pages[i + 1] } else { 0 };
        page[16..24].copy_from_slice(&next.to_le_bytes());
        page[OVERFLOW_HEADER..OVERFLOW_HEADER + chunk.len()].copy_from_slice(chunk);
        p.write(pages[i], &page)?;
    }
    Ok(pages[0])
}

pub fn read_overflow_chain<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    head: u64,
    rem_len: usize,
    page_count: u64,
    out: &mut Vec<u8>,
) -> Result<(), Error> {
    let mut cur = head;
    let mut rem = rem_len;
    while rem > 0 {
        if cur == 0 || cur >= page_count {
            return Err(Error::Corrupt);
        }
        let page = p.read_view(at, cur)?;
        if page[0] != KIND_OVERFLOW {
            return Err(Error::Corrupt);
        }
        let take = rem.min(OVERFLOW_DATA_CAP);
        out.extend_from_slice(&page[OVERFLOW_HEADER..OVERFLOW_HEADER + take]);
        rem -= take;
        cur = read_u64(&page, 16)?;
    }
    if cur != 0 {
        return Err(Error::Corrupt);
    }
    Ok(())
}

pub fn collect_overflow_chain<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    head: u64,
    rem_len: usize,
    page_count: u64,
) -> Result<Vec<u64>, Error> {
    let mut pages = Vec::with_capacity(rem_len.div_ceil(OVERFLOW_DATA_CAP));
    let mut cur = head;
    let mut rem = rem_len;
    while rem > 0 {
        if cur == 0 || cur >= page_count {
            return Err(Error::Corrupt);
        }
        let page = p.read_view(at, cur)?;
        if page[0] != KIND_OVERFLOW {
            return Err(Error::Corrupt);
        }
        pages.push(cur);
        let take = rem.min(OVERFLOW_DATA_CAP);
        rem -= take;
        cur = read_u64(&page, 16)?;
    }
    if cur != 0 {
        return Err(Error::Corrupt);
    }
    Ok(pages)
}

pub fn read_leaf_value<S: Storage>(
    p: &mut Pager<S>,
    at: Option<Snapshot>,
    page: &Page,
    cell: &CellMeta,
    page_count: u64,
) -> Result<Vec<u8>, Error> {
    let inline_len = cell.inline_end - cell.inline_start;
    let mut out = Vec::with_capacity(cell.val_len);
    out.extend_from_slice(&page[cell.inline_start..cell.inline_end]);
    if let Some(head) = cell.overflow {
        let rem = cell.val_len.checked_sub(inline_len).ok_or(Error::Corrupt)?;
        if rem == 0 {
            return Err(Error::Corrupt);
        }
        read_overflow_chain(p, at, head, rem, page_count, &mut out)?;
    }
    Ok(out)
}
