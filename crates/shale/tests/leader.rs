//! Leader-run shadow model and corruption run (public API only, fresh seeds).
//! `STRATA_CASES` scales both (default 20).
#![allow(clippy::unwrap_used)]
use assay::{forall, Rng};
use shale::{mkfs, Fs, Kind, MemDisk, MkfsOptions};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Debug)]
enum Node {
    File(Vec<u8>),
    Dir,
}
type Model = BTreeMap<String, Node>;

fn cases() -> u32 {
    std::env::var("STRATA_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20)
}

fn parent_ok(m: &Model, path: &str) -> bool {
    let p = &path[..path.rfind('/').unwrap()];
    p.is_empty() || m.get(p) == Some(&Node::Dir)
}

fn path(r: &mut Rng) -> String {
    const DIRS: [&str; 5] = ["", "/a", "/a/b", "/c", "/a/x"];
    const NAMES: [&str; 6] = ["a", "b", "c", "x", "y", "z"];
    format!("{}/{}", DIRS[r.below(5)], NAMES[r.below(6)])
}

fn step(r: &mut Rng, fs: &mut Fs<MemDisk>, m: &mut Model) {
    let p = path(r);
    match r.below(10) {
        0 | 1 => {
            let dir = r.below(2) == 0;
            let ok = parent_ok(m, &p) && !m.contains_key(&p);
            let got = if dir { fs.mkdir(&p) } else { fs.create(&p) };
            assert_eq!(got.is_ok(), ok, "create/mkdir {p}: {got:?}");
            if ok {
                m.insert(
                    p,
                    if dir {
                        Node::Dir
                    } else {
                        Node::File(Vec::new())
                    },
                );
            }
        }
        2 | 3 => {
            let off = r.below(60_000);
            let mut data = r.bytes(20_000);
            if data.is_empty() {
                data.push(1);
            }
            let got = fs.write(&p, off as u64, &data);
            match m.get_mut(&p) {
                Some(Node::File(f)) => {
                    assert_eq!(got, Ok(data.len()), "write {p}");
                    if f.len() < off + data.len() {
                        f.resize(off + data.len(), 0);
                    }
                    f[off..off + data.len()].copy_from_slice(&data);
                }
                _ => assert!(got.is_err(), "write {p} should fail"),
            }
        }
        4 => {
            let off = r.below(90_000);
            let mut buf = vec![0xAA; r.below(30_000)];
            let got = fs.read(&p, off as u64, &mut buf);
            match m.get(&p) {
                Some(Node::File(f)) => {
                    let want = &f[off.min(f.len())..(off + buf.len()).min(f.len())];
                    assert_eq!(&buf[..got.unwrap()], want, "read {p} at {off}");
                }
                _ => assert!(got.is_err(), "read {p} should fail"),
            }
        }
        5 => {
            let size = r.below(90_000);
            let got = fs.truncate(&p, size as u64);
            match m.get_mut(&p) {
                Some(Node::File(f)) => {
                    assert_eq!(got, Ok(()), "truncate {p}");
                    f.resize(size, 0);
                }
                _ => assert!(got.is_err(), "truncate {p} should fail"),
            }
        }
        6 => {
            let ok = matches!(m.get(&p), Some(Node::File(_)));
            assert_eq!(fs.remove(&p).is_ok(), ok, "remove {p}");
            if ok {
                m.remove(&p);
            }
        }
        7 => {
            let inside = format!("{p}/");
            let ok = m.get(&p) == Some(&Node::Dir) && !m.keys().any(|k| k.starts_with(&inside));
            assert_eq!(fs.rmdir(&p).is_ok(), ok, "rmdir {p}");
            if ok {
                m.remove(&p);
            }
        }
        8 => {
            let q = path(r);
            let inside = format!("{p}/");
            let ok = m.contains_key(&p)
                && !m.contains_key(&q)
                && parent_ok(m, &q)
                && !q.starts_with(&inside);
            let got = fs.rename(&p, &q);
            assert_eq!(got.is_ok(), ok, "rename {p} -> {q}: {got:?}");
            if ok {
                let moved: Vec<String> = m
                    .keys()
                    .filter(|k| **k == p || k.starts_with(&inside))
                    .cloned()
                    .collect();
                for k in moved {
                    let node = m.remove(&k).unwrap();
                    m.insert(format!("{q}{}", &k[p.len()..]), node);
                }
            }
        }
        _ => match (fs.stat(&p), m.get(&p)) {
            (Ok(s), Some(Node::File(f))) => {
                assert!(
                    matches!(s.kind, Kind::File) && s.size == f.len() as u64,
                    "stat {p}"
                )
            }
            (Ok(s), Some(Node::Dir)) => assert!(matches!(s.kind, Kind::Dir), "stat {p}"),
            (Err(_), None) => {}
            (got, want) => panic!("stat {p}: {:?} vs {want:?}", got.is_ok()),
        },
    }
}

fn compare(fs: &mut Fs<MemDisk>, m: &Model) {
    let mut seen = 0;
    let mut stack = vec![String::new()];
    while let Some(d) = stack.pop() {
        let entries = fs.list(if d.is_empty() { "/" } else { &d }).unwrap();
        let prefix = format!("{d}/");
        let want: Vec<&String> = m
            .keys()
            .filter(|k| k.starts_with(&prefix) && !k[prefix.len()..].contains('/'))
            .collect();
        assert_eq!(entries.len(), want.len(), "list {d}");
        for (e, k) in entries.iter().zip(want) {
            assert_eq!(&format!("{prefix}{}", e.name), k);
            match &m[k] {
                Node::Dir => {
                    assert!(matches!(e.kind, Kind::Dir));
                    stack.push(k.clone());
                }
                Node::File(f) => {
                    assert!(matches!(e.kind, Kind::File));
                    let mut buf = vec![0; f.len() + 10];
                    let n = fs.read(k, 0, &mut buf).unwrap();
                    assert_eq!(&buf[..n], &f[..], "content of {k}");
                }
            }
            seen += 1;
        }
    }
    assert_eq!(seen, m.len());
}

#[test]
fn shadow_model_fresh_seed() {
    forall(cases(), 0x51A7_2026, |r| {
        let mut d = MemDisk::new(4096);
        mkfs(&mut d, MkfsOptions::default()).unwrap();
        let mut fs = Fs::mount(d).unwrap();
        let base = fs.statfs().unwrap();
        let mut m = Model::new();
        for i in 0..400 {
            step(r, &mut fs, &mut m);
            if i % 40 == 39 {
                fs = Fs::mount(fs.unmount().unwrap()).unwrap();
                compare(&mut fs, &m);
            }
        }
        for (k, node) in m.iter().rev() {
            match node {
                Node::File(_) => fs.remove(k).unwrap(),
                Node::Dir => fs.rmdir(k).unwrap(),
            }
        }
        let end = fs.statfs().unwrap();
        assert_eq!(
            (end.free_blocks, end.free_inodes),
            (base.free_blocks, base.free_inodes),
            "space leaked"
        );
    });
}

#[test]
fn corrupted_images_never_panic() {
    const PATHS: [&str; 7] = ["/", "/a", "/a/b", "/a/f", "/a/b/g", "/h", "/new"];
    forall(cases() * 50, 0xC0FF_EE11, |r| {
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
        fs.mkdir("/a").unwrap();
        fs.mkdir("/a/b").unwrap();
        for f in ["/a/f", "/a/b/g", "/h"] {
            fs.create(f).unwrap();
            let data = r.bytes(30_000);
            fs.write(f, r.below(9000) as u64, &data).unwrap();
        }
        let mut d = fs.unmount().unwrap();
        let n = d.bytes().len();
        for _ in 0..=r.below(8) {
            let i = if r.below(4) > 0 {
                r.below(32 * 4096)
            } else {
                r.below(n)
            };
            d.bytes_mut()[i] = r.below(256) as u8;
        }
        if let Ok(mut fs) = Fs::mount(d) {
            let mut buf = [0u8; 5000];
            for p in PATHS {
                let _ = fs.stat(p);
                let _ = fs.list(p);
                let _ = fs.read(p, 100, &mut buf);
                let _ = fs.write(p, 3000, &buf);
                let _ = fs.truncate(p, 10);
                let _ = fs.create(p);
                let _ = fs.mkdir(p);
                let _ = fs.statfs();
            }
            let _ = fs.rename("/a/b", "/moved");
            for p in PATHS {
                let _ = fs.remove(p);
                let _ = fs.rmdir(p);
            }
            let _ = fs.unmount();
        }
    });
}
