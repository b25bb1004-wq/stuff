//! Leader check for T22 (chisel ELF). Copied to crates/chisel/tests/elf_leader.rs.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use chisel::elf::{self, ElfError, Image, Segment, PF_R, PF_W, PF_X};
use quartz::machine::{Config, Exit, Machine, RAM_BASE};
use std::process::Command;

const PROG: &[u8] = include_bytes!("fixtures/elf/prog.elf");
const PROG_TXT: &str = include_str!("fixtures/elf/prog.txt");
const COMMON: &str = include_str!("../../quartz/tests/fixtures/machine/common.inc");

fn hex(s: &str) -> u64 {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap()
}
fn u16_at(b: &[u8], o: usize) -> u64 {
    u64::from(u16::from_le_bytes(b[o..o + 2].try_into().unwrap()))
}
fn u32_at(b: &[u8], o: usize) -> u64 {
    u64::from(u32::from_le_bytes(b[o..o + 4].try_into().unwrap()))
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn cstr(b: &[u8], o: usize) -> String {
    let end = o + b[o..].iter().position(|&c| c == 0).unwrap();
    String::from_utf8(b[o..end].to_vec()).unwrap()
}

/// The leader's own parser: checks every field the brief fixes for `write`.
fn check_written(f: &[u8], img: &Image) {
    let n = img.segments.len();
    assert_eq!(
        &f[..16],
        &[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(u16_at(f, 16), 2, "e_type");
    assert_eq!(u16_at(f, 18), 243, "e_machine");
    assert_eq!(u32_at(f, 20), 1, "e_version");
    assert_eq!(u64_at(f, 24), img.entry, "e_entry");
    assert_eq!(u64_at(f, 32), 64, "e_phoff");
    let shoff = u64_at(f, 40) as usize;
    assert_eq!(u32_at(f, 48), 1, "e_flags");
    assert_eq!(u16_at(f, 52), 64, "e_ehsize");
    assert_eq!(u16_at(f, 54), 56, "e_phentsize");
    assert_eq!(u16_at(f, 56), n as u64, "e_phnum");
    assert_eq!(u16_at(f, 58), 64, "e_shentsize");
    assert_eq!(u16_at(f, 60), 4, "e_shnum");
    let shstrndx = u16_at(f, 62) as usize;
    for (i, s) in img.segments.iter().enumerate() {
        let p = 64 + 56 * i;
        assert_eq!(u32_at(f, p), 1, "p_type {i}");
        assert_eq!(u32_at(f, p + 4), u64::from(s.flags), "p_flags {i}");
        let off = u64_at(f, p + 8);
        assert_eq!(off % 4096, s.vaddr % 4096, "p_offset congruence {i}");
        assert_eq!(u64_at(f, p + 16), s.vaddr, "p_vaddr {i}");
        assert_eq!(u64_at(f, p + 24), s.vaddr, "p_paddr {i}");
        assert_eq!(u64_at(f, p + 32), s.data.len() as u64, "p_filesz {i}");
        assert_eq!(u64_at(f, p + 40), s.mem_size, "p_memsz {i}");
        assert_eq!(u64_at(f, p + 48), 4096, "p_align {i}");
        let off = off as usize;
        assert!(
            f[off..off + s.data.len()] == s.data[..],
            "segment {i} bytes"
        );
    }
    let sh = |i: usize, field: usize, wide: bool| {
        let o = shoff + 64 * i + field;
        if wide {
            u64_at(f, o)
        } else {
            u32_at(f, o)
        }
    };
    assert!(f[shoff..shoff + 64].iter().all(|&b| b == 0), "null section");
    let shstr = sh(shstrndx, 24, true) as usize;
    let name = |i: usize| cstr(f, shstr + sh(i, 0, false) as usize);
    assert_eq!(shstrndx, 1);
    assert_eq!(
        (name(1), name(2), name(3)),
        (".shstrtab".into(), ".symtab".into(), ".strtab".into())
    );
    assert_eq!(
        (sh(1, 4, false), sh(2, 4, false), sh(3, 4, false)),
        (3, 2, 3),
        "sh_type"
    );
    assert_eq!(sh(2, 56, true), 24, "symtab entsize");
    assert_eq!(sh(2, 48, true), 8, "symtab align");
    assert_eq!(sh(2, 40, false), 3, "symtab link");
    assert_eq!(sh(2, 44, false), 1, "symtab info");
    let (symoff, symsize) = (sh(2, 24, true) as usize, sh(2, 32, true) as usize);
    let stroff = sh(3, 24, true) as usize;
    assert_eq!(f[stroff], 0, ".strtab starts with NUL");
    assert_eq!(symsize, 24 * (img.symbols.len() + 1), "symtab size");
    assert!(
        f[symoff..symoff + 24].iter().all(|&b| b == 0),
        "null symbol"
    );
    for (k, (nm, val)) in img.symbols.iter().enumerate() {
        let e = symoff + 24 * (k + 1);
        assert_eq!(
            &cstr(f, stroff + u32_at(f, e) as usize),
            nm,
            "symbol {k} name"
        );
        assert_eq!(
            (f[e + 4], f[e + 5], u16_at(f, e + 6)),
            (0x10, 0, 0xfff1),
            "symbol {k}"
        );
        assert_eq!(
            (u64_at(f, e + 8), u64_at(f, e + 16)),
            (*val, 0),
            "symbol {k} value"
        );
    }
    let data: usize = img.segments.iter().map(|s| s.data.len()).sum();
    let names: usize = img.symbols.iter().map(|(s, _)| s.len() + 1).sum();
    let cap = 64 + 56 * n + 4096 * (n + 1) + data + 256 + 24 * (img.symbols.len() + 1) + names + 32;
    assert!(f.len() <= cap, "file {} bytes, cap {cap}", f.len());
}

fn random_image(r: &mut Rng) -> Image {
    let mut segments = Vec::new();
    let mut at = 0x8000_0000u64 + r.below(4) as u64 * 0x1000;
    for _ in 0..r.below(5) {
        at += r.below(3) as u64 * 0x1000 + r.below(4096) as u64;
        let data = r.bytes(9000);
        let mem_size = data.len() as u64
            + if r.chance(1, 3) {
                r.below(20000) as u64
            } else {
                0
            };
        let flags = [PF_R | PF_X, PF_R, PF_R | PF_W, PF_R | PF_W | PF_X][r.below(4)];
        segments.push(Segment {
            vaddr: at,
            flags,
            data,
            mem_size,
        });
        at += mem_size;
    }
    if r.chance(1, 4) {
        segments.reverse();
    }
    let symbols = (0..r.below(12))
        .map(|i| {
            let name = format!(
                "{}{i}",
                ["_start", "main.loop", "$x", "L", "a_b$c"][r.below(5)]
            );
            (name, r.u64())
        })
        .collect();
    Image {
        entry: r.u64(),
        segments,
        symbols,
    }
}

#[test]
fn leader_writer_fields_and_round_trip() {
    let mut r = Rng::new(0x22_e1f0_0001);
    for case in 0..300 {
        let img = random_image(&mut r);
        let f = elf::write(&img);
        check_written(&f, &img);
        assert_eq!(elf::read(&f), Ok(img), "case {case}: round trip");
    }
}

/// (vaddr, offset, filesz, memsz, flags) per LOAD line.
type Load = (u64, u64, u64, u64, u32);

fn expected_prog() -> (u64, Vec<Load>, Vec<(String, u64)>) {
    let (mut entry, mut loads, mut syms) = (0, Vec::new(), Vec::new());
    for l in PROG_TXT.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        match w[0] {
            "entry" => entry = hex(w[1]),
            "load" => loads.push((
                hex(w[1]),
                hex(w[2]),
                hex(w[3]),
                hex(w[4]),
                w[5].parse().unwrap(),
            )),
            "sym" => syms.push((w[1].to_string(), hex(w[2]))),
            _ => panic!("prog.txt: {l}"),
        }
    }
    (entry, loads, syms)
}

#[test]
fn leader_reads_lld_output() {
    let (entry, loads, syms) = expected_prog();
    let img = elf::read(PROG).unwrap();
    assert_eq!(img.entry, entry);
    assert_eq!(img.segments.len(), loads.len());
    for (s, &(va, off, fs, ms, fl)) in img.segments.iter().zip(&loads) {
        assert_eq!(
            (s.vaddr, s.mem_size, s.flags),
            (va, ms, fl),
            "segment {va:#x}"
        );
        assert!(
            s.data[..] == PROG[off as usize..(off + fs) as usize],
            "segment {va:#x} bytes"
        );
    }
    assert_eq!(img.symbols, syms);
}

fn machine() -> Machine {
    Machine::new(Config {
        ram_size: 1 << 20,
        ..Config::default()
    })
}

#[test]
fn leader_lld_program_runs_with_dirty_ram() {
    let img = elf::read(PROG).unwrap();
    let mut m = machine();
    m.load(RAM_BASE, &[0xa5; 0x3000]).unwrap();
    m.cpu.pc = 0x1234;
    elf::load(&img, &mut m).unwrap();
    assert_eq!(m.cpu.pc, img.entry);
    assert_eq!(m.run(100_000), Exit::Pass);
    assert_eq!(m.board.take_uart_output(), b"elf ok\n");
    let bad = Image {
        entry: 0,
        segments: vec![Segment {
            vaddr: 0x10,
            flags: PF_R,
            data: vec![1],
            mem_size: 1,
        }],
        symbols: vec![],
    };
    assert!(
        elf::load(&bad, &mut machine()).is_err(),
        "load outside RAM must fault"
    );
}

#[test]
fn leader_llvm_readelf_accepts_our_files() {
    let mut r = Rng::new(0x22_11e1_0002);
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for case in 0..20 {
        let mut img = random_image(&mut r);
        img.segments.retain(|s| !s.data.is_empty());
        let path = dir.join(format!("t22-{}-{case}.elf", std::process::id()));
        std::fs::write(&path, elf::write(&img)).unwrap();
        let out = match Command::new("llvm-readelf")
            .args(["-h", "-l", "-s", "-W"])
            .arg(&path)
            .output()
        {
            Ok(o) => o,
            Err(_) => {
                eprintln!("llvm-readelf not installed; skipping");
                return;
            }
        };
        let _ = std::fs::remove_file(&path);
        let (so, se) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(
            out.status.success() && se.trim().is_empty(),
            "case {case}: {se}"
        );
        let entry = so
            .lines()
            .find_map(|l| l.trim().strip_prefix("Entry point address:"));
        assert_eq!(
            entry.map(|e| hex(e.trim())),
            Some(img.entry),
            "case {case}: entry
{so}"
        );
        assert!(so.contains("RISC-V"), "case {case}: machine");
        for s in &img.segments {
            let line = so
                .lines()
                .find(|l| {
                    l.trim_start().starts_with("LOAD") && l.contains(&format!("{:#018x}", s.vaddr))
                })
                .unwrap_or_else(|| panic!("case {case}: no LOAD for {:#x}\n{so}", s.vaddr));
            assert!(
                line.contains(&format!("{:#08x}", s.data.len())),
                "case {case}: filesz {line}"
            );
            assert!(
                line.contains(&format!("{:#08x}", s.mem_size)),
                "case {case}: memsz {line}"
            );
        }
        for (nm, val) in &img.symbols {
            assert!(
                so.lines()
                    .any(|l| l.ends_with(&format!(" {nm}")) && l.contains(&format!("{val:016x}"))),
                "case {case}: symbol {nm}\n{so}"
            );
        }
    }
}

#[test]
fn leader_read_errors() {
    let good = elf::write(&Image {
        entry: 0x8000_0000,
        segments: vec![
            Segment {
                vaddr: 0x8000_0000,
                flags: PF_R | PF_X,
                data: vec![0x13, 0, 0, 0],
                mem_size: 4,
            },
            Segment {
                vaddr: 0x8000_2000,
                flags: PF_R | PF_W,
                data: vec![7; 16],
                mem_size: 64,
            },
        ],
        symbols: vec![("_start".into(), 0x8000_0000)],
    });
    assert!(elf::read(&good).is_ok());
    let put = |o: usize, v: &[u8]| {
        let mut b = good.clone();
        b[o..o + v.len()].copy_from_slice(v);
        b
    };
    let ph1 = 64 + 56; // second program header
    let unsupported = |b: Vec<u8>, what: &str| {
        assert!(
            matches!(elf::read(&b), Err(ElfError::Unsupported(_))),
            "{what}"
        );
    };
    let bad_segment = |b: Vec<u8>, what: &str| {
        assert!(
            matches!(elf::read(&b), Err(ElfError::BadSegment(_))),
            "{what}"
        );
    };
    assert_eq!(elf::read(&good[..63]), Err(ElfError::Truncated));
    assert_eq!(elf::read(b""), Err(ElfError::Truncated));
    assert_eq!(elf::read(&put(1, b"X")), Err(ElfError::BadMagic));
    unsupported(put(4, &[1]), "class 32");
    unsupported(put(5, &[2]), "big endian");
    unsupported(put(6, &[0]), "ident version");
    unsupported(put(16, &3u16.to_le_bytes()), "ET_DYN");
    unsupported(put(18, &62u16.to_le_bytes()), "x86-64");
    unsupported(put(54, &32u16.to_le_bytes()), "phentsize");
    // Program headers past the end.
    assert_eq!(
        elf::read(&put(32, &(good.len() as u64 - 60).to_le_bytes())),
        Err(ElfError::Truncated)
    );
    // Segment bytes past the end.
    assert_eq!(
        elf::read(&put(ph1 + 8, &(good.len() as u64).to_le_bytes())),
        Err(ElfError::Truncated)
    );
    assert_eq!(
        elf::read(&put(ph1 + 32, &u64::MAX.to_le_bytes())),
        Err(ElfError::Truncated)
    );
    // Section headers past the end.
    assert_eq!(
        elf::read(&put(40, &(good.len() as u64 - 10).to_le_bytes())),
        Err(ElfError::Truncated)
    );
    bad_segment(put(ph1 + 40, &8u64.to_le_bytes()), "filesz > memsz");
    bad_segment(put(ph1 + 16, &0x8000_0002u64.to_le_bytes()), "overlap");
    bad_segment(
        {
            let mut b = put(ph1 + 16, &(u64::MAX - 8).to_le_bytes());
            b[ph1 + 40..ph1 + 48].copy_from_slice(&64u64.to_le_bytes());
            b
        },
        "vaddr + memsz overflow",
    );
    // A non-LOAD header is ignored.
    let note = put(ph1, &4u32.to_le_bytes());
    assert_eq!(elf::read(&note).unwrap().segments.len(), 1);
    // Truncating anywhere never panics; inside the headers it is an error.
    for cut in 0..good.len() {
        let got = elf::read(&good[..cut]);
        assert!(cut >= 64 + 2 * 56 || got.is_err(), "cut {cut}");
    }
}

#[test]
fn leader_chisel_programs_run_from_elf() {
    let fx: [(&str, &str, &[u8]); 2] = [
        (
            "hello",
            include_str!("../../quartz/tests/fixtures/machine/hello.S"),
            include_bytes!("../../quartz/tests/fixtures/machine/hello.bin"),
        ),
        (
            "timer",
            include_str!("../../quartz/tests/fixtures/machine/timer.S"),
            include_bytes!("../../quartz/tests/fixtures/machine/timer.bin"),
        ),
    ];
    for (name, src, bin) in fx {
        let mut m = machine();
        m.load(RAM_BASE, bin).unwrap();
        let want = (m.run(500_000), m.board.take_uart_output());
        // Same code, placed 64 KiB up with a trampoline at RAM_BASE that is never used.
        let base = RAM_BASE + 0x1_0000;
        let out = chisel::assemble(&format!("{COMMON}\n{src}"), base).unwrap();
        let img = Image {
            entry: base,
            segments: vec![
                Segment {
                    vaddr: RAM_BASE,
                    flags: PF_R | PF_X,
                    data: chisel::assemble("1: j 1b\n", RAM_BASE).unwrap().bytes,
                    mem_size: 4,
                },
                Segment {
                    vaddr: base,
                    flags: PF_R | PF_X,
                    data: out.bytes,
                    mem_size: 0x1000,
                },
            ],
            symbols: out.symbols,
        };
        let back = elf::read(&elf::write(&img)).unwrap();
        assert_eq!(back, img, "{name}: round trip");
        let mut m = machine();
        elf::load(&back, &mut m).unwrap();
        let got = (m.run(500_000), m.board.take_uart_output());
        assert_eq!(got, want, "{name}");
    }
}
