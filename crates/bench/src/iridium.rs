//! Benchmarks for `iridium`: in-memory and file-backed B+tree operations.

use std::ops::Bound;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use iridium::{FileStorage, MemStorage, Pager, Storage, Tree};

use crate::{measure, Opts, Sample};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Run all `iridium` benchmarks.
pub fn run(opts: &Opts) -> Result<Vec<Sample>, String> {
    let scale = usize::try_from(opts.scale.max(1)).map_err(|_| "scale overflow".to_string())?;
    let mem_n = 20_000usize
        .checked_mul(scale)
        .ok_or_else(|| "dataset size overflow".to_string())?;
    let file_n = 10_000usize
        .checked_mul(scale)
        .ok_or_else(|| "file dataset size overflow".to_string())?;

    Ok(vec![
        bench_seq_insert(opts, mem_n)?,
        bench_rand_insert(opts, mem_n)?,
        bench_lookup_hit(opts, mem_n)?,
        bench_range_scan(opts, mem_n)?,
        bench_file_insert(opts, file_n)?,
    ])
}

fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn make_value(k: &[u8; 8]) -> [u8; 32] {
    let mut val = [0u8; 32];
    val[..8].copy_from_slice(k);
    for (idx, byte) in val[8..].iter_mut().enumerate() {
        *byte = k[idx % 8] ^ (idx as u8).wrapping_mul(31);
    }
    val
}

fn populate_mem_tree(n: usize, randomized: bool) -> Result<(Pager<MemStorage>, Tree), String> {
    let mut pager = Pager::open(MemStorage::new(), MemStorage::new())
        .map_err(|e| format!("pager open failed: {e}"))?;
    pager
        .begin()
        .map_err(|e| format!("pager begin failed: {e}"))?;
    let tree = Tree::create(&mut pager).map_err(|e| format!("tree create failed: {e}"))?;
    for i in 0..n {
        let raw = if randomized {
            mix64(i as u64)
        } else {
            i as u64
        };
        let key = raw.to_be_bytes();
        let val = make_value(&key);
        let prev = tree
            .insert(&mut pager, &key, &val)
            .map_err(|e| format!("tree insert failed: {e}"))?;
        if prev.is_some() {
            return Err("unexpected duplicate key during population".to_string());
        }
    }
    pager
        .commit()
        .map_err(|e| format!("pager commit failed: {e}"))?;
    Ok((pager, tree))
}

fn verify_samples<S: Storage>(
    pager: &mut Pager<S>,
    tree: &Tree,
    n: usize,
    randomized: bool,
) -> Result<(), String> {
    if n == 0 {
        return Ok(());
    }
    for idx in [0usize, n / 2, n - 1] {
        let raw = if randomized {
            mix64(idx as u64)
        } else {
            idx as u64
        };
        let key = raw.to_be_bytes();
        let expected = make_value(&key);
        let got = tree
            .get(pager, None, &key)
            .map_err(|e| format!("sample get failed: {e}"))?;
        if got.as_deref() != Some(&expected) {
            return Err(format!("sample get mismatch at index {idx}"));
        }
    }
    Ok(())
}

fn bench_seq_insert(opts: &Opts, n: usize) -> Result<Sample, String> {
    measure("iridium", "seq_insert", "keys/s", opts.budget, || {
        let (mut pager, tree) = populate_mem_tree(n, false)?;
        verify_samples(&mut pager, &tree, n, false)?;
        Ok(n as u64)
    })
}

fn bench_rand_insert(opts: &Opts, n: usize) -> Result<Sample, String> {
    measure("iridium", "rand_insert", "keys/s", opts.budget, || {
        let (mut pager, tree) = populate_mem_tree(n, true)?;
        verify_samples(&mut pager, &tree, n, true)?;
        Ok(n as u64)
    })
}

fn bench_lookup_hit(opts: &Opts, n: usize) -> Result<Sample, String> {
    let (mut pager, tree) = populate_mem_tree(n, true)?;
    measure("iridium", "lookup_hit", "keys/s", opts.budget, || {
        for i in 0..n {
            let key = mix64(i as u64).to_be_bytes();
            let expected = make_value(&key);
            let got = tree
                .get(&mut pager, None, &key)
                .map_err(|e| format!("lookup get failed: {e}"))?;
            if got.as_deref() != Some(&expected) {
                return Err(format!("lookup mismatch at index {i}"));
            }
        }
        Ok(n as u64)
    })
}

fn bench_range_scan(opts: &Opts, n: usize) -> Result<Sample, String> {
    let (mut pager, tree) = populate_mem_tree(n, false)?;
    measure("iridium", "range_scan", "keys/s", opts.budget, || {
        let mut count = 0usize;
        let mut prev_key: Option<[u8; 8]> = None;
        let mut scan_err: Option<String> = None;

        tree.scan(&mut pager, None, Bound::Unbounded, &mut |k, v| {
            let Ok(key_arr): Result<[u8; 8], _> = k.try_into() else {
                scan_err = Some("scan returned key with unexpected length".to_string());
                return false;
            };
            if let Some(prev) = prev_key {
                if key_arr <= prev {
                    scan_err = Some("scan keys out of order".to_string());
                    return false;
                }
            }
            if v != make_value(&key_arr) {
                scan_err = Some("scan value mismatch".to_string());
                return false;
            }
            prev_key = Some(key_arr);
            count += 1;
            true
        })
        .map_err(|e| format!("range scan failed: {e}"))?;

        if let Some(err) = scan_err {
            return Err(err);
        }
        if count != n {
            return Err(format!("range scan visited {count} keys, expected {n}"));
        }
        Ok(n as u64)
    })
}

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn create() -> Result<Self, String> {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("monolith_bench_iridium_{pid}_{seq}"));
        std::fs::create_dir_all(&path).map_err(|e| format!("create temp dir failed: {e}"))?;
        Ok(Self { path })
    }

    fn cleanup(self) -> Result<(), String> {
        std::fs::remove_dir_all(&self.path).map_err(|e| format!("remove temp dir failed: {e}"))
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn bench_file_insert(opts: &Opts, n: usize) -> Result<Sample, String> {
    measure("iridium", "file_insert", "inserts/s", opts.budget, || {
        let dir = TempDirGuard::create()?;
        let main_path = dir.path.join("main.db");
        let log_path = dir.path.join("main.wal");
        let main = FileStorage::open(&main_path).map_err(|e| format!("open main failed: {e}"))?;
        let log = FileStorage::open(&log_path).map_err(|e| format!("open log failed: {e}"))?;
        let mut pager = Pager::open(main, log).map_err(|e| format!("pager open failed: {e}"))?;

        pager
            .begin()
            .map_err(|e| format!("pager begin failed: {e}"))?;
        let tree = Tree::create(&mut pager).map_err(|e| format!("tree create failed: {e}"))?;

        for i in 0..n {
            let key = (i as u64).to_be_bytes();
            let val = make_value(&key);
            let prev = tree
                .insert(&mut pager, &key, &val)
                .map_err(|e| format!("file tree insert failed: {e}"))?;
            if prev.is_some() {
                return Err("unexpected duplicate key in file insert".to_string());
            }
            if (i + 1) % 1000 == 0 && i + 1 < n {
                pager
                    .commit()
                    .map_err(|e| format!("file commit failed: {e}"))?;
                pager
                    .begin()
                    .map_err(|e| format!("file begin failed: {e}"))?;
            }
        }
        pager
            .commit()
            .map_err(|e| format!("final file commit failed: {e}"))?;

        verify_samples(&mut pager, &tree, n, false)?;
        let _ = pager
            .close()
            .map_err(|e| format!("pager close failed: {e}"))?;
        dir.cleanup()?;
        Ok(n as u64)
    })
}
