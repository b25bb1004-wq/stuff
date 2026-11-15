//! Integration tests for Pager semantics, snapshots, misuse/corrupt handling, and FileStorage.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;

use assay::forall;
use varve::{Error, FileStorage, MemStorage, Pager, PAGE, USABLE};

fn crc32_ref(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

fn temp_dir(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("varve_test_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn open_empty_initializes_page0_and_log_header() {
    let p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    assert_eq!(p.page_count(), 1);
    assert_eq!(p.root(), 0);
    assert_eq!(p.log_frames(), 0);
    let (main, log) = p.into_storage();

    assert_eq!(main.bytes().len(), PAGE);
    assert_eq!(&main.bytes()[0..8], b"VARVE\0v1");
    assert_eq!(
        u32::from_le_bytes(main.bytes()[8..12].try_into().unwrap()),
        4096
    );
    assert_eq!(
        u64::from_le_bytes(main.bytes()[12..20].try_into().unwrap()),
        1
    );
    assert_eq!(
        u64::from_le_bytes(main.bytes()[20..28].try_into().unwrap()),
        0
    );
    assert_eq!(
        u64::from_le_bytes(main.bytes()[28..36].try_into().unwrap()),
        0
    );
    assert_eq!(
        u64::from_le_bytes(main.bytes()[36..44].try_into().unwrap()),
        0
    );
    assert_eq!(
        u32::from_le_bytes(main.bytes()[USABLE..PAGE].try_into().unwrap()),
        crc32_ref(&main.bytes()[..USABLE])
    );

    assert_eq!(log.bytes().len(), 32);
    assert_eq!(&log.bytes()[0..8], b"VARVEWAL");
    assert_eq!(
        u32::from_le_bytes(log.bytes()[8..12].try_into().unwrap()),
        1
    );
    assert_eq!(
        u32::from_le_bytes(log.bytes()[12..16].try_into().unwrap()),
        4096
    );
    assert_eq!(
        u64::from_le_bytes(log.bytes()[16..24].try_into().unwrap()),
        1
    );
    assert_eq!(
        u32::from_le_bytes(log.bytes()[28..32].try_into().unwrap()),
        crc32_ref(&log.bytes()[..28])
    );
}

#[test]
fn commits_never_touch_main_and_write_ignores_caller_crc() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    let (main0, log0) = p.into_storage();
    let main_initial = main0.bytes().to_vec();
    p = Pager::open(main0, log0).unwrap();

    p.begin().unwrap();
    let p1 = p.alloc().unwrap();
    assert_eq!(p1, 1);
    let zero_page = p.read(p1).unwrap();
    assert!(zero_page[..USABLE].iter().all(|&b| b == 0));
    assert_eq!(
        u32::from_le_bytes(zero_page[USABLE..PAGE].try_into().unwrap()),
        crc32_ref(&zero_page[..USABLE])
    );

    let mut payload = [0xABu8; PAGE];
    payload[USABLE..PAGE].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    p.write(p1, &payload).unwrap();
    p.set_root(p1).unwrap();
    p.commit().unwrap();

    let got = p.read(p1).unwrap();
    assert_eq!(&got[..USABLE], &payload[..USABLE]);
    assert_eq!(
        u32::from_le_bytes(got[USABLE..PAGE].try_into().unwrap()),
        crc32_ref(&got[..USABLE])
    );
    assert_eq!(p.log_frames(), 2);

    let (main1, log1) = p.into_storage();
    assert_eq!(main1.bytes(), main_initial.as_slice());

    let mut p = Pager::open(main1, log1).unwrap();
    assert_eq!(p.page_count(), 2);
    assert_eq!(p.root(), 1);
    assert_eq!(p.read(p1).unwrap(), got);
}

#[test]
fn free_list_lifo_order_and_format() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let a = p.alloc().unwrap();
    let b = p.alloc().unwrap();
    let c = p.alloc().unwrap();
    assert_eq!((a, b, c), (1, 2, 3));
    p.commit().unwrap();

    p.begin().unwrap();
    p.free(a).unwrap();
    let fa = p.read(a).unwrap();
    assert_eq!(fa[0], 5);
    assert_eq!(u64::from_le_bytes(fa[16..24].try_into().unwrap()), 0);
    assert!(fa[1..16].iter().all(|&x| x == 0));
    assert!(fa[24..USABLE].iter().all(|&x| x == 0));

    p.free(b).unwrap();
    let fb = p.read(b).unwrap();
    assert_eq!(fb[0], 5);
    assert_eq!(u64::from_le_bytes(fb[16..24].try_into().unwrap()), a);
    p.commit().unwrap();

    p.begin().unwrap();
    assert_eq!(p.alloc().unwrap(), b);
    assert_eq!(p.alloc().unwrap(), a);
    assert_eq!(p.alloc().unwrap(), 4);
    assert_eq!(p.page_count(), 5);
    p.commit().unwrap();
}

#[test]
fn snapshots_block_checkpoint_and_preserve_history() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let p1 = p.alloc().unwrap();
    let mut page_v1 = [1u8; PAGE];
    page_v1[0] = 11;
    p.write(p1, &page_v1).unwrap();
    p.commit().unwrap();

    let s1 = p.snapshot();

    p.begin().unwrap();
    let p2 = p.alloc().unwrap();
    let mut page_v2 = [2u8; PAGE];
    page_v2[0] = 22;
    p.write(p1, &page_v2).unwrap();
    p.write(p2, &page_v2).unwrap();
    // Snapshot taken during uncommitted tx must only see committed state.
    let s1_again = p.snapshot();
    assert_eq!(p.read(p1).unwrap()[..USABLE], page_v2[..USABLE]);
    assert_eq!(
        p.read_snapshot(s1, p1).unwrap()[..USABLE],
        page_v1[..USABLE]
    );
    assert_eq!(
        p.read_snapshot(s1_again, p1).unwrap()[..USABLE],
        page_v1[..USABLE]
    );
    p.commit().unwrap();

    let s2 = p.snapshot();
    assert_eq!(
        p.read_snapshot(s1, p1).unwrap()[..USABLE],
        page_v1[..USABLE]
    );
    assert_eq!(p.read_snapshot(s1, p2), Err(Error::Misuse));
    assert_eq!(
        p.read_snapshot(s2, p1).unwrap()[..USABLE],
        page_v2[..USABLE]
    );
    assert_eq!(
        p.read_snapshot(s2, p2).unwrap()[..USABLE],
        page_v2[..USABLE]
    );

    assert!(!p.checkpoint().unwrap());
    assert!(p.log_frames() > 0);

    p.release(s1);
    assert_eq!(p.read_snapshot(s1, p1), Err(Error::Misuse));
    assert!(!p.checkpoint().unwrap());

    p.release(s1_again);
    p.release(s2);
    assert!(p.checkpoint().unwrap());
    assert_eq!(p.log_frames(), 0);
}

#[test]
fn auto_checkpoint_after_1000_frames_respects_snapshots() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let pg = p.alloc().unwrap();
    p.commit().unwrap();

    let snap = p.snapshot();
    // Each empty/single-page commit writes 2 frames (pg + page 0).
    for i in 0..510u64 {
        p.begin().unwrap();
        let mut buf = [0u8; PAGE];
        buf[..8].copy_from_slice(&i.to_le_bytes());
        p.write(pg, &buf).unwrap();
        p.commit().unwrap();
    }
    assert!(p.log_frames() > 1000);
    p.release(snap);

    // Next commit exceeds 1000 frames with no snapshot -> auto-checkpoints.
    p.begin().unwrap();
    p.commit().unwrap();
    assert_eq!(p.log_frames(), 0);
}

#[test]
fn close_rolls_back_and_checkpoints_ignoring_snapshots() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let p1 = p.alloc().unwrap();
    let mut buf = [7u8; PAGE];
    p.write(p1, &buf).unwrap();
    p.commit().unwrap();

    let _snap = p.snapshot();
    p.begin().unwrap();
    buf[0] = 99;
    p.write(p1, &buf).unwrap();
    let (main, log) = p.close().unwrap();

    let mut p2 = Pager::open(main, log).unwrap();
    assert_eq!(p2.log_frames(), 0);
    assert_eq!(p2.read(p1).unwrap()[0], 7);
}

#[test]
fn misuse_and_busy_errors() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    let dummy = [0u8; PAGE];
    assert_eq!(p.write(1, &dummy), Err(Error::Misuse));
    assert_eq!(p.alloc(), Err(Error::Misuse));
    assert_eq!(p.free(1), Err(Error::Misuse));
    assert_eq!(p.set_root(0), Err(Error::Misuse));
    assert_eq!(p.commit(), Err(Error::Misuse));
    assert_eq!(p.read(1), Err(Error::Misuse));
    p.rollback(); // no-op outside tx

    p.begin().unwrap();
    assert_eq!(p.begin(), Err(Error::Busy));
    assert_eq!(p.write(0, &dummy), Err(Error::Misuse));
    assert_eq!(p.write(1, &dummy), Err(Error::Misuse));
    assert_eq!(p.free(0), Err(Error::Misuse));
    assert_eq!(p.free(1), Err(Error::Misuse));

    let p1 = p.alloc().unwrap();
    p.free(p1).unwrap();
    assert_eq!(p.free(p1), Err(Error::Misuse));
    p.rollback();
    assert_eq!(p.page_count(), 1);
}

#[test]
fn hostile_main_and_log_never_panic() {
    assert_eq!(
        Pager::open(MemStorage::from_bytes(vec![1, 2, 3]), MemStorage::new()).unwrap_err(),
        Error::Corrupt
    );
    let mut bad_main = vec![0u8; PAGE];
    bad_main[0..8].copy_from_slice(b"NOPE\0\0\0\0");
    assert_eq!(
        Pager::open(MemStorage::from_bytes(bad_main), MemStorage::new()).unwrap_err(),
        Error::Corrupt
    );

    forall(200, 0x5A5A, |rng| {
        let main_len = rng.range(0, 9000) as usize;
        let log_len = rng.range(0, 9000) as usize;
        let mut m = vec![0u8; main_len];
        let mut l = vec![0u8; log_len];
        rng.fill(&mut m);
        rng.fill(&mut l);
        let res = Pager::open(MemStorage::from_bytes(m), MemStorage::from_bytes(l));
        if let Ok(mut pager) = res {
            let _ = pager.read(0);
            let _ = pager.read(1);
        }
    });
}

#[test]
fn corrupt_page_in_main_detected_and_trailing_log_ignored() {
    let mut p = Pager::open(MemStorage::new(), MemStorage::new()).unwrap();
    p.begin().unwrap();
    let p1 = p.alloc().unwrap();
    let mut page = [0x33u8; PAGE];
    p.write(p1, &page).unwrap();
    p.commit().unwrap();
    let (main, mut log) = p.close().unwrap();

    // Append garbage trailing uncommitted bytes to log.
    use varve::Storage;
    log.write_at(32, &[0xFF; 5000]).unwrap();
    let mut p = Pager::open(main, log).unwrap();
    assert_eq!(p.log_frames(), 0);
    assert_eq!(p.read(p1).unwrap()[..USABLE], page[..USABLE]);

    // Corrupt page 1 in main and reopen.
    let (mut main, log) = p.close().unwrap();
    page[10] ^= 0x01;
    main.write_at(PAGE as u64 + 10, &[page[10]]).unwrap();
    let mut p = Pager::open(main, log).unwrap();
    assert_eq!(p.read(p1), Err(Error::Corrupt));
}

#[test]
fn file_storage_round_trip_in_temp_dir() {
    let dir = temp_dir("filestorage");
    let main_path = dir.join("db.main");
    let log_path = dir.join("db.main-wal");

    {
        let main = FileStorage::open(&main_path).unwrap();
        let log = FileStorage::open(&log_path).unwrap();
        let mut p = Pager::open(main, log).unwrap();
        p.begin().unwrap();
        let p1 = p.alloc().unwrap();
        let mut buf = [0x5Cu8; PAGE];
        buf[0..4].copy_from_slice(b"TEST");
        p.write(p1, &buf).unwrap();
        p.set_root(p1).unwrap();
        p.commit().unwrap();
        p.close().unwrap();
    }

    {
        let main = FileStorage::open(&main_path).unwrap();
        let log = FileStorage::open(&log_path).unwrap();
        let mut p = Pager::open(main, log).unwrap();
        assert_eq!(p.page_count(), 2);
        assert_eq!(p.root(), 1);
        let got = p.read(1).unwrap();
        assert_eq!(&got[0..4], b"TEST");
        p.close().unwrap();
    }

    let _ = fs::remove_dir_all(&dir);
}
