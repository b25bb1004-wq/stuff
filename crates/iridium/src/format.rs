//! Binary encoding and decoding for Iridium pages and WAL headers/frames.

use crate::crc::{crc32, Crc32};
use crate::{Error, Page, PAGE, USABLE};

pub const PAGE0_MAGIC: &[u8; 8] = b"IRIDI\0v1";
pub const WAL_MAGIC: &[u8; 8] = b"IRIDIWAL";
pub const WAL_VERSION: u32 = 1;
pub const WAL_HEADER_SIZE: usize = 32;
pub const WAL_FRAME_SIZE: usize = WAL_HEADER_SIZE + PAGE;
pub const FREE_PAGE_KIND: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page0Meta {
    pub page_count: u64,
    pub free_head: u64,
    pub root: u64,
    pub change_counter: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMeta {
    pub page_no: u64,
    pub commit: u64,
    pub salt: u64,
}

fn read_u32_le(slice: &[u8], off: usize) -> Option<u32> {
    let bytes: [u8; 4] = slice.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_u64_le(slice: &[u8], off: usize) -> Option<u64> {
    let bytes: [u8; 8] = slice.get(off..off + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

pub fn stamp_page_crc(page: &mut Page) {
    let c = crc32(&page[..USABLE]);
    page[USABLE..PAGE].copy_from_slice(&c.to_le_bytes());
}

pub fn verify_page_crc(page: &Page) -> Result<(), Error> {
    let expected = crc32(&page[..USABLE]);
    let actual = read_u32_le(page, USABLE).ok_or(Error::Corrupt)?;
    if expected != actual {
        return Err(Error::Corrupt);
    }
    Ok(())
}

pub fn encode_zero_page() -> Page {
    let mut page = [0u8; PAGE];
    stamp_page_crc(&mut page);
    page
}

pub fn encode_free_page(next_free: u64) -> Page {
    let mut page = [0u8; PAGE];
    page[0] = FREE_PAGE_KIND;
    page[16..24].copy_from_slice(&next_free.to_le_bytes());
    stamp_page_crc(&mut page);
    page
}

pub fn decode_free_page(page: &Page) -> Result<u64, Error> {
    verify_page_crc(page)?;
    if page[0] != FREE_PAGE_KIND {
        return Err(Error::Corrupt);
    }
    read_u64_le(page, 16).ok_or(Error::Corrupt)
}

pub fn encode_page0(meta: Page0Meta) -> Page {
    let mut page = [0u8; PAGE];
    page[0..8].copy_from_slice(PAGE0_MAGIC);
    page[8..12].copy_from_slice(&(PAGE as u32).to_le_bytes());
    page[12..20].copy_from_slice(&meta.page_count.to_le_bytes());
    page[20..28].copy_from_slice(&meta.free_head.to_le_bytes());
    page[28..36].copy_from_slice(&meta.root.to_le_bytes());
    page[36..44].copy_from_slice(&meta.change_counter.to_le_bytes());
    stamp_page_crc(&mut page);
    page
}

pub fn decode_page0(page: &Page) -> Result<Page0Meta, Error> {
    verify_page_crc(page)?;
    if page.get(0..8) != Some(PAGE0_MAGIC.as_slice()) {
        return Err(Error::Corrupt);
    }
    let page_size = read_u32_le(page, 8).ok_or(Error::Corrupt)?;
    if page_size != PAGE as u32 {
        return Err(Error::Corrupt);
    }
    let page_count = read_u64_le(page, 12).ok_or(Error::Corrupt)?;
    if page_count == 0 {
        return Err(Error::Corrupt);
    }
    let free_head = read_u64_le(page, 20).ok_or(Error::Corrupt)?;
    let root = read_u64_le(page, 28).ok_or(Error::Corrupt)?;
    let change_counter = read_u64_le(page, 36).ok_or(Error::Corrupt)?;
    Ok(Page0Meta {
        page_count,
        free_head,
        root,
        change_counter,
    })
}

pub fn encode_wal_header(salt: u64) -> [u8; WAL_HEADER_SIZE] {
    let mut hdr = [0u8; WAL_HEADER_SIZE];
    hdr[0..8].copy_from_slice(WAL_MAGIC);
    hdr[8..12].copy_from_slice(&WAL_VERSION.to_le_bytes());
    hdr[12..16].copy_from_slice(&(PAGE as u32).to_le_bytes());
    hdr[16..24].copy_from_slice(&salt.to_le_bytes());
    let c = crc32(&hdr[0..28]);
    hdr[28..32].copy_from_slice(&c.to_le_bytes());
    hdr
}

pub fn decode_wal_header(hdr: &[u8; WAL_HEADER_SIZE]) -> Option<u64> {
    if hdr.get(0..8) != Some(WAL_MAGIC.as_slice()) {
        return None;
    }
    if read_u32_le(hdr, 8)? != WAL_VERSION {
        return None;
    }
    if read_u32_le(hdr, 12)? != PAGE as u32 {
        return None;
    }
    if hdr.get(24..28) != Some([0u8; 4].as_slice()) {
        return None;
    }
    let expected = crc32(&hdr[0..28]);
    let actual = read_u32_le(hdr, 28)?;
    if expected != actual {
        return None;
    }
    read_u64_le(hdr, 16)
}

pub fn encode_wal_frame(meta: FrameMeta, page: &Page) -> [u8; WAL_FRAME_SIZE] {
    let mut frame = [0u8; WAL_FRAME_SIZE];
    frame[0..8].copy_from_slice(&meta.page_no.to_le_bytes());
    frame[8..16].copy_from_slice(&meta.commit.to_le_bytes());
    frame[16..24].copy_from_slice(&meta.salt.to_le_bytes());
    let mut hasher = Crc32::new();
    hasher.update(&frame[0..28]);
    hasher.update(page);
    let c = hasher.finalize();
    frame[28..32].copy_from_slice(&c.to_le_bytes());
    frame[WAL_HEADER_SIZE..WAL_FRAME_SIZE].copy_from_slice(page);
    frame
}

pub fn decode_wal_frame(frame: &[u8; WAL_FRAME_SIZE], expected_salt: u64) -> Option<FrameMeta> {
    if frame.get(24..28) != Some([0u8; 4].as_slice()) {
        return None;
    }
    let salt = read_u64_le(frame, 16)?;
    if salt != expected_salt {
        return None;
    }
    let mut hasher = Crc32::new();
    hasher.update(&frame[0..28]);
    hasher.update(&frame[WAL_HEADER_SIZE..WAL_FRAME_SIZE]);
    let expected = hasher.finalize();
    let actual = read_u32_le(frame, 28)?;
    if expected != actual {
        return None;
    }
    let page_no = read_u64_le(frame, 0)?;
    let commit = read_u64_le(frame, 8)?;
    Some(FrameMeta {
        page_no,
        commit,
        salt,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn page0_round_trip_and_corruption() {
        let meta = Page0Meta {
            page_count: 7,
            free_head: 3,
            root: 2,
            change_counter: 42,
        };
        let mut p = encode_page0(meta);
        assert_eq!(decode_page0(&p).unwrap(), meta);
        p[15] ^= 1;
        assert_eq!(decode_page0(&p), Err(Error::Corrupt));
    }

    #[test]
    fn wal_header_and_frame_round_trip() {
        let mut hdr = encode_wal_header(99);
        assert_eq!(decode_wal_header(&hdr), Some(99));
        hdr[20] ^= 1;
        assert_eq!(decode_wal_header(&hdr), None);

        let page = encode_free_page(12);
        let mut frame = encode_wal_frame(
            FrameMeta {
                page_no: 4,
                commit: 8,
                salt: 99,
            },
            &page,
        );
        let parsed = decode_wal_frame(&frame, 99).unwrap();
        assert_eq!(parsed.page_no, 4);
        assert_eq!(parsed.commit, 8);
        assert_eq!(decode_wal_frame(&frame, 100), None);
        frame[WAL_HEADER_SIZE + 50] ^= 0x80;
        assert_eq!(decode_wal_frame(&frame, 99), None);
    }
}
