//! WAL recovery, frame I/O, and checkpoint helpers for the pager.

use std::collections::BTreeMap;

use crate::format::{
    decode_page0, decode_wal_frame, decode_wal_header, encode_page0, encode_wal_header,
    verify_page_crc, Page0Meta, WAL_FRAME_SIZE, WAL_HEADER_SIZE,
};
use crate::{Error, Page, Storage, PAGE};

pub struct RecoveredState {
    pub salt: u64,
    pub log_len: u64,
    pub kept_frames: u64,
    pub kept_map: BTreeMap<u64, u64>,
    pub page_frames: BTreeMap<u64, Vec<u64>>,
    pub committed: Page0Meta,
}

pub fn frame_offset(frame_idx: u64) -> Option<u64> {
    frame_idx
        .checked_mul(WAL_FRAME_SIZE as u64)?
        .checked_add(WAL_HEADER_SIZE as u64)
}

pub fn recover_or_init<S: Storage>(main: &mut S, log: &mut S) -> Result<RecoveredState, Error> {
    let main_len = main.len()?;
    let mut kept_frames = 0u64;
    let mut kept_map = BTreeMap::new();
    let mut page_frames: BTreeMap<u64, Vec<u64>> = BTreeMap::new();

    if main_len == 0 {
        let init_meta = Page0Meta {
            page_count: 1,
            free_head: 0,
            root: 0,
            change_counter: 0,
        };
        let page0 = encode_page0(init_meta);
        main.write_at(0, &page0)?;
        main.sync()?;

        let (s, l) = ensure_valid_or_reset_log(log)?;
        let (salt, log_len) = if l > WAL_HEADER_SIZE as u64 {
            (reset_log(log, 1)?, WAL_HEADER_SIZE as u64)
        } else {
            (s, l)
        };

        return Ok(RecoveredState {
            salt,
            log_len,
            kept_frames: 0,
            kept_map,
            page_frames,
            committed: init_meta,
        });
    }

    let (salt, log_len) = ensure_valid_or_reset_log(log)?;

    if log_len > WAL_HEADER_SIZE as u64 {
        let mut frame_idx = 0u64;
        let mut pending: Vec<(u64, u64)> = Vec::new();
        let mut frame_buf = [0u8; WAL_FRAME_SIZE];
        while let Some(off) = frame_offset(frame_idx) {
            let Some(end) = off.checked_add(WAL_FRAME_SIZE as u64) else {
                break;
            };
            if end > log_len {
                break;
            }
            log.read_at(off, &mut frame_buf)?;
            let Some(meta) = decode_wal_frame(&frame_buf, salt) else {
                break;
            };
            pending.push((meta.page_no, frame_idx));
            if meta.commit != 0 {
                for (page_no, idx) in pending.drain(..) {
                    kept_map.insert(page_no, idx);
                    page_frames.entry(page_no).or_default().push(idx);
                }
                kept_frames = frame_idx + 1;
            }
            frame_idx += 1;
        }
    }

    let page0 = if let Some(&idx) = kept_map.get(&0) {
        read_log_frame_page(log, idx)?
    } else {
        if main_len < PAGE as u64 {
            return Err(Error::Corrupt);
        }
        read_main_page(main, 0)?
    };
    let committed = decode_page0(&page0)?;
    if kept_map.keys().any(|&p| p >= committed.page_count) {
        return Err(Error::Corrupt);
    }

    Ok(RecoveredState {
        salt,
        log_len,
        kept_frames,
        kept_map,
        page_frames,
        committed,
    })
}

pub fn checkpoint_wal<S: Storage>(
    main: &mut S,
    log: &mut S,
    kept_map: &BTreeMap<u64, u64>,
    salt: &mut u64,
    log_len: &mut u64,
) -> Result<(), Error> {
    if !kept_map.is_empty() {
        for (&page_no, &frame_idx) in kept_map {
            let page = read_log_frame_page(log, frame_idx)?;
            let off = page_no.checked_mul(PAGE as u64).ok_or(Error::TooLarge)?;
            main.write_at(off, &page)?;
        }
        main.sync()?;
    }

    if !kept_map.is_empty() || *log_len != WAL_HEADER_SIZE as u64 {
        let next_salt = salt.wrapping_add(1);
        let hdr = encode_wal_header(next_salt);
        log.write_at(0, &hdr)?;
        log.sync()?;
        log.set_len(WAL_HEADER_SIZE as u64)?;
        *salt = next_salt;
        *log_len = WAL_HEADER_SIZE as u64;
    }
    Ok(())
}

pub fn reset_log<S: Storage>(log: &mut S, salt: u64) -> Result<u64, Error> {
    log.set_len(0)?;
    let hdr = encode_wal_header(salt);
    log.write_at(0, &hdr)?;
    log.sync()?;
    Ok(salt)
}

pub fn ensure_valid_or_reset_log<S: Storage>(log: &mut S) -> Result<(u64, u64), Error> {
    let len = log.len()?;
    if len >= WAL_HEADER_SIZE as u64 {
        let mut hdr = [0u8; WAL_HEADER_SIZE];
        log.read_at(0, &mut hdr)?;
        if let Some(salt) = decode_wal_header(&hdr) {
            return Ok((salt, len));
        }
    }
    let salt = reset_log(log, 1)?;
    Ok((salt, WAL_HEADER_SIZE as u64))
}

pub fn read_log_frame_page<S: Storage>(log: &mut S, frame_idx: u64) -> Result<Page, Error> {
    let off = frame_offset(frame_idx)
        .and_then(|v| v.checked_add(WAL_HEADER_SIZE as u64))
        .ok_or(Error::TooLarge)?;
    let mut page = [0u8; PAGE];
    log.read_at(off, &mut page)?;
    verify_page_crc(&page)?;
    Ok(page)
}

pub fn read_main_page<S: Storage>(main: &mut S, page_no: u64) -> Result<Page, Error> {
    let off = page_no.checked_mul(PAGE as u64).ok_or(Error::TooLarge)?;
    let mut page = [0u8; PAGE];
    main.read_at(off, &mut page)?;
    verify_page_crc(&page)?;
    Ok(page)
}
