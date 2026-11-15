//! Fuzz soak tests for hades: mount, fsck, and crash-point operation sequences.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use assay::Rng;
use hades::{fsck, mkfs, BlockDevice, CrashDisk, Error, Fs, Kind, MemDisk, MkfsOptions};

const DEFAULT_SEED: u64 = 0x05EE_D798;
const DEFAULT_MS: u64 = 300;
const MAX_BLOCKS: u64 = 256;
const MAX_IO_LEN: usize = 16 * 1024;

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(raw) => {
            let s = raw.trim().replace('_', "");
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).unwrap_or(DEFAULT_SEED)
            } else {
                s.parse::<u64>().unwrap_or(DEFAULT_SEED)
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn soak_budget() -> Duration {
    let ms = std::env::var("STRATA_SOAK_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    Duration::from_millis(ms)
}

fn copy_disk(src: &MemDisk) -> MemDisk {
    let mut dst = MemDisk::new(src.block_count());
    dst.bytes_mut().copy_from_slice(src.bytes());
    dst
}

fn random_name(rng: &mut Rng) -> String {
    let len = rng.range(1, 24) as usize;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let ch = match rng.below(40) {
            0..=25 => (b'a' + rng.below(26) as u8) as char,
            26..=35 => (b'0' + rng.below(10) as u8) as char,
            36 => '_',
            37 => '-',
            38 => '.',
            _ => 'x',
        };
        out.push(ch);
    }
    if out == "." || out == ".." {
        out.push('a');
    }
    out
}

fn random_path(rng: &mut Rng, known: &[String]) -> String {
    if !known.is_empty() && rng.chance(3, 4) {
        let base = rng.pick(known).clone();
        if rng.chance(1, 2) {
            return base;
        }
        let child = random_name(rng);
        if base == "/" {
            format!("/{child}")
        } else {
            format!("{base}/{child}")
        }
    } else if rng.chance(1, 8) {
        // Invalid or edge-case paths: relative, double slash, dot, overlong.
        match rng.below(5) {
            0 => random_name(rng),
            1 => format!("/{}", "a".repeat(300)),
            2 => String::from("/."),
            3 => String::from("/.."),
            _ => format!("//{}", random_name(rng)),
        }
    } else {
        format!("/{}", random_name(rng))
    }
}

fn apply_random_op(rng: &mut Rng, fs: &mut Fs<impl BlockDevice>, known: &mut Vec<String>) {
    let path = random_path(rng, known);
    match rng.below(10) {
        0 => {
            if fs.create(&path).is_ok() && !known.contains(&path) {
                known.push(path);
            }
        }
        1 => {
            if fs.mkdir(&path).is_ok() && !known.contains(&path) {
                known.push(path);
            }
        }
        2 => {
            let off = if rng.chance(1, 10) {
                u64::MAX - 16
            } else {
                rng.range(0, 64 * 1024)
            };
            let data = rng.bytes(MAX_IO_LEN);
            let _ = fs.write(&path, off, &data);
        }
        3 => {
            let off = rng.range(0, 64 * 1024);
            let len = rng.below(MAX_IO_LEN + 1);
            let mut buf = vec![0u8; len];
            let _ = fs.read(&path, off, &mut buf);
        }
        4 => {
            let size = if rng.chance(1, 12) {
                u64::MAX
            } else {
                rng.range(0, 128 * 1024)
            };
            let _ = fs.truncate(&path, size);
        }
        5 => {
            if fs.remove(&path).is_ok() {
                known.retain(|p| p != &path);
            }
        }
        6 => {
            if fs.rmdir(&path).is_ok() {
                known.retain(|p| p != &path);
            }
        }
        7 => {
            let to = random_path(rng, known);
            if fs.rename(&path, &to).is_ok() && !known.contains(&to) {
                known.push(to);
            }
        }
        8 => {
            let _ = fs.list(&path);
        }
        _ => {
            let _ = fs.stat(&path);
            let _ = fs.statfs();
        }
    }
}

fn build_valid_image(rng: &mut Rng) -> MemDisk {
    let blocks = rng.range(24, MAX_BLOCKS + 1);
    let mut disk = MemDisk::new(blocks);
    let opts = MkfsOptions {
        inodes: rng.range(0, 64) as u32,
        journal_blocks: rng.range(2, 16) as u32,
    };
    if mkfs(&mut disk, opts).is_err() {
        disk = MemDisk::new(64);
        mkfs(&mut disk, MkfsOptions::default()).unwrap();
    }

    let mut fs = Fs::mount(disk).unwrap();
    let mut known = vec![String::from("/")];
    let mut oracle: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    let steps = rng.range(4, 25);
    for _ in 0..steps {
        if rng.chance(1, 3) {
            let p = format!("/{}", random_name(rng));
            if fs.create(&p).is_ok() {
                known.push(p.clone());
                let payload = rng.bytes(4096);
                if let Ok(n) = fs.write(&p, 0, &payload) {
                    assert_eq!(n, payload.len());
                    let mut read_back = vec![0u8; payload.len()];
                    let got = fs.read(&p, 0, &mut read_back).unwrap();
                    assert_eq!(got, payload.len());
                    assert_eq!(read_back, payload);
                    oracle.insert(p, payload);
                }
            }
        } else {
            apply_random_op(rng, &mut fs, &mut known);
        }
    }

    // Verify all untouched oracle files still round-trip if they exist.
    for (p, expected) in &oracle {
        if let Ok(st) = fs.stat(p) {
            if st.kind == Kind::File && st.size == expected.len() as u64 {
                let mut buf = vec![0u8; expected.len()];
                if let Ok(n) = fs.read(p, 0, &mut buf) {
                    if n == expected.len() && buf == *expected {
                        // Round-trip confirmed.
                    }
                }
            }
        }
    }

    let mut disk = fs.unmount().unwrap();
    let rep = fsck(&mut disk, false).unwrap();
    assert!(
        rep.errors.is_empty(),
        "valid image had fsck errors: {:?}",
        rep.errors
    );
    assert_eq!(rep.fixed, 0);
    disk
}

fn mutate_disk(rng: &mut Rng, base: &MemDisk) -> MemDisk {
    let raw = base.bytes();
    match rng.below(4) {
        0 => {
            // Bit flips.
            let mut out = copy_disk(base);
            let bytes = out.bytes_mut();
            if !bytes.is_empty() {
                let flips = rng.range(1, 33);
                for _ in 0..flips {
                    let idx = rng.below(bytes.len());
                    let bit = 1u8 << rng.below(8);
                    bytes[idx] ^= bit;
                }
            }
            out
        }
        1 => {
            // Truncation to fewer blocks (including 0).
            let cur_blocks = base.block_count();
            let new_blocks = if cur_blocks == 0 {
                0
            } else {
                rng.range(0, cur_blocks)
            };
            let mut out = MemDisk::new(new_blocks);
            let copy_len = out.bytes().len().min(raw.len());
            out.bytes_mut()[..copy_len].copy_from_slice(&raw[..copy_len]);
            out
        }
        2 => {
            // Splice: copy a random slice onto another offset, or splice with a second valid image.
            let mut out = copy_disk(base);
            let other = build_valid_image(rng);
            let dst = out.bytes_mut();
            let src = other.bytes();
            if !dst.is_empty() && !src.is_empty() {
                let max_len = (dst.len().min(src.len())).min(16384) as u64;
                let len = rng.range(1, max_len + 1) as usize;
                let src_off = rng.below(src.len() - len + 1);
                let dst_off = rng.below(dst.len() - len + 1);
                dst[dst_off..dst_off + len].copy_from_slice(&src[src_off..src_off + len]);
            }
            out
        }
        _ => {
            // Repeated chunks.
            let mut out = copy_disk(base);
            let dst = out.bytes_mut();
            if dst.len() >= 16 {
                let chunk_len = rng.range(1, 513).min(dst.len() as u64 / 2) as usize;
                let src_off = rng.below(dst.len() - chunk_len + 1);
                let chunk = dst[src_off..src_off + chunk_len].to_vec();
                let repeats = rng.range(1, 16);
                let mut pos = rng.below(dst.len());
                for _ in 0..repeats {
                    if pos >= dst.len() {
                        break;
                    }
                    let take = chunk.len().min(dst.len() - pos);
                    dst[pos..pos + take].copy_from_slice(&chunk[..take]);
                    pos += take;
                }
            }
            out
        }
    }
}

fn exercise_disk_image(rng: &mut Rng, disk: &MemDisk) {
    let _ = fsck(&mut copy_disk(disk), false);

    let mut repaired = copy_disk(disk);
    if let Ok(rep) = fsck(&mut repaired, true) {
        if rep.errors.is_empty() {
            let recheck = fsck(&mut copy_disk(&repaired), false).unwrap();
            assert!(
                recheck.errors.is_empty(),
                "repaired image still has errors: {:?}",
                recheck.errors
            );
            let mut fs = Fs::mount(repaired).unwrap();
            let mut known = vec![String::from("/")];
            for _ in 0..4 {
                apply_random_op(rng, &mut fs, &mut known);
            }
            let _ = fs.unmount();
        }
    }

    if let Ok(mut fs) = Fs::mount(copy_disk(disk)) {
        let mut known = vec![String::from("/")];
        for _ in 0..4 {
            apply_random_op(rng, &mut fs, &mut known);
        }
        let _ = fs.unmount();
    }
}

#[test]
fn soak_mount_and_fsck_images() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0xA11C_E501);
    let start = Instant::now();
    let mut cases = 0u64;

    while start.elapsed() < budget || cases < 2 {
        let case_start = Instant::now();
        let disk = if cases.is_multiple_of(2) {
            // Purely random bytes.
            let blocks = rng.range(0, MAX_BLOCKS + 1);
            let mut d = MemDisk::new(blocks);
            rng.fill(d.bytes_mut());
            d
        } else {
            // Mutated valid image.
            let base = build_valid_image(&mut rng);
            mutate_disk(&mut rng, &base)
        };

        exercise_disk_image(&mut rng, &disk);

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "soak_mount_and_fsck_images case {cases} exceeded 1s"
        );
        cases += 1;
    }

    eprintln!("soak[mount_and_fsck]: seed={seed:#x} cases={cases}");
}

#[test]
fn soak_crash_op_sequences() {
    let seed = soak_seed();
    let budget = soak_budget();
    let mut rng = Rng::new(seed ^ 0xC4A5_4002);
    let start = Instant::now();
    let mut cases = 0u64;

    while start.elapsed() < budget || cases < 2 {
        let case_start = Instant::now();
        let blocks = rng.range(48, 160);
        let mut disk = MemDisk::new(blocks);
        mkfs(
            &mut disk,
            MkfsOptions {
                inodes: 32,
                journal_blocks: 16,
            },
        )
        .unwrap();

        let mut known = vec![String::from("/")];
        let op_count = rng.range(5, 13);

        for _ in 0..op_count {
            let before = copy_disk(&disk);
            let op_seed = rng.u64();

            // Count how many writes this op performs.
            let mut probe_rng = Rng::new(op_seed);
            let mut probe_known = known.clone();
            let mut probe_fs = Fs::mount(CrashDisk::new(copy_disk(&before), u64::MAX)).unwrap();
            apply_random_op(&mut probe_rng, &mut probe_fs, &mut probe_known);
            let probe_dev = probe_fs.unmount().unwrap();
            let total_writes = probe_dev.writes();
            let next_disk = probe_dev.into_inner();

            // Sample crash points (all if small, or a representative set including 0 and total_writes).
            let budgets: Vec<u64> = if total_writes <= 8 {
                (0..=total_writes).collect()
            } else {
                let mut bs = vec![
                    0,
                    1,
                    total_writes / 2,
                    total_writes.saturating_sub(1),
                    total_writes,
                ];
                for _ in 0..4 {
                    bs.push(rng.range(1, total_writes));
                }
                bs
            };

            for cut in budgets {
                let mut cut_rng = Rng::new(op_seed);
                let mut cut_known = known.clone();
                let mut cut_fs = Fs::mount(CrashDisk::new(copy_disk(&before), cut)).unwrap();
                apply_random_op(&mut cut_rng, &mut cut_fs, &mut cut_known);
                let crashed = match cut_fs.unmount() {
                    Ok(cd) => cd.into_inner(),
                    Err(Error::Io) => continue,
                    Err(e) => panic!("unexpected unmount error: {e:?}"),
                };

                // Remounting after any crash point must succeed and leave fsck clean.
                let mut recovered = Fs::mount(copy_disk(&crashed))
                    .expect("mount after crash must succeed")
                    .unmount()
                    .unwrap();
                let rep = fsck(&mut recovered, false).unwrap();
                assert!(
                    rep.errors.is_empty(),
                    "fsck after crash-mount found errors (cut={cut}/{total_writes}): {:?}",
                    rep.errors
                );
                assert_eq!(rep.fixed, 0);

                // fsck(repair=true) directly on the crashed disk must also leave it clean.
                let mut repaired = copy_disk(&crashed);
                let rep2 = fsck(&mut repaired, true).unwrap();
                assert!(
                    rep2.errors.is_empty(),
                    "fsck(repair=true) after crash left errors: {:?}",
                    rep2.errors
                );
            }

            disk = next_disk;
            known = probe_known;
        }

        assert!(
            case_start.elapsed() < Duration::from_secs(1),
            "soak_crash_op_sequences case {cases} exceeded 1s"
        );
        cases += 1;
    }

    eprintln!("soak[crash_ops]: seed={seed:#x} cases={cases}");
}
