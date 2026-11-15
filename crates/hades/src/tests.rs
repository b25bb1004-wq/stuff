#![allow(clippy::unwrap_used)]
//! Unit tests: mkfs and mount at several sizes, path rules, rename rules and bad images.
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::{mkfs, Error, Fs, Kind, MemDisk, MkfsOptions};

fn fresh_fs(blocks: u64, journal_blocks: u32) -> Fs<MemDisk> {
    let mut d = MemDisk::new(blocks);
    mkfs(
        &mut d,
        MkfsOptions {
            inodes: 0,
            journal_blocks,
        },
    )
    .unwrap();
    Fs::mount(d).unwrap()
}

#[test]
fn mkfs_mount_and_data_round_trip() {
    for (blocks, journal) in [(64u64, 8u32), (256, 64), (4096, 64)] {
        let mut fs = fresh_fs(blocks, journal);
        fs.mkdir("/d").unwrap();
        fs.create("/d/f").unwrap();
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        assert_eq!(fs.write("/d/f", 0, &data).unwrap(), data.len());
        let d = fs.unmount().unwrap();
        let mut fs = Fs::mount(d).unwrap();
        let mut buf = vec![0u8; 12_000];
        let n = fs.read("/d/f", 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], &data[..]);
        assert_eq!(fs.stat("/d/f").unwrap().size, 10_000);
        assert_eq!(fs.list("/").unwrap()[0].kind, Kind::Dir);
    }
}

#[test]
fn too_small_device_is_no_space() {
    for blocks in [0u64, 1, 3, 30] {
        let mut d = MemDisk::new(blocks);
        assert_eq!(mkfs(&mut d, MkfsOptions::default()), Err(Error::NoSpace));
    }
}

#[test]
fn zeroed_or_empty_device_does_not_mount() {
    assert_eq!(Fs::mount(MemDisk::new(64)).err(), Some(Error::Corrupt));
    assert_eq!(Fs::mount(MemDisk::new(0)).err(), Some(Error::Corrupt));
}

#[test]
fn path_rules() {
    let mut fs = fresh_fs(256, 8);
    assert_eq!(fs.create("relative"), Err(Error::BadPath));
    assert_eq!(fs.create("/a/"), Err(Error::BadPath));
    assert_eq!(fs.create("/a//b"), Err(Error::BadPath));
    assert_eq!(
        fs.create(&format!("/{}", "x".repeat(256))),
        Err(Error::NameTooLong)
    );
    assert_eq!(fs.create("/"), Err(Error::Exists));
    assert_eq!(fs.rmdir("/"), Err(Error::BadPath));
    assert_eq!(fs.remove("/"), Err(Error::IsDir));
    assert_eq!(fs.create("/x/y"), Err(Error::NotFound));
    fs.create("/f").unwrap();
    assert_eq!(fs.create("/f/g"), Err(Error::NotDir));
    assert_eq!(fs.mkdir("/f"), Err(Error::Exists));
    assert_eq!(fs.read("/", 0, &mut [0u8; 4]), Err(Error::IsDir));
    assert_eq!(fs.rmdir("/f"), Err(Error::NotDir));
    assert_eq!(fs.remove("/nothere"), Err(Error::NotFound));
}

#[test]
fn rename_and_rmdir_rules() {
    let mut fs = fresh_fs(256, 8);
    fs.mkdir("/a").unwrap();
    fs.mkdir("/a/b").unwrap();
    fs.create("/f").unwrap();
    assert_eq!(fs.rename("/a", "/a/b/c"), Err(Error::BadPath));
    assert_eq!(fs.rename("/f", "/a"), Err(Error::Exists));
    assert_eq!(fs.rmdir("/a"), Err(Error::NotEmpty));
    fs.rename("/f", "/g").unwrap();
    let names: Vec<_> = fs.list("/").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["a", "g"]);
    fs.rmdir("/a/b").unwrap();
    fs.rmdir("/a").unwrap();
    assert_eq!(fs.stat("/a").err(), Some(Error::NotFound));
}

#[test]
fn truncate_grow_is_zero_filled_and_shrink_cuts() {
    let mut fs = fresh_fs(256, 8);
    fs.create("/t").unwrap();
    fs.write("/t", 0, &[9u8; 5000]).unwrap();
    fs.truncate("/t", 100).unwrap();
    fs.truncate("/t", 6000).unwrap();
    let mut buf = vec![7u8; 6000];
    assert_eq!(fs.read("/t", 0, &mut buf).unwrap(), 6000);
    assert!(buf[..100].iter().all(|&b| b == 9));
    assert!(buf[100..].iter().all(|&b| b == 0));
}
