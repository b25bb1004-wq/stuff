//! Path, directory and file operations. Each function works inside the caller's `Txn`, and
//! returns an error before any block is committed if a check fails.
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::layout::{
    self, Extent, Inode, BLOCK64, KIND_DIR, KIND_FILE, KIND_FREE, MAX_EXTENTS, ROOT,
};
use crate::txn::Txn;
use crate::{DirEntry, Error, Kind, Stat, BLOCK};

/// Split an absolute path into checked components. `/` gives no components.
pub(crate) fn parse(path: &str) -> Result<Vec<&str>, Error> {
    let rest = path.strip_prefix('/').ok_or(Error::BadPath)?;
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\0') {
            return Err(Error::BadPath);
        }
        if part.len() > 255 {
            return Err(Error::NameTooLong);
        }
        parts.push(part);
    }
    Ok(parts)
}

/// Byte offset of block `idx` of a file, found through its extents.
fn block_at(ino: &Inode, idx: u64) -> Option<u64> {
    let mut base = 0u64;
    for e in &ino.extents {
        let end = base.checked_add(e.len)?;
        if idx < end {
            return e.start.checked_add(idx - base);
        }
        base = end;
    }
    None
}

fn kind_of(k: u32) -> Result<Kind, Error> {
    match k {
        KIND_FILE => Ok(Kind::File),
        KIND_DIR => Ok(Kind::Dir),
        _ => Err(Error::Corrupt),
    }
}

/// Read `buf.len()` bytes of `ino` from `off`. Short at the end of the file.
fn read_data<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    ino: &Inode,
    off: u64,
    buf: &mut [u8],
) -> Result<usize, Error> {
    if off >= ino.size {
        return Ok(0);
    }
    let len = (buf.len() as u64).min(ino.size - off) as usize;
    let mut done = 0usize;
    while done < len {
        let pos = off + done as u64;
        let within = (pos % BLOCK64) as usize;
        let chunk = (BLOCK - within).min(len - done);
        let blk = block_at(ino, pos / BLOCK64).ok_or(Error::Corrupt)?;
        let src = layout::sub(t.get(blk)?, within, chunk)?;
        layout::put(buf, done, src)?;
        done += chunk;
    }
    Ok(len)
}

/// Add zeroed blocks to the end of `ino`, merging with the last extent when possible.
fn grow<D: BlockDevice>(t: &mut Txn<'_, D>, ino: &mut Inode, count: u64) -> Result<(), Error> {
    for _ in 0..count {
        let hint = match ino.extents.last() {
            Some(e) => e.start.saturating_add(e.len),
            None => t.geo.first_data,
        };
        let blk = t.alloc(hint)?;
        let merge = matches!(ino.extents.last(), Some(e) if e.start.saturating_add(e.len) == blk);
        if merge {
            if let Some(e) = ino.extents.last_mut() {
                e.len = e.len.saturating_add(1);
            }
        } else {
            if ino.extents.len() >= MAX_EXTENTS {
                return Err(Error::TooLarge);
            }
            ino.extents.push(Extent { start: blk, len: 1 });
        }
    }
    Ok(())
}

/// Write `data` into inode `n` at `off`, growing the file. Gaps read as zeros.
fn write_data<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    n: u32,
    off: u64,
    data: &[u8],
) -> Result<usize, Error> {
    if data.is_empty() {
        return Ok(0);
    }
    let mut ino = t.read_inode(n)?;
    let end = off.checked_add(data.len() as u64).ok_or(Error::TooLarge)?;
    let new_size = end.max(ino.size);
    let need = new_size.div_ceil(BLOCK64) - ino.size.div_ceil(BLOCK64);
    grow(t, &mut ino, need)?;
    let mut done = 0usize;
    while done < data.len() {
        let pos = off + done as u64;
        let within = (pos % BLOCK64) as usize;
        let chunk = (BLOCK - within).min(data.len() - done);
        let blk = block_at(&ino, pos / BLOCK64).ok_or(Error::Corrupt)?;
        let src = data.get(done..done + chunk).ok_or(Error::Corrupt)?;
        let page = t.get_mut(blk)?;
        layout::put(page, within, src)?;
        done += chunk;
    }
    ino.size = new_size;
    t.write_inode(n, &mut ino)?;
    Ok(data.len())
}

/// Set the size of inode `n`. Shrinking frees blocks and zeroes the cut-off tail; growing
/// zero-fills.
fn truncate_to<D: BlockDevice>(t: &mut Txn<'_, D>, n: u32, size: u64) -> Result<(), Error> {
    let mut ino = t.read_inode(n)?;
    if size > ino.size {
        let need = size.div_ceil(BLOCK64) - ino.size.div_ceil(BLOCK64);
        grow(t, &mut ino, need)?;
    } else if size < ino.size {
        let keep = size.div_ceil(BLOCK64);
        let mut remove = ino.size.div_ceil(BLOCK64) - keep;
        while remove > 0 {
            let Some(e) = ino.extents.last_mut() else {
                return Err(Error::Corrupt);
            };
            if e.len <= remove {
                let (s, l) = (e.start, e.len);
                ino.extents.pop();
                remove -= l;
                for k in 0..l {
                    t.free(s + k)?;
                }
            } else {
                let s = e.start + e.len - remove;
                e.len -= remove;
                for k in 0..remove {
                    t.free(s + k)?;
                }
                remove = 0;
            }
        }
        if !size.is_multiple_of(BLOCK64) {
            let blk = block_at(&ino, keep - 1).ok_or(Error::Corrupt)?;
            let page = t.get_mut(blk)?;
            if let Some(z) = page.get_mut((size % BLOCK64) as usize..) {
                z.fill(0);
            }
        }
    }
    ino.size = size;
    t.write_inode(n, &mut ino)
}

/// Free all data of inode `n` and mark the inode free.
fn destroy<D: BlockDevice>(t: &mut Txn<'_, D>, n: u32) -> Result<(), Error> {
    truncate_to(t, n, 0)?;
    let mut ino = t.read_inode(n)?;
    ino.kind = KIND_FREE;
    ino.links = 0;
    t.write_inode(n, &mut ino)
}

/// One directory record: `off` in the table, inode 0 when removed, and the name.
struct Rec<'a> {
    off: usize,
    ino: u32,
    name: &'a [u8],
}

fn parse_dir(buf: &[u8]) -> Result<Vec<Rec<'_>>, Error> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let ino = layout::u32_at(buf, pos)?;
        let len = usize::from(*buf.get(pos + 4).ok_or(Error::Corrupt)?);
        if len == 0 {
            return Err(Error::Corrupt);
        }
        let name = layout::sub(buf, pos + 5, len)?;
        if name.iter().any(|&c| c == b'/' || c == 0) {
            return Err(Error::Corrupt);
        }
        out.push(Rec {
            off: pos,
            ino,
            name,
        });
        pos += 5 + len;
    }
    Ok(out)
}

/// The whole entry table of directory `n`.
fn dir_bytes<D: BlockDevice>(t: &mut Txn<'_, D>, n: u32) -> Result<Vec<u8>, Error> {
    let ino = t.read_inode(n)?;
    if ino.kind != KIND_DIR {
        return Err(Error::NotDir);
    }
    let size = usize::try_from(ino.size).map_err(|_| Error::TooLarge)?;
    let mut buf = vec![0u8; size];
    read_data(t, &ino, 0, &mut buf)?;
    Ok(buf)
}

/// Inode of `name` in directory `dir`, if present.
fn lookup<D: BlockDevice>(t: &mut Txn<'_, D>, dir: u32, name: &[u8]) -> Result<Option<u32>, Error> {
    let buf = dir_bytes(t, dir)?;
    let found = parse_dir(&buf)?
        .iter()
        .find(|r| r.ino != 0 && r.name == name)
        .map(|r| r.ino);
    Ok(found)
}

/// Inode at the end of `comps`, walking from the root.
fn resolve<D: BlockDevice>(t: &mut Txn<'_, D>, comps: &[&str]) -> Result<u32, Error> {
    let mut cur = ROOT;
    for c in comps {
        cur = lookup(t, cur, c.as_bytes())?.ok_or(Error::NotFound)?;
    }
    Ok(cur)
}

/// Add an entry `name` -> `child` to directory `dir`, reusing a removed slot of equal length.
fn dir_add<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    dir: u32,
    name: &[u8],
    child: u32,
) -> Result<(), Error> {
    let buf = dir_bytes(t, dir)?;
    let recs = parse_dir(&buf)?;
    if recs.iter().any(|r| r.ino != 0 && r.name == name) {
        return Err(Error::Exists);
    }
    let reuse = recs
        .iter()
        .find(|r| r.ino == 0 && r.name.len() == name.len())
        .map(|r| r.off);
    let len = u8::try_from(name.len()).map_err(|_| Error::NameTooLong)?;
    let mut rec = Vec::with_capacity(5 + name.len());
    rec.extend_from_slice(&child.to_le_bytes());
    rec.push(len);
    rec.extend_from_slice(name);
    let at = match reuse {
        Some(off) => off as u64,
        None => buf.len() as u64,
    };
    write_data(t, dir, at, &rec)?;
    Ok(())
}

/// Remove entry `name` from directory `dir` and return its inode. Trailing removed entries
/// are cut off, so an emptied directory has size 0 again.
fn dir_remove<D: BlockDevice>(t: &mut Txn<'_, D>, dir: u32, name: &[u8]) -> Result<u32, Error> {
    let buf = dir_bytes(t, dir)?;
    let recs = parse_dir(&buf)?;
    let hit = recs
        .iter()
        .find(|r| r.ino != 0 && r.name == name)
        .ok_or(Error::NotFound)?;
    let (off, ino) = (hit.off, hit.ino);
    let keep_end = recs
        .iter()
        .filter(|r| r.ino != 0 && r.off != off)
        .map(|r| r.off + 5 + r.name.len())
        .max()
        .unwrap_or(0);
    write_data(t, dir, off as u64, &0u32.to_le_bytes())?;
    truncate_to(t, dir, keep_end as u64)?;
    Ok(ino)
}

/// Directory entries of `n` that are live.
fn live_names<D: BlockDevice>(t: &mut Txn<'_, D>, n: u32) -> Result<usize, Error> {
    let buf = dir_bytes(t, n)?;
    Ok(parse_dir(&buf)?.iter().filter(|r| r.ino != 0).count())
}

/// Create a file (`kind` = KIND_FILE) or a directory (KIND_DIR) at `path`.
pub(crate) fn make<D: BlockDevice>(t: &mut Txn<'_, D>, path: &str, kind: u32) -> Result<(), Error> {
    let comps = parse(path)?;
    let Some((name, pc)) = comps.split_last() else {
        return Err(Error::Exists);
    };
    let parent = resolve(t, pc)?;
    let n = t.alloc_inode()?;
    t.write_inode(n, &mut Inode::new(kind))?;
    dir_add(t, parent, name.as_bytes(), n)
}

/// Remove the file at `path`.
pub(crate) fn remove<D: BlockDevice>(t: &mut Txn<'_, D>, path: &str) -> Result<(), Error> {
    let comps = parse(path)?;
    let Some((name, pc)) = comps.split_last() else {
        return Err(Error::IsDir);
    };
    let parent = resolve(t, pc)?;
    let n = lookup(t, parent, name.as_bytes())?.ok_or(Error::NotFound)?;
    if t.read_inode(n)?.kind == KIND_DIR {
        return Err(Error::IsDir);
    }
    dir_remove(t, parent, name.as_bytes())?;
    destroy(t, n)
}

/// Remove the empty directory at `path`.
pub(crate) fn rmdir<D: BlockDevice>(t: &mut Txn<'_, D>, path: &str) -> Result<(), Error> {
    let comps = parse(path)?;
    let Some((name, pc)) = comps.split_last() else {
        return Err(Error::BadPath);
    };
    let parent = resolve(t, pc)?;
    let n = lookup(t, parent, name.as_bytes())?.ok_or(Error::NotFound)?;
    if live_names(t, n)? > 0 {
        return Err(Error::NotEmpty);
    }
    dir_remove(t, parent, name.as_bytes())?;
    destroy(t, n)
}

/// Move `from` to `to`. A directory may not move into itself.
pub(crate) fn rename<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    from: &str,
    to: &str,
) -> Result<(), Error> {
    let fc = parse(from)?;
    let tc = parse(to)?;
    let Some((fname, fpc)) = fc.split_last() else {
        return Err(Error::BadPath);
    };
    let Some((tname, tpc)) = tc.split_last() else {
        return Err(Error::Exists);
    };
    let fparent = resolve(t, fpc)?;
    let node = lookup(t, fparent, fname.as_bytes())?.ok_or(Error::NotFound)?;
    if tc.len() > fc.len() && tc.starts_with(&fc) {
        return Err(Error::BadPath);
    }
    let tparent = resolve(t, tpc)?;
    if lookup(t, tparent, tname.as_bytes())?.is_some() {
        return Err(Error::Exists);
    }
    dir_remove(t, fparent, fname.as_bytes())?;
    dir_add(t, tparent, tname.as_bytes(), node)
}

/// The inode of `path`, which must name a file.
fn file_of<D: BlockDevice>(t: &mut Txn<'_, D>, path: &str) -> Result<u32, Error> {
    let comps = parse(path)?;
    let n = resolve(t, &comps)?;
    match t.read_inode(n)?.kind {
        KIND_FILE => Ok(n),
        KIND_DIR => Err(Error::IsDir),
        _ => Err(Error::Corrupt),
    }
}

/// Write `data` into the file at `path`.
pub(crate) fn write<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    path: &str,
    off: u64,
    data: &[u8],
) -> Result<usize, Error> {
    let n = file_of(t, path)?;
    write_data(t, n, off, data)
}

/// Read from the file at `path` into `buf`.
pub(crate) fn read<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    path: &str,
    off: u64,
    buf: &mut [u8],
) -> Result<usize, Error> {
    let n = file_of(t, path)?;
    let ino = t.read_inode(n)?;
    read_data(t, &ino, off, buf)
}

/// Resize the file at `path`.
pub(crate) fn truncate<D: BlockDevice>(
    t: &mut Txn<'_, D>,
    path: &str,
    size: u64,
) -> Result<(), Error> {
    let n = file_of(t, path)?;
    truncate_to(t, n, size)
}

/// Entries of the directory at `path`, sorted by name.
pub(crate) fn list<D: BlockDevice>(t: &mut Txn<'_, D>, path: &str) -> Result<Vec<DirEntry>, Error> {
    let comps = parse(path)?;
    let n = resolve(t, &comps)?;
    let buf = dir_bytes(t, n)?;
    let mut out = Vec::new();
    for r in parse_dir(&buf)? {
        if r.ino == 0 {
            continue;
        }
        let name = core::str::from_utf8(r.name).map_err(|_| Error::Corrupt)?;
        let kind = kind_of(t.read_inode(r.ino)?.kind)?;
        out.push(DirEntry {
            name: String::from(name),
            kind,
            inode: r.ino,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Metadata of `path`.
pub(crate) fn stat<D: BlockDevice>(t: &mut Txn<'_, D>, path: &str) -> Result<Stat, Error> {
    let comps = parse(path)?;
    let n = resolve(t, &comps)?;
    let ino = t.read_inode(n)?;
    Ok(Stat {
        kind: kind_of(ino.kind)?,
        size: ino.size,
        inode: n,
        links: ino.links,
    })
}
