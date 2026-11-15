//! Fuzz soak tests for chronos parsers, decoders, diff and merge.

#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use assay::Rng;
use chronos::remote::{decode_pack, encode_pack};
use chronos::{
    decode_commit, decode_tree, diff, encode_commit, encode_tree, from_hex, hash_object, lines,
    merge3, to_hex, unified, Commit, Edit, Id, Kind, MemStore, Mode, Repo, Signature, Store,
    TreeEntry,
};

const DEFAULT_SEED: u64 = 0x05EE_D795;
const DEFAULT_MS: u64 = 300;
const MAX_INPUT_BYTES: usize = 32 * 1024;
const CASE_TIMEOUT: Duration = Duration::from_secs(1);

fn soak_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(&hex.replace('_', ""), 16).expect("STRATA_SOAK_SEED: bad hex")
            } else {
                s.replace('_', "")
                    .parse::<u64>()
                    .expect("STRATA_SOAK_SEED: bad number")
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

fn rand_id(rng: &mut Rng) -> Id {
    let mut id = [0u8; 32];
    rng.fill(&mut id);
    id
}

fn rand_ascii_ident(rng: &mut Rng, max_len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-";
    let len = rng.range(1, max_len.max(1) as u64 + 1) as usize;
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(*rng.pick(CHARS) as char);
    }
    s
}

fn rand_utf8_text(rng: &mut Rng, max_len: usize) -> String {
    let len = rng.below(max_len + 1);
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        let ch = match rng.range(0, 10) {
            0 => '\n',
            1 => ' ',
            2 => '\t',
            3..=7 => (b'a' + rng.below(26) as u8) as char,
            8 => (b'0' + rng.below(10) as u8) as char,
            _ => *rng.pick(&[
                '<', '>', '/', ':', '-', '_', '.', '@', '\u{00e9}', '\u{2603}',
            ]),
        };
        s.push(ch);
    }
    s
}

fn rand_lines_bytes(rng: &mut Rng, max_lines: usize) -> Vec<u8> {
    let n = rng.below(max_lines + 1);
    let mut out = Vec::new();
    for i in 0..n {
        let line_len = rng.below(40);
        for _ in 0..line_len {
            let b = *rng.pick(b"abcd0123 \txyz\n");
            out.push(b);
        }
        if i + 1 < n || rng.chance(3, 4) {
            out.push(b'\n');
        }
    }
    out.truncate(MAX_INPUT_BYTES);
    out
}

fn mutate_bytes(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    if base.is_empty() {
        return rng.bytes(64);
    }
    let mut out = base.to_vec();
    let ops = rng.range(1, 5);
    for _ in 0..ops {
        if out.is_empty() {
            out = rng.bytes(32);
            continue;
        }
        match rng.range(0, 5) {
            // Bit flip
            0 => {
                let idx = rng.below(out.len());
                let bit = 1u8 << rng.below(8);
                out[idx] ^= bit;
            }
            // Truncation
            1 => {
                let new_len = rng.below(out.len() + 1);
                out.truncate(new_len);
            }
            // Splice with random bytes
            2 => {
                let split = rng.below(out.len() + 1);
                let tail_start = rng.below(out.len() + 1);
                let mid = rng.bytes(64);
                let mut spliced = Vec::with_capacity(
                    (split + mid.len() + (out.len() - tail_start)).min(MAX_INPUT_BYTES),
                );
                spliced.extend_from_slice(&out[..split]);
                spliced.extend_from_slice(&mid);
                spliced.extend_from_slice(&out[tail_start..]);
                spliced.truncate(MAX_INPUT_BYTES);
                out = spliced;
            }
            // Repeated chunk
            3 => {
                let start = rng.below(out.len());
                let end = start + rng.below((out.len() - start).min(64) + 1);
                let chunk = out[start..end].to_vec();
                let reps = rng.range(2, 6) as usize;
                for _ in 0..reps {
                    if out.len().saturating_add(chunk.len()) > MAX_INPUT_BYTES {
                        break;
                    }
                    out.extend_from_slice(&chunk);
                }
            }
            // Byte overwrite / insert
            _ => {
                let idx = rng.below(out.len());
                out[idx] = (rng.u64() & 0xff) as u8;
            }
        }
    }
    out.truncate(MAX_INPUT_BYTES);
    out
}

fn mutate_text(rng: &mut Rng, base: &str) -> String {
    let mut raw = mutate_bytes(rng, base.as_bytes());
    if raw.len() > MAX_INPUT_BYTES {
        raw.truncate(MAX_INPUT_BYTES);
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn run_target(name: &str, base_seed: u64, budget: Duration, mut step: impl FnMut(&mut Rng, u64)) {
    let mut master = Rng::new(base_seed);
    let start = Instant::now();
    let mut cases: u64 = 0;
    while start.elapsed() < budget || cases < 10 {
        let case_seed = master.u64();
        let mut rng = Rng::new(case_seed);
        let t0 = Instant::now();
        step(&mut rng, cases);
        let dt = t0.elapsed();
        assert!(
            dt <= CASE_TIMEOUT,
            "soak target {name} exceeded 1s ({dt:?}) on case {cases} (seed {case_seed:#018x})"
        );
        cases += 1;
    }
    println!("soak target={name} seed={base_seed:#018x} cases={cases}");
}

fn gen_valid_pack_objects(rng: &mut Rng) -> Vec<(Kind, Vec<u8>)> {
    let n = rng.below(12);
    let mut objs = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = *rng.pick(&[Kind::Blob, Kind::Tree, Kind::Commit]);
        let data = rng.bytes(256);
        objs.push((kind, data));
    }
    objs
}

fn gen_valid_tree_entries(rng: &mut Rng) -> Vec<TreeEntry> {
    let n = rng.below(16);
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let mode = *rng.pick(&[Mode::File, Mode::Exec, Mode::Dir]);
        let name = rand_ascii_ident(rng, 16);
        let id = rand_id(rng);
        entries.push(TreeEntry { mode, name, id });
    }
    entries
}

fn gen_valid_commit(rng: &mut Rng) -> Commit {
    let parent_count = rng.below(4);
    let parents = (0..parent_count).map(|_| rand_id(rng)).collect();
    let make_sig = |rng: &mut Rng| {
        let sign = if rng.chance(1, 2) { 1 } else { -1 };
        let hh = rng.range(0, 14) as i32;
        let mm = *rng.pick(&[0, 15, 30, 45]);
        Signature {
            name: format!("{} {}", rand_ascii_ident(rng, 8), rand_ascii_ident(rng, 8)),
            email: format!("{}@example.com", rand_ascii_ident(rng, 10)),
            time: rng.range(0, 2_000_000_000) as i64,
            tz_minutes: sign * (hh * 60 + mm),
        }
    };
    Commit {
        tree: rand_id(rng),
        parents,
        author: make_sig(rng),
        committer: make_sig(rng),
        message: rand_utf8_text(rng, 128),
    }
}

fn apply_edits(a: &[u8], b: &[u8], edits: &[Edit]) -> Vec<u8> {
    let mut out = Vec::new();
    for &e in edits {
        match e {
            Edit::Keep(i, _) => out.push(a[i]),
            Edit::Delete(_) => {}
            Edit::Insert(j) => out.push(b[j]),
        }
    }
    out
}

#[test]
fn soak_all_targets() {
    let seed = soak_seed();
    let budget = soak_budget();

    // 1. Pack decoder (and round-trip)
    run_target("decode_pack", seed ^ 0x01, budget, |rng, idx| {
        let objs = gen_valid_pack_objects(rng);
        let packed = encode_pack(&objs);
        let decoded = decode_pack(&packed, 1000).unwrap();
        assert_eq!(decoded, objs);

        let input = if idx % 2 == 0 {
            rng.bytes(1024)
        } else {
            mutate_bytes(rng, &packed)
        };
        let _ = decode_pack(&input, 1024);
    });

    // 2. Tree decoder (and round-trip)
    run_target("decode_tree", seed ^ 0x02, budget, |rng, idx| {
        let entries = gen_valid_tree_entries(rng);
        let encoded = encode_tree(&entries);
        let decoded = decode_tree(&encoded).unwrap();
        assert_eq!(encode_tree(&decoded), encoded);

        let input = if idx % 2 == 0 {
            rng.bytes(1024)
        } else {
            mutate_bytes(rng, &encoded)
        };
        let _ = decode_tree(&input);
    });

    // 3. Commit decoder (and round-trip)
    run_target("decode_commit", seed ^ 0x03, budget, |rng, idx| {
        let commit = gen_valid_commit(rng);
        let encoded = encode_commit(&commit);
        let decoded = decode_commit(&encoded).unwrap();
        assert_eq!(decoded, commit);

        let input = if idx % 2 == 0 {
            rng.bytes(1024)
        } else {
            mutate_bytes(rng, &encoded)
        };
        let _ = decode_commit(&input);
    });

    // 4. Object / repo decoders (Repo::get, Repo::open, Repo::staged, Repo::head, Repo::branches)
    run_target("repo_object_decoder", seed ^ 0x04, budget, |rng, idx| {
        let mut repo = Repo::init(MemStore::new());
        let kind = *rng.pick(&[Kind::Blob, Kind::Tree, Kind::Commit]);
        let data = rng.bytes(512);
        let id = repo.put(kind, &data);
        assert_eq!(repo.get(&id), Ok((kind, data.clone())));
        assert_eq!(hash_object(kind, &data), id);

        let hex = to_hex(&id);
        let obj_key = format!("objects/{}/{}", &hex[..2], &hex[2..]);
        let orig_raw = repo.store.read(&obj_key).unwrap();
        let bad_raw = if idx % 2 == 0 {
            rng.bytes(512)
        } else {
            mutate_bytes(rng, &orig_raw)
        };
        repo.store.write(&obj_key, &bad_raw);
        let _ = repo.get(&id);

        let bad_head = if idx % 2 == 0 {
            rng.bytes(128)
        } else {
            mutate_bytes(rng, b"ref: refs/heads/main\n")
        };
        repo.store.write("HEAD", &bad_head);
        let bad_index = rng.bytes(256);
        repo.store.write("index", &bad_index);
        let _ = repo.head();
        let _ = repo.staged();
        let _ = repo.branches();
        let _ = Repo::open(repo.store);
    });

    // 5. Hex decoder (and round-trip)
    run_target("from_hex", seed ^ 0x05, budget, |rng, idx| {
        let id = rand_id(rng);
        let hex = to_hex(&id);
        assert_eq!(from_hex(&hex), Some(id));

        let input = if idx % 2 == 0 {
            rand_utf8_text(rng, 128)
        } else {
            mutate_text(rng, &hex)
        };
        let _ = from_hex(&input);
    });

    // 6. Diff and unified diff on random/mutated text
    run_target("diff_and_unified", seed ^ 0x06, budget, |rng, idx| {
        let (a, b) = if idx % 2 == 0 {
            (rng.bytes(256), rng.bytes(256))
        } else {
            let base = rand_lines_bytes(rng, 32);
            (mutate_bytes(rng, &base), mutate_bytes(rng, &base))
        };
        let edits = diff(&a, &b);
        assert_eq!(apply_edits(&a, &b, &edits), b);
        assert_eq!(lines(&a).concat(), a);
        let ctx = rng.below(8);
        let path = rand_ascii_ident(rng, 12);
        let _ = unified(&path, &a, &b, ctx);
    });

    // 7. 3-way merge on random/mutated text
    run_target("merge3", seed ^ 0x07, budget, |rng, idx| {
        let (base, ours, theirs) = if idx % 2 == 0 {
            (rng.bytes(256), rng.bytes(256), rng.bytes(256))
        } else {
            let base = rand_lines_bytes(rng, 32);
            (
                base.clone(),
                mutate_bytes(rng, &base),
                mutate_bytes(rng, &base),
            )
        };

        let same = merge3(&base, &ours, &ours);
        assert_eq!(same.conflicts, 0);
        assert_eq!(same.text, ours);

        let fast_theirs = merge3(&base, &base, &theirs);
        assert_eq!(fast_theirs.conflicts, 0);
        assert_eq!(fast_theirs.text, theirs);

        let fast_ours = merge3(&base, &ours, &base);
        assert_eq!(fast_ours.conflicts, 0);
        assert_eq!(fast_ours.text, ours);

        let _ = merge3(&base, &ours, &theirs);
    });
}
