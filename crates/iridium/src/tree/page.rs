//! Slotted B+tree page encoding, decoding, and validation.

pub use super::slotted::{build_page, delete_cell_in_place, insert_cell_in_place};
use crate::{Error, Page, PAGE, USABLE};

pub const KIND_INTERIOR: u8 = 2;
pub const KIND_LEAF: u8 = 3;
pub const HEADER_SIZE: usize = 16;
pub const MAX_PAYLOAD: usize = USABLE - HEADER_SIZE;
pub const INLINE_LIMIT: usize = 1200;
pub const MAX_KEY_LEN: usize = 1024;
pub const MAX_VAL_LEN: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct CellMeta {
    pub off: usize,
    pub end: usize,
    pub key_start: usize,
    pub key_end: usize,
    pub child: u64,
    pub val_len: usize,
    pub inline_start: usize,
    pub inline_end: usize,
    pub overflow: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ValidatedNode {
    pub kind: u8,
    pub cell_count: usize,
    pub content_start: usize,
    pub fragmented: usize,
    pub link: u64,
    pub total_cell_bytes: usize,
    pub cells: Vec<CellMeta>,
}

impl ValidatedNode {
    pub fn key<'a>(&self, page: &'a Page, idx: usize) -> &'a [u8] {
        let c = &self.cells[idx];
        &page[c.key_start..c.key_end]
    }

    pub fn used_bytes(&self) -> usize {
        self.total_cell_bytes + 2 * self.cell_count
    }

    pub fn child(&self, idx: usize) -> u64 {
        if idx < self.cell_count {
            self.cells[idx].child
        } else {
            self.link
        }
    }

    pub fn search(&self, page: &Page, key: &[u8]) -> Result<usize, usize> {
        let mut lo = 0usize;
        let mut hi = self.cell_count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.key(page, mid).cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Equal => return Ok(mid),
                std::cmp::Ordering::Greater => hi = mid,
            }
        }
        Err(lo)
    }

    pub fn child_slot(&self, page: &Page, key: &[u8]) -> usize {
        match self.search(page, key) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

pub fn varint_len(mut v: u64) -> usize {
    let mut n = 1usize;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

pub fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

pub fn decode_varint(slice: &[u8]) -> Result<(u64, usize), Error> {
    let mut val = 0u64;
    for i in 0..10 {
        let &b = slice.get(i).ok_or(Error::Corrupt)?;
        if i == 9 {
            if b != 1 {
                return Err(Error::Corrupt);
            }
            val |= 1u64 << 63;
            return Ok((val, 10));
        }
        if b & 0x80 == 0 {
            if i > 0 && b == 0 {
                return Err(Error::Corrupt);
            }
            val |= u64::from(b) << (i * 7);
            return Ok((val, i + 1));
        }
        val |= u64::from(b & 0x7f) << (i * 7);
    }
    Err(Error::Corrupt)
}

pub fn read_u16(slice: &[u8], off: usize) -> Result<u16, Error> {
    let bytes: [u8; 2] = slice
        .get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::Corrupt)?;
    Ok(u16::from_le_bytes(bytes))
}

pub fn read_u64(slice: &[u8], off: usize) -> Result<u64, Error> {
    let bytes: [u8; 8] = slice
        .get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::Corrupt)?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn write_header(
    page: &mut Page,
    kind: u8,
    count: u16,
    content_start: u16,
    frag: u16,
    link: u64,
) {
    page[0] = kind;
    page[1] = 0;
    page[2..4].copy_from_slice(&count.to_le_bytes());
    page[4..6].copy_from_slice(&content_start.to_le_bytes());
    page[6..8].copy_from_slice(&frag.to_le_bytes());
    page[8..16].copy_from_slice(&link.to_le_bytes());
}

pub fn empty_leaf_page() -> Page {
    let mut page = [0u8; PAGE];
    write_header(&mut page, KIND_LEAF, 0, USABLE as u16, 0, 0);
    page
}

pub fn encode_interior_cell(child: u64, key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + varint_len(key.len() as u64) + key.len());
    out.extend_from_slice(&child.to_le_bytes());
    encode_varint(key.len() as u64, &mut out);
    out.extend_from_slice(key);
    out
}

pub fn encode_leaf_cell(
    key: &[u8],
    val_len: usize,
    inline: &[u8],
    overflow: Option<u64>,
) -> Vec<u8> {
    let extra = if overflow.is_some() { 8 } else { 0 };
    let cap = varint_len(key.len() as u64)
        + varint_len(val_len as u64)
        + key.len()
        + inline.len()
        + extra;
    let mut out = Vec::with_capacity(cap);
    encode_varint(key.len() as u64, &mut out);
    encode_varint(val_len as u64, &mut out);
    out.extend_from_slice(key);
    out.extend_from_slice(inline);
    if let Some(ov) = overflow {
        out.extend_from_slice(&ov.to_le_bytes());
    }
    out
}

pub fn validate_node(
    page: &Page,
    is_root: bool,
    expected_kind: Option<u8>,
    page_count: u64,
    lo_bound: Option<&[u8]>,
    hi_bound: Option<&[u8]>,
) -> Result<ValidatedNode, Error> {
    let kind = page[0];
    if kind != KIND_INTERIOR && kind != KIND_LEAF {
        return Err(Error::Corrupt);
    }
    if let Some(exp) = expected_kind {
        if kind != exp {
            return Err(Error::Corrupt);
        }
    }
    if page[1] != 0 {
        return Err(Error::Corrupt);
    }

    let cell_count = usize::from(read_u16(page, 2)?);
    let content_start = usize::from(read_u16(page, 4)?);
    let fragmented = usize::from(read_u16(page, 6)?);
    let link = read_u64(page, 8)?;

    if kind == KIND_INTERIOR {
        if cell_count == 0 {
            return Err(Error::Corrupt);
        }
        if link == 0 || link >= page_count {
            return Err(Error::Corrupt);
        }
    } else {
        if !is_root && cell_count == 0 {
            return Err(Error::Corrupt);
        }
        if link != 0 && link >= page_count {
            return Err(Error::Corrupt);
        }
    }

    let offsets_end = HEADER_SIZE + 2 * cell_count;
    if content_start < offsets_end || content_start > USABLE {
        return Err(Error::Corrupt);
    }
    if content_start + fragmented > USABLE {
        return Err(Error::Corrupt);
    }

    let mut cells: Vec<CellMeta> = Vec::with_capacity(cell_count);
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(cell_count);
    let mut total_cell_bytes = 0usize;

    for i in 0..cell_count {
        let off = usize::from(read_u16(page, HEADER_SIZE + 2 * i)?);
        if off < content_start || off >= USABLE {
            return Err(Error::Corrupt);
        }
        let meta = if kind == KIND_INTERIOR {
            parse_interior_cell(page, off, page_count)?
        } else {
            parse_leaf_cell(page, off, page_count)?
        };
        let len = meta.end - meta.off;
        total_cell_bytes += len;
        spans.push((meta.off, meta.end));
        if i > 0 {
            let prev = &cells[i - 1];
            if page[prev.key_start..prev.key_end] >= page[meta.key_start..meta.key_end] {
                return Err(Error::Corrupt);
            }
        }
        cells.push(meta);
    }

    if total_cell_bytes + 2 * cell_count > MAX_PAYLOAD {
        return Err(Error::Corrupt);
    }
    if total_cell_bytes + fragmented > USABLE - content_start {
        return Err(Error::Corrupt);
    }

    spans.sort_unstable_by_key(|&(s, _)| s);
    for w in spans.windows(2) {
        if w[0].1 > w[1].0 {
            return Err(Error::Corrupt);
        }
    }

    if cell_count > 0 {
        let first_key = &page[cells[0].key_start..cells[0].key_end];
        let last_key = &page[cells[cell_count - 1].key_start..cells[cell_count - 1].key_end];
        if kind == KIND_INTERIOR {
            if let Some(lo) = lo_bound {
                if first_key <= lo {
                    return Err(Error::Corrupt);
                }
            }
            if let Some(hi) = hi_bound {
                if last_key >= hi {
                    return Err(Error::Corrupt);
                }
            }
        } else {
            if let Some(lo) = lo_bound {
                if first_key < lo {
                    return Err(Error::Corrupt);
                }
            }
            if let Some(hi) = hi_bound {
                if last_key >= hi {
                    return Err(Error::Corrupt);
                }
            }
        }
    }

    Ok(ValidatedNode {
        kind,
        cell_count,
        content_start,
        fragmented,
        link,
        total_cell_bytes,
        cells,
    })
}

fn parse_interior_cell(page: &Page, off: usize, page_count: u64) -> Result<CellMeta, Error> {
    if off + 8 > USABLE {
        return Err(Error::Corrupt);
    }
    let child = read_u64(page, off)?;
    if child == 0 || child >= page_count {
        return Err(Error::Corrupt);
    }
    let (klen_u64, klen_bytes) = decode_varint(&page[off + 8..USABLE])?;
    let klen = usize::try_from(klen_u64).map_err(|_| Error::Corrupt)?;
    if klen > MAX_KEY_LEN {
        return Err(Error::Corrupt);
    }
    let key_start = off + 8 + klen_bytes;
    let key_end = key_start.checked_add(klen).ok_or(Error::Corrupt)?;
    if key_end > USABLE {
        return Err(Error::Corrupt);
    }
    Ok(CellMeta {
        off,
        end: key_end,
        key_start,
        key_end,
        child,
        val_len: 0,
        inline_start: 0,
        inline_end: 0,
        overflow: None,
    })
}

fn parse_leaf_cell(page: &Page, off: usize, page_count: u64) -> Result<CellMeta, Error> {
    let (klen_u64, kl_n) = decode_varint(&page[off..USABLE])?;
    let klen = usize::try_from(klen_u64).map_err(|_| Error::Corrupt)?;
    if klen > MAX_KEY_LEN {
        return Err(Error::Corrupt);
    }
    let (vlen_u64, vl_n) = decode_varint(&page[off + kl_n..USABLE])?;
    let vlen = usize::try_from(vlen_u64).map_err(|_| Error::Corrupt)?;
    if vlen > MAX_VAL_LEN {
        return Err(Error::Corrupt);
    }
    let key_start = off + kl_n + vl_n;
    let key_end = key_start.checked_add(klen).ok_or(Error::Corrupt)?;
    if key_end > USABLE {
        return Err(Error::Corrupt);
    }
    let (inline_len, has_ov) = if klen + vlen <= INLINE_LIMIT {
        (vlen, false)
    } else {
        (INLINE_LIMIT - klen, true)
    };
    let inline_start = key_end;
    let inline_end = inline_start.checked_add(inline_len).ok_or(Error::Corrupt)?;
    let (end, overflow) = if has_ov {
        let ov_end = inline_end.checked_add(8).ok_or(Error::Corrupt)?;
        if ov_end > USABLE {
            return Err(Error::Corrupt);
        }
        let ov = read_u64(page, inline_end)?;
        if ov == 0 || ov >= page_count {
            return Err(Error::Corrupt);
        }
        (ov_end, Some(ov))
    } else {
        if inline_end > USABLE {
            return Err(Error::Corrupt);
        }
        (inline_end, None)
    };
    Ok(CellMeta {
        off,
        end,
        key_start,
        key_end,
        child: 0,
        val_len: vlen,
        inline_start,
        inline_end,
        overflow,
    })
}
