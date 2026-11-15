//! Leader check for T25 (titan): boot transcript at several RAM sizes, step bound, decode cache off,
//! fresh-seed Sv39 shadow model with remap/unmap edge cases, page-boundary translation, frames never leaking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use assay::Rng;
use silicon::machine::{Config, Exit, Machine};
use titan::frames::{Error as FErr, FrameAllocator};
use titan::sv39::{Error, FramePool, PageTable, Pte, A, D, G, R, U, W, X};

const SEED: u64 = 0x01EA_DE25_0000;
const MIB: usize = 1024 * 1024;

fn kernel() -> &'static [u8] {
    static ELF: OnceLock<Vec<u8>> = OnceLock::new();
    ELF.get_or_init(|| {
        let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = root.join("target").join("titan-kernel");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = Command::new(cargo)
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &dir)
            .args([
                "build",
                "-p",
                "titan",
                "--bin",
                "kernel",
                "--release",
                "--target",
                "riscv64gc-unknown-none-elf",
            ])
            .status()
            .unwrap()
            .success();
        assert!(ok, "kernel build failed");
        std::fs::read(dir.join("riscv64gc-unknown-none-elf/release/kernel")).unwrap()
    })
}

fn hex_after(line: &str, key: &str) -> u64 {
    let s = line.split(key).nth(1).unwrap();
    let s = s.split(|c: char| !c.is_ascii_hexdigit()).next().unwrap();
    u64::from_str_radix(s, 16).unwrap()
}

/// Boot and check every transcript line exactly; returns the step count.
fn boot(ram: usize, decode_cache: bool) -> u64 {
    let img = chisel::elf::read(kernel()).unwrap();
    let mut m = Machine::new(Config {
        ram_size: ram,
        decode_cache,
        ..Config::default()
    });
    chisel::elf::load(&img, &mut m).unwrap();
    m.cpu.x[10] = 0;
    m.cpu.x[11] = ram as u64;
    let exit = m.run(50_000_000);
    let steps = m.board.mtime();
    let out = String::from_utf8(m.board.take_uart_output()).unwrap();
    assert_eq!(
        exit,
        Exit::Pass,
        "ram {ram} cache {decode_cache} steps {steps}:\n{out}"
    );
    let l: Vec<&str> = out.lines().collect();
    assert!(l.len() >= 11, "{out}"); // T26 appends process lines after the T25 boot lines
    let end = 0x8000_0000u64 + ram as u64;
    assert_eq!(l[0], "Titan v0.1");
    assert_eq!(
        l[1],
        format!("ram 0x80000000..0x{end:x} ({} MiB)", ram / MIB)
    );
    let hs = hex_after(l[2], "heap 0x");
    let he = hex_after(l[2], "..0x");
    assert_eq!(l[2], format!("heap 0x{hs:x}..0x{he:x}"));
    assert_eq!(
        hs % (2 * MIB as u64),
        0,
        "heap start not 2 MiB aligned: {}",
        l[2]
    );
    assert_eq!(he - hs, 4 * MIB as u64, "heap not 4 MiB: {}", l[2]);
    assert!(hs > 0x8000_0000 && he < end, "{}", l[2]);
    assert_eq!(l[3], format!("frames {} free", (end - he) / 4096));
    assert_eq!(l[4], "sv39 on");
    assert_eq!(l[5], "heap ok");
    let mut addrs = BTreeSet::new();
    for (i, (kind, code)) in [("load", 13), ("store", 15), ("fetch", 12)]
        .iter()
        .enumerate()
    {
        let line = l[6 + i];
        let a = hex_after(line, &format!("fault {kind} 0x"));
        assert_eq!(line, format!("fault {kind} 0x{a:x} scause {code}"));
        assert!(addrs.insert(a), "fault addresses must differ: {out}");
    }
    assert_eq!(l[9], "ticks ok");
    assert_eq!(l[10], "boot ok");
    steps
}

#[test]
fn transcript_at_many_ram_sizes_and_step_bound() {
    for mib in [16usize, 17, 31, 100, 128, 255, 256] {
        let steps = boot(mib * MIB, true);
        // 256 MiB took about 5.7M steps for the worker; allow 2x headroom, never near the 50M cap
        assert!(steps < 12_000_000, "{mib} MiB took {steps} steps");
    }
}

#[test]
fn boots_without_decode_cache() {
    // the uncached fetch path must agree: catches a missing sfence.vma after page-table edits
    boot(16 * MIB, false);
    boot(64 * MIB, false);
}

const LEAF_PERMS: [u64; 5] = [
    1 << 1,
    (1 << 1) | (1 << 2),
    (1 << 1) | (1 << 3),
    (1 << 1) | (1 << 2) | (1 << 3),
    1 << 3,
];

fn rand_flags(r: &mut Rng) -> Pte {
    let mut f = Pte::from_bits(*r.pick(&LEAF_PERMS));
    for extra in [U, G, A, D] {
        if r.chance(1, 2) {
            f |= extra;
        }
    }
    f
}

fn rand_va(r: &mut Rng) -> u64 {
    // small clustered pool so remaps, shared tables and empty-table reclaim all happen
    let tops = [
        0u64,
        0x0000_003F_C000_0000,
        0xFFFF_FFC0_0000_0000,
        0xFFFF_FFFF_C000_0000,
    ];
    let top = *r.pick(&tops);
    let mid = r.below(3) as u64 * 0x20_0000;
    let low = r.below(6) as u64 * 0x1000;
    let va = top + mid + low;
    if top == 0x0000_003F_C000_0000 && r.chance(1, 4) {
        0x0000_003F_FFFF_F000 // highest user page
    } else {
        va
    }
}

#[test]
fn sv39_shadow_model_fresh_seed() {
    let mut r = Rng::new(SEED);
    for case in 0..300 {
        let pool = FramePool::new(0x8040_0000, 64);
        let total = pool.total_count();
        let mut pt = PageTable::new(pool).unwrap();
        let mut shadow: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
        for _ in 0..200 {
            let va = rand_va(&mut r);
            match r.below(10) {
                0..=4 => {
                    let pa = 0x9000_0000 + r.below(4096) as u64 * 4096;
                    let f = rand_flags(&mut r);
                    match pt.map(va, pa, f) {
                        Ok(()) => {
                            shadow.insert(va, (pa, (f | Pte::V).bits()));
                        }
                        Err(Error::OutOfMemory) => {}
                        Err(e) => panic!("case {case}: map {va:#x} -> {e:?}"),
                    }
                }
                5..=7 => match (pt.unmap(va), shadow.remove(&va)) {
                    (Ok((pa, f)), Some((spa, sf))) => {
                        assert_eq!((pa, f.bits()), (spa, sf), "case {case}")
                    }
                    (Err(Error::NotMapped), None) => {}
                    (got, want) => panic!("case {case}: unmap {va:#x} got {got:?} shadow {want:?}"),
                },
                8 => {
                    // misaligned and non-canonical inputs are errors and change nothing
                    assert_eq!(
                        pt.map(va | 0x8, 0x9000_0000, R),
                        Err(Error::MisalignedAddress)
                    );
                    assert_eq!(pt.map(va, 0x9000_0010, R), Err(Error::MisalignedAddress));
                    assert_eq!(pt.unmap(va | 0x10), Err(Error::MisalignedAddress));
                    let bad = 0x0000_0040_0000_0000 | (va & 0x3F_FFFF_F000);
                    assert_eq!(pt.map(bad, 0x9000_0000, R), Err(Error::InvalidAddress));
                    assert_eq!(pt.translate(bad), None);
                }
                _ => {
                    let off = r.below(4096) as u64;
                    let got = pt.translate(va + off);
                    let want = shadow.get(&va).map(|&(pa, f)| (pa + off, f));
                    assert_eq!(
                        got.map(|(pa, f)| (pa, f.bits())),
                        want,
                        "case {case}: translate {:#x}",
                        va + off
                    );
                }
            }
        }
        for &va in shadow.clone().keys() {
            pt.unmap(va).unwrap();
        }
        let pool = pt.into_allocator();
        assert_eq!(
            pool.free_count(),
            total - 1,
            "case {case}: table frames leaked (root only should remain)"
        );
    }
}

#[test]
fn leaf_flags_must_be_a_valid_leaf() {
    // InvalidFlags is documented as "neither R nor X, or W without R": a V-only entry at level 0 is a
    // reserved pointer encoding that hardware faults on, and W-only is reserved
    let mut pt = PageTable::new(FramePool::new(0x8040_0000, 16)).unwrap();
    assert_eq!(
        pt.map(0x1000, 0x9000_0000, Pte::empty()),
        Err(Error::InvalidFlags)
    );
    assert_eq!(pt.map(0x1000, 0x9000_0000, W), Err(Error::InvalidFlags));
    assert_eq!(pt.map(0x1000, 0x9000_0000, W | X), Err(Error::InvalidFlags));
    assert_eq!(
        pt.map(0x1000, 0x9000_0000, U | A | D),
        Err(Error::InvalidFlags)
    );
    assert_eq!(pt.translate(0x1000), None);
    assert_eq!(
        pt.allocator().free_count(),
        15,
        "a rejected map must not keep table frames"
    );
}

#[test]
fn page_boundary_translation() {
    let mut pt = PageTable::new(FramePool::new(0x8040_0000, 16)).unwrap();
    let va = 0x0040_0000u64 - 0x1000; // last page below a 2 MiB (level-1) boundary
    pt.map(va, 0x9000_0000, R | W).unwrap();
    assert_eq!(pt.translate(va + 0xFFF).map(|t| t.0), Some(0x9000_0FFF));
    assert_eq!(
        pt.translate(va + 0x1000),
        None,
        "first byte of the next page is unmapped"
    );
    assert_eq!(
        pt.translate(va - 1),
        None,
        "last byte of the previous page is unmapped"
    );
    pt.map(va + 0x1000, 0x9100_0000, R).unwrap(); // crosses into a new level-0 table
    assert_eq!(pt.translate(va + 0x1000).map(|t| t.0), Some(0x9100_0000));
    let (_, f) = pt.translate(va + 0x1000).unwrap();
    assert!(!f.contains(W));
    pt.unmap(va).unwrap();
    pt.unmap(va + 0x1000).unwrap();
    assert_eq!(pt.allocator().free_count(), 15);
}

#[test]
fn frames_never_leak_fresh_seed() {
    let mut r = Rng::new(SEED ^ 0xF4A3);
    for case in 0..200 {
        let start = 0x8000_0000 + r.below(64) as u64 * 4096 + r.below(2) as u64 * 7;
        let n = 1 + r.below(300) as u64;
        let mut fa = FrameAllocator::new(start, start + n * 4096 + r.below(4096) as u64);
        let total = fa.total_count();
        let mut live: BTreeSet<u64> = BTreeSet::new();
        for _ in 0..600 {
            if r.chance(3, 5) {
                match fa.alloc() {
                    Some(pa) => {
                        assert_eq!(pa % 4096, 0);
                        assert!(
                            pa >= fa.start() && pa < fa.start() + total as u64 * 4096,
                            "case {case}"
                        );
                        assert!(
                            live.insert(pa),
                            "case {case}: frame {pa:#x} handed out twice"
                        );
                    }
                    None => assert_eq!(live.len(), total, "case {case}: None with free frames"),
                }
            } else if let Some(&pa) = live.iter().nth(r.below(live.len().max(1))) {
                fa.free(pa).unwrap();
                live.remove(&pa);
                assert_eq!(fa.free(pa), Err(FErr::DoubleFree(pa)), "case {case}");
            }
            assert_eq!(
                fa.free(fa.start() + 1),
                Err(FErr::Misaligned(fa.start() + 1))
            );
            let past = fa.start() + total as u64 * 4096;
            assert_eq!(fa.free(past), Err(FErr::OutOfRange(past)));
            assert_eq!(
                fa.free_count() + live.len(),
                total,
                "case {case}: count drift"
            );
            assert_eq!(fa.used_count(), live.len());
        }
        for pa in live {
            fa.free(pa).unwrap();
        }
        assert_eq!(fa.free_count(), total, "case {case}");
    }
}

// ---- T26 leader checks (2026-11-15) ----
use titan::elf;
use titan::uaccess::{check_user_range, USER_MAX, USER_MIN};

fn tiny_user_elf() -> Vec<u8> {
    let mut b = vec![0u8; 120 + 16];
    b[0..4].copy_from_slice(b"\x7fELF");
    b[4] = 2; // 64-bit
    b[5] = 1; // little endian
    b[6] = 1;
    b[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    b[18..20].copy_from_slice(&0xF3u16.to_le_bytes()); // RISC-V
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    b[24..32].copy_from_slice(&0x1_0078u64.to_le_bytes()); // entry
    b[32..40].copy_from_slice(&64u64.to_le_bytes()); // phoff
    b[52..54].copy_from_slice(&64u16.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&1u16.to_le_bytes());
    let p = 64;
    b[p..p + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    b[p + 4..p + 8].copy_from_slice(&5u32.to_le_bytes()); // R X
    b[p + 8..p + 16].copy_from_slice(&0u64.to_le_bytes()); // offset
    b[p + 16..p + 24].copy_from_slice(&0x1_0000u64.to_le_bytes()); // vaddr
    b[p + 24..p + 32].copy_from_slice(&0x1_0000u64.to_le_bytes());
    b[p + 32..p + 40].copy_from_slice(&136u64.to_le_bytes()); // filesz
    b[p + 40..p + 48].copy_from_slice(&0x2000u64.to_le_bytes()); // memsz
    b[p + 48..p + 56].copy_from_slice(&0x1000u64.to_le_bytes());
    b
}

fn assert_inside_user(img: &[u8], prog: &elf::Program) {
    assert!(
        (USER_MIN..USER_MAX).contains(&prog.entry),
        "entry {:#x}",
        prog.entry
    );
    for s in &prog.segments {
        let end = s
            .vaddr
            .checked_add(s.mem_size)
            .expect("segment end overflows");
        assert!(
            s.vaddr >= USER_MIN && end <= USER_MAX,
            "segment {:#x}+{:#x}",
            s.vaddr,
            s.mem_size
        );
        assert!(
            s.file.start <= s.file.end && s.file.end <= img.len(),
            "file range {:?}",
            s.file
        );
        assert!((s.file.end - s.file.start) as u64 <= s.mem_size);
    }
}

#[test]
fn elf_fuzz_fresh_seed_never_panics_and_stays_in_user_space() {
    let base = tiny_user_elf();
    let prog = elf::parse(&base).expect("leader's minimal user ELF must parse");
    assert_inside_user(&base, &prog);
    let mut r = Rng::new(SEED ^ 0x7126);
    let mut accepted = 0;
    for _ in 0..20_000 {
        let mut img = base.clone();
        for _ in 0..1 + r.below(8) {
            let i = r.below(img.len());
            img[i] = r.u64() as u8;
        }
        if r.below(8) == 0 {
            img.truncate(r.below(img.len() + 1));
        }
        if let Ok(p) = elf::parse(&img) {
            accepted += 1;
            assert_inside_user(&img, &p);
        }
    }
    assert!(accepted > 0, "fuzz never produced an accepted image");
}

#[test]
fn uaccess_edges_at_user_space_boundary() {
    assert!(check_user_range(USER_MIN, 1).is_ok());
    assert!(check_user_range(USER_MIN - 1, 1).is_err());
    assert!(check_user_range(USER_MAX - 1, 1).is_ok());
    assert!(check_user_range(USER_MAX - 1, 2).is_err());
    assert!(check_user_range(USER_MAX, 1).is_err());
    assert!(check_user_range(USER_MAX - 4096, 4096).is_ok());
    assert!(check_user_range(u64::MAX, 2).is_err());
    assert!(check_user_range(USER_MIN, u64::MAX).is_err());
    assert!(check_user_range(0, 1).is_err());
}
