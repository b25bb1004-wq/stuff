#![allow(clippy::unwrap_used)]
//! Property tests: a shadow model checked against the real file system, space accounting,
//! failed operations that change nothing, and corrupted images that must not panic or hang.
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use assay::{forall, Rng};

use crate::layout::Geo;
use crate::{mkfs, BlockDevice, Error, Fs, Kind, MemDisk, MkfsOptions, BLOCK};

pub(crate) const NAMES: [&str; 3] = ["a", "b", "c"];

/// Path rules as the spec states them, written again for the model.
fn mparse(path: &str) -> Result<Vec<String>, Error> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err(Error::BadPath);
    };
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(Error::BadPath);
        }
        if part.len() > 255 {
            return Err(Error::NameTooLong);
        }
        out.push(part.to_string());
    }
    Ok(out)
}

fn path_of(c: &[String]) -> String {
    if c.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", c.join("/"))
    }
}

/// The expected tree: directories (the root is the empty component list) and file contents.
#[derive(Default)]
pub(crate) struct Model {
    pub(crate) files: BTreeMap<Vec<String>, Vec<u8>>,
    pub(crate) dirs: BTreeSet<Vec<String>>,
}

impl Model {
    pub(crate) fn new() -> Model {
        let mut m = Model::default();
        m.dirs.insert(Vec::new());
        m
    }

    fn exists(&self, c: &[String]) -> bool {
        self.dirs.contains(c) || self.files.contains_key(c)
    }

    /// True for a directory, false for a file. NotFound or NotDir along the way.
    fn resolve(&self, c: &[String]) -> Result<bool, Error> {
        let mut cur: Vec<String> = Vec::new();
        let mut is_dir = true;
        for part in c {
            if !is_dir {
                return Err(Error::NotDir);
            }
            cur.push(part.clone());
            if self.dirs.contains(&cur) {
                is_dir = true;
            } else if self.files.contains_key(&cur) {
                is_dir = false;
            } else {
                return Err(Error::NotFound);
            }
        }
        Ok(is_dir)
    }

    fn parent_dir(&self, pc: &[String]) -> Result<(), Error> {
        if self.resolve(pc)? {
            Ok(())
        } else {
            Err(Error::NotDir)
        }
    }

    fn has_children(&self, c: &[String]) -> bool {
        let under = |k: &Vec<String>| k.len() == c.len() + 1 && k.starts_with(c);
        self.dirs.iter().any(under) || self.files.keys().any(under)
    }

    pub(crate) fn make(&mut self, path: &str, dir: bool) -> Result<(), Error> {
        let c = mparse(path)?;
        let Some((_, pc)) = c.split_last() else {
            return Err(Error::Exists);
        };
        self.parent_dir(pc)?;
        if self.exists(&c) {
            return Err(Error::Exists);
        }
        if dir {
            self.dirs.insert(c);
        } else {
            self.files.insert(c, Vec::new());
        }
        Ok(())
    }

    fn file_mut(&mut self, c: &[String]) -> Result<&mut Vec<u8>, Error> {
        if self.resolve(c)? {
            return Err(Error::IsDir);
        }
        self.files.get_mut(c).ok_or(Error::Corrupt)
    }

    pub(crate) fn write(&mut self, path: &str, off: u64, data: &[u8]) -> Result<usize, Error> {
        let c = mparse(path)?;
        let file = self.file_mut(&c)?;
        if data.is_empty() {
            return Ok(0);
        }
        let end = off as usize + data.len();
        if file.len() < end {
            file.resize(end, 0);
        }
        file[off as usize..end].copy_from_slice(data);
        Ok(data.len())
    }

    fn read(&self, path: &str, off: u64, len: usize) -> Result<Vec<u8>, Error> {
        let c = mparse(path)?;
        if self.resolve(&c)? {
            return Err(Error::IsDir);
        }
        let f = self.files.get(&c).ok_or(Error::Corrupt)?;
        let start = (off as usize).min(f.len());
        let end = start.saturating_add(len).min(f.len());
        Ok(f[start..end].to_vec())
    }

    pub(crate) fn truncate(&mut self, path: &str, size: u64) -> Result<(), Error> {
        let c = mparse(path)?;
        self.file_mut(&c)?.resize(size as usize, 0);
        Ok(())
    }

    pub(crate) fn remove(&mut self, path: &str) -> Result<(), Error> {
        let c = mparse(path)?;
        let Some((_, pc)) = c.split_last() else {
            return Err(Error::IsDir);
        };
        self.parent_dir(pc)?;
        if self.dirs.contains(&c) {
            return Err(Error::IsDir);
        }
        self.files.remove(&c).map(|_| ()).ok_or(Error::NotFound)
    }

    pub(crate) fn rmdir(&mut self, path: &str) -> Result<(), Error> {
        let c = mparse(path)?;
        let Some((_, pc)) = c.split_last() else {
            return Err(Error::BadPath);
        };
        self.parent_dir(pc)?;
        if self.files.contains_key(&c) {
            return Err(Error::NotDir);
        }
        if !self.dirs.contains(&c) {
            return Err(Error::NotFound);
        }
        if self.has_children(&c) {
            return Err(Error::NotEmpty);
        }
        self.dirs.remove(&c);
        Ok(())
    }

    pub(crate) fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        let fc = mparse(from)?;
        let tc = mparse(to)?;
        let Some((_, fpc)) = fc.split_last() else {
            return Err(Error::BadPath);
        };
        let Some((_, tpc)) = tc.split_last() else {
            return Err(Error::Exists);
        };
        self.parent_dir(fpc)?;
        if !self.exists(&fc) {
            return Err(Error::NotFound);
        }
        if tc.len() > fc.len() && tc.starts_with(&fc) {
            return Err(Error::BadPath);
        }
        self.parent_dir(tpc)?;
        if self.exists(&tc) {
            return Err(Error::Exists);
        }
        let dkeys: Vec<Vec<String>> = self
            .dirs
            .iter()
            .filter(|k| k.starts_with(&fc))
            .cloned()
            .collect();
        for k in &dkeys {
            self.dirs.remove(k);
        }
        for k in dkeys {
            let mut nk = tc.clone();
            nk.extend_from_slice(&k[fc.len()..]);
            self.dirs.insert(nk);
        }
        let fkeys: Vec<Vec<String>> = self
            .files
            .keys()
            .filter(|k| k.starts_with(&fc))
            .cloned()
            .collect();
        for k in fkeys {
            let data = self.files.remove(&k).unwrap_or_default();
            let mut nk = tc.clone();
            nk.extend_from_slice(&k[fc.len()..]);
            self.files.insert(nk, data);
        }
        Ok(())
    }

    fn list(&self, path: &str) -> Result<Vec<(String, bool)>, Error> {
        let c = mparse(path)?;
        if !self.resolve(&c)? {
            return Err(Error::NotDir);
        }
        let child = |k: &Vec<String>| k.len() == c.len() + 1 && k.starts_with(&c);
        let mut out: Vec<(String, bool)> = Vec::new();
        for k in self.dirs.iter().filter(|k| child(k)) {
            out.push((k.last().cloned().unwrap_or_default(), true));
        }
        for k in self.files.keys().filter(|k| child(k)) {
            out.push((k.last().cloned().unwrap_or_default(), false));
        }
        out.sort();
        Ok(out)
    }

    /// (is directory, size of a file or 0 for a directory).
    fn stat(&self, path: &str) -> Result<(bool, u64), Error> {
        let c = mparse(path)?;
        let is_dir = self.resolve(&c)?;
        let size = self.files.get(&c).map_or(0, |f| f.len() as u64);
        Ok((is_dir, size))
    }
}

#[derive(Debug)]
pub(crate) enum Op {
    Make {
        path: String,
        dir: bool,
    },
    Write {
        path: String,
        off: u64,
        data: Vec<u8>,
    },
    Read {
        path: String,
        off: u64,
        len: usize,
    },
    Truncate {
        path: String,
        size: u64,
    },
    Remove {
        path: String,
    },
    Rmdir {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    List {
        path: String,
    },
    Stat {
        path: String,
    },
}

fn gen_path(r: &mut Rng, names: &[&str]) -> String {
    match r.below(40) {
        0 => {
            return String::from(
                *r.pick(&["", "x", "/a/", "//", "/a//b", "/./a", "/../a", "/a/.."]),
            )
        }
        1 => return format!("/{}", "z".repeat(256)),
        _ => {}
    }
    let n = r.below(4);
    let mut s = String::new();
    if n == 0 {
        s.push('/');
    }
    for _ in 0..n {
        s.push('/');
        s.push_str(r.pick(names));
    }
    s
}

pub(crate) fn gen_op(r: &mut Rng, names: &[&str]) -> Op {
    match r.below(20) {
        0..=2 => Op::Make {
            path: gen_path(r, names),
            dir: false,
        },
        3 => Op::Make {
            path: gen_path(r, names),
            dir: true,
        },
        4..=8 => {
            let len = if r.chance(1, 4) { 0 } else { r.below(20_001) };
            let mut data = vec![0u8; len];
            r.fill(&mut data);
            Op::Write {
                path: gen_path(r, names),
                off: r.range(0, 40_000),
                data,
            }
        }
        9 => Op::Read {
            path: gen_path(r, names),
            off: r.range(0, 40_000),
            len: r.below(25_000),
        },
        10 => Op::Truncate {
            path: gen_path(r, names),
            size: r.range(0, 30_000),
        },
        11 => Op::Remove {
            path: gen_path(r, names),
        },
        12 => Op::Rmdir {
            path: gen_path(r, names),
        },
        13 => Op::Rename {
            from: gen_path(r, names),
            to: gen_path(r, names),
        },
        14 => Op::List {
            path: gen_path(r, names),
        },
        _ => Op::Stat {
            path: gen_path(r, names),
        },
    }
}

pub(crate) fn apply<D: BlockDevice>(fs: &mut Fs<D>, m: &mut Model, op: &Op) {
    match op {
        Op::Make { path, dir } => {
            let got = if *dir {
                fs.mkdir(path)
            } else {
                fs.create(path)
            };
            assert_eq!(got, m.make(path, *dir), "{op:?}");
        }
        Op::Write { path, off, data } => {
            assert_eq!(
                fs.write(path, *off, data),
                m.write(path, *off, data),
                "{op:?}"
            );
        }
        Op::Read { path, off, len } => {
            let mut buf = vec![0u8; *len];
            let got = fs.read(path, *off, &mut buf).map(|n| buf[..n].to_vec());
            assert_eq!(got, m.read(path, *off, *len), "{op:?}");
        }
        Op::Truncate { path, size } => {
            assert_eq!(fs.truncate(path, *size), m.truncate(path, *size), "{op:?}");
        }
        Op::Remove { path } => assert_eq!(fs.remove(path), m.remove(path), "{op:?}"),
        Op::Rmdir { path } => assert_eq!(fs.rmdir(path), m.rmdir(path), "{op:?}"),
        Op::Rename { from, to } => assert_eq!(fs.rename(from, to), m.rename(from, to), "{op:?}"),
        Op::List { path } => {
            let got = fs.list(path).map(|v| {
                v.into_iter()
                    .map(|e| (e.name, e.kind == Kind::Dir))
                    .collect::<Vec<_>>()
            });
            assert_eq!(got, m.list(path), "{op:?}");
        }
        Op::Stat { path } => {
            let got = fs.stat(path).map(|s| {
                (
                    s.kind == Kind::Dir,
                    if s.kind == Kind::File { s.size } else { 0 },
                )
            });
            assert_eq!(got, m.stat(path), "{op:?}");
        }
    }
}

/// Run an operation on a possibly corrupt image; only panics and hangs matter here.
fn run_unchecked(fs: &mut Fs<MemDisk>, op: &Op) {
    match op {
        Op::Make { path, dir } => {
            let _ = if *dir {
                fs.mkdir(path)
            } else {
                fs.create(path)
            };
        }
        Op::Write { path, off, data } => {
            let _ = fs.write(path, *off, data);
        }
        Op::Read { path, off, len } => {
            let mut buf = vec![0u8; *len];
            let _ = fs.read(path, *off, &mut buf);
        }
        Op::Truncate { path, size } => {
            let _ = fs.truncate(path, *size);
        }
        Op::Remove { path } => {
            let _ = fs.remove(path);
        }
        Op::Rmdir { path } => {
            let _ = fs.rmdir(path);
        }
        Op::Rename { from, to } => {
            let _ = fs.rename(from, to);
        }
        Op::List { path } => {
            let _ = fs.list(path);
        }
        Op::Stat { path } => {
            let _ = fs.stat(path);
        }
    }
}

fn check_tree(fs: &mut Fs<MemDisk>, m: &Model) {
    for d in &m.dirs {
        let p = path_of(d);
        let got: Vec<(String, bool)> = fs
            .list(&p)
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.kind == Kind::Dir))
            .collect();
        assert_eq!(Ok(got), m.list(&p), "tree at {p}");
    }
    for (f, data) in &m.files {
        let p = path_of(f);
        assert_eq!(fs.stat(&p).unwrap().size, data.len() as u64, "size of {p}");
        let mut buf = vec![0u8; data.len()];
        assert_eq!(fs.read(&p, 0, &mut buf).unwrap(), data.len());
        assert!(&buf == data, "bytes of {p}");
    }
}

#[test]
fn shadow_model_matches_every_result() {
    forall(100, 0x5EED_0006, |r| {
        let mut d = MemDisk::new(1024);
        mkfs(&mut d, MkfsOptions::default()).unwrap();
        let mut fs = Fs::mount(d).unwrap();
        let mut m = Model::new();
        for step in 0..300 {
            let op = gen_op(r, &NAMES);
            apply(&mut fs, &mut m, &op);
            if step % 50 == 49 {
                let d = fs.unmount().unwrap();
                fs = Fs::mount(d).unwrap();
                check_tree(&mut fs, &m);
            }
        }
        check_tree(&mut fs, &m);
    });
}

#[test]
fn full_disk_fails_cleanly_and_space_returns_to_mkfs_value() {
    let mut d = MemDisk::new(512);
    mkfs(
        &mut d,
        MkfsOptions {
            inodes: 0,
            journal_blocks: 8,
        },
    )
    .unwrap();
    let mut fs = Fs::mount(d).unwrap();
    let base = fs.statfs().unwrap();
    fs.mkdir("/d").unwrap();
    fs.create("/d/x").unwrap();
    let chunk = vec![0xABu8; 3000];
    let mut full = false;
    for i in 0..400u32 {
        let p = format!("/f{i}");
        match fs.create(&p) {
            Ok(()) => {
                let mut off = 0u64;
                loop {
                    match fs.write(&p, off, &chunk) {
                        Ok(n) => off += n as u64,
                        Err(Error::NoSpace) => break,
                        Err(e) => panic!("unexpected {e:?}"),
                    }
                }
            }
            Err(Error::NoSpace) => {
                full = true;
                break;
            }
            Err(e) => panic!("unexpected {e:?}"),
        }
    }
    assert!(full, "the disk should fill up");
    // A failed write changes nothing on disk.
    let d = fs.unmount().unwrap();
    let snap = d.bytes().to_vec();
    let mut fs = Fs::mount(d).unwrap();
    let end = fs.stat("/f0").unwrap().size;
    assert_eq!(
        fs.write("/f0", end + (1 << 20), &chunk),
        Err(Error::NoSpace)
    );
    assert_eq!(fs.rename("/d", "/d/x/y"), Err(Error::BadPath));
    let d = fs.unmount().unwrap();
    assert!(
        d.bytes() == snap.as_slice(),
        "failed operations must not touch the disk"
    );
    let mut fs = Fs::mount(d).unwrap();
    // Remove everything: the free space must match the value right after mkfs.
    for e in fs.list("/").unwrap() {
        let p = format!("/{}", e.name);
        match e.kind {
            Kind::File => fs.remove(&p).unwrap(),
            Kind::Dir => {
                for c in fs.list(&p).unwrap() {
                    fs.remove(&format!("{p}/{}", c.name)).unwrap();
                }
                fs.rmdir(&p).unwrap();
            }
        }
    }
    assert_eq!(fs.statfs().unwrap(), base);
}

fn populated_image() -> Vec<u8> {
    let mut d = MemDisk::new(256);
    mkfs(
        &mut d,
        MkfsOptions {
            inodes: 64,
            journal_blocks: 8,
        },
    )
    .unwrap();
    let mut fs = Fs::mount(d).unwrap();
    fs.create("/a").unwrap();
    fs.create("/b").unwrap();
    fs.mkdir("/c").unwrap();
    fs.create("/c/a").unwrap();
    // Interleaved writes fragment both files, so they need the indirect block.
    for k in 0..10u64 {
        fs.write("/a", k * 4096, &[1u8; 4096]).unwrap();
        fs.write("/b", k * 4096, &[2u8; 4096]).unwrap();
    }
    fs.write("/c/a", 0, b"hello").unwrap();
    fs.unmount().unwrap().bytes().to_vec()
}

#[test]
fn corrupted_images_never_panic_or_hang() {
    let image = populated_image();
    let g = Geo::plan(256, 64, 8).unwrap();
    forall(300, 0xC0DE, |r| {
        let mut d = MemDisk::new(256);
        d.bytes_mut().copy_from_slice(&image);
        for _ in 0..r.range(1, 9) {
            let blk = match r.below(4) {
                0 => 0,
                1 => g.bitmap_start,
                2 => g.itable_start + r.range(0, g.itable_len),
                _ => g.first_data + r.range(0, 256 - g.first_data),
            };
            let at = blk as usize * BLOCK + r.below(BLOCK);
            let flip = 1 + r.below(255) as u8;
            if let Some(b) = d.bytes_mut().get_mut(at) {
                *b ^= flip;
            }
        }
        match Fs::mount(d) {
            Err(e) => assert_eq!(e, Error::Corrupt),
            Ok(mut fs) => {
                for _ in 0..40 {
                    let op = gen_op(r, &NAMES);
                    run_unchecked(&mut fs, &op);
                }
                let _ = fs.statfs();
            }
        }
    });
}

#[test]
fn random_bytes_do_not_mount() {
    forall(300, 0xBAD, |r| {
        let mut d = MemDisk::new(256);
        r.fill(d.bytes_mut());
        assert_eq!(Fs::mount(d).err(), Some(Error::Corrupt));
    });
}

#[test]
fn block_count_is_reported() {
    assert_eq!(MemDisk::new(7).block_count(), 7);
}
