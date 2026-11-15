//! ELF64 tests for chisel.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use assay::{forall, Rng};
use chisel::assemble;
use chisel::elf::{load, read, write, ElfError, Image, Segment, PF_R, PF_W, PF_X};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::Fault;

fn u16_le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn u64_le(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        b[off],
        b[off + 1],
        b[off + 2],
        b[off + 3],
        b[off + 4],
        b[off + 5],
        b[off + 6],
        b[off + 7],
    ])
}

fn cstr(tab: &[u8], off: usize) -> &str {
    let tail = &tab[off..];
    let nul = tail.iter().position(|&b| b == 0).unwrap();
    std::str::from_utf8(&tail[..nul]).unwrap()
}

#[test]
fn header_and_table_fields() {
    let img = Image {
        entry: 0x8000_0123,
        segments: vec![
            Segment {
                vaddr: 0x8000_0123,
                flags: PF_R | PF_X,
                data: vec![0x13, 0x00, 0x00, 0x00],
                mem_size: 4,
            },
            Segment {
                vaddr: 0x8000_2008,
                flags: PF_R | PF_W,
                data: vec![1, 2, 3],
                mem_size: 64,
            },
        ],
        symbols: vec![
            ("_start".into(), 0x8000_0123),
            ("bss_var".into(), 0x8000_2010),
        ],
    };

    let bytes = write(&img).unwrap();
    assert_eq!(&bytes[0..4], b"\x7fELF");
    assert_eq!(bytes[4], 2);
    assert_eq!(bytes[5], 1);
    assert_eq!(bytes[6], 1);
    assert_eq!(bytes[7], 0);
    assert_eq!(bytes[8], 0);
    assert_eq!(&bytes[9..16], &[0u8; 7]);
    assert_eq!(u16_le(&bytes, 16), 2);
    assert_eq!(u16_le(&bytes, 18), 243);
    assert_eq!(u32_le(&bytes, 20), 1);
    assert_eq!(u64_le(&bytes, 24), img.entry);
    assert_eq!(u64_le(&bytes, 32), 64);
    let e_shoff = u64_le(&bytes, 40) as usize;
    assert_eq!(u32_le(&bytes, 48), 1);
    assert_eq!(u16_le(&bytes, 52), 64);
    assert_eq!(u16_le(&bytes, 54), 56);
    assert_eq!(u16_le(&bytes, 56), 2);
    assert_eq!(u16_le(&bytes, 58), 64);
    assert_eq!(u16_le(&bytes, 60), 4);
    let e_shstrndx = u16_le(&bytes, 62) as usize;
    assert_eq!(e_shstrndx, 1);

    for (i, seg) in img.segments.iter().enumerate() {
        let ph = 64 + i * 56;
        assert_eq!(u32_le(&bytes, ph), 1);
        assert_eq!(u32_le(&bytes, ph + 4), seg.flags);
        let p_off = u64_le(&bytes, ph + 8);
        let p_vaddr = u64_le(&bytes, ph + 16);
        let p_paddr = u64_le(&bytes, ph + 24);
        let p_filesz = u64_le(&bytes, ph + 32);
        let p_memsz = u64_le(&bytes, ph + 40);
        let p_align = u64_le(&bytes, ph + 48);
        assert_eq!(p_vaddr, seg.vaddr);
        assert_eq!(p_paddr, seg.vaddr);
        assert_eq!(p_off % 4096, seg.vaddr % 4096);
        assert_eq!(p_filesz, seg.data.len() as u64);
        assert_eq!(p_memsz, seg.mem_size);
        assert_eq!(p_align, 4096);
        assert_eq!(
            &bytes[p_off as usize..p_off as usize + seg.data.len()],
            seg.data.as_slice()
        );
    }

    // Sections: 0=null, 1=.shstrtab, 2=.symtab, 3=.strtab
    assert_eq!(&bytes[e_shoff..e_shoff + 64], &[0u8; 64]);
    let sh1 = e_shoff + 64;
    let sh2 = e_shoff + 128;
    let sh3 = e_shoff + 192;

    assert_eq!(u32_le(&bytes, sh1 + 4), 3);
    let shstrtab_off = u64_le(&bytes, sh1 + 24) as usize;
    let shstrtab_sz = u64_le(&bytes, sh1 + 32) as usize;
    let shstrtab = &bytes[shstrtab_off..shstrtab_off + shstrtab_sz];
    assert_eq!(cstr(shstrtab, u32_le(&bytes, sh1) as usize), ".shstrtab");
    assert_eq!(cstr(shstrtab, u32_le(&bytes, sh2) as usize), ".symtab");
    assert_eq!(cstr(shstrtab, u32_le(&bytes, sh3) as usize), ".strtab");

    assert_eq!(u32_le(&bytes, sh2 + 4), 2);
    assert_eq!(u32_le(&bytes, sh2 + 40), 3);
    assert_eq!(u32_le(&bytes, sh2 + 44), 1);
    assert_eq!(u64_le(&bytes, sh2 + 48), 8);
    assert_eq!(u64_le(&bytes, sh2 + 56), 24);

    assert_eq!(u32_le(&bytes, sh3 + 4), 3);
    let strtab_off = u64_le(&bytes, sh3 + 24) as usize;
    let strtab_sz = u64_le(&bytes, sh3 + 32) as usize;
    let strtab = &bytes[strtab_off..strtab_off + strtab_sz];
    assert_eq!(strtab[0], 0);

    let symtab_off = u64_le(&bytes, sh2 + 24) as usize;
    let symtab_sz = u64_le(&bytes, sh2 + 32) as usize;
    assert_eq!(symtab_sz, 24 * 3);
    assert_eq!(&bytes[symtab_off..symtab_off + 24], &[0u8; 24]);

    for (i, (name, val)) in img.symbols.iter().enumerate() {
        let s = symtab_off + (i + 1) * 24;
        let st_name = u32_le(&bytes, s) as usize;
        assert_eq!(cstr(strtab, st_name), name);
        assert_eq!(bytes[s + 4], 0x10);
        assert_eq!(bytes[s + 5], 0);
        assert_eq!(u16_le(&bytes, s + 6), 0xfff1);
        assert_eq!(u64_le(&bytes, s + 8), *val);
        assert_eq!(u64_le(&bytes, s + 16), 0);
    }

    assert_eq!(read(&bytes), Ok(img));
}

#[test]
fn roundtrip_random_images() {
    forall(64, 0x5EED_0022, |rng: &mut Rng| {
        let n_seg = rng.range(0, 6) as usize;
        let mut cursor = 0x8000_0000u64;
        let mut segments = Vec::with_capacity(n_seg);
        let mut total_data = 0usize;

        for _ in 0..n_seg {
            cursor += rng.range(0, 0x2000);
            let data_len = rng.range(0, 256) as usize;
            let extra = rng.range(0, 256);
            let mem_size = data_len as u64 + extra;
            let mut data = vec![0u8; data_len];
            rng.fill(&mut data);
            let flags = (rng.u64() as u32) & 7;
            segments.push(Segment {
                vaddr: cursor,
                flags,
                data,
                mem_size,
            });
            total_data += data_len;
            cursor += mem_size + 1;
        }

        let k_sym = rng.range(0, 12) as usize;
        let mut symbols = Vec::with_capacity(k_sym);
        let mut total_sym_bytes = 0usize;
        for i in 0..k_sym {
            let name = format!("sym_{i}_{}", rng.u64() & 0xffff);
            total_sym_bytes += name.len() + 1;
            let addr = rng.u64();
            symbols.push((name, addr));
        }

        let img = Image {
            entry: rng.u64(),
            segments,
            symbols,
        };
        let bytes = write(&img).unwrap();
        let max_len = 64
            + 56 * n_seg
            + 4096 * (n_seg + 1)
            + total_data
            + 256
            + 24 * (k_sym + 1)
            + total_sym_bytes
            + 32;
        assert!(
            bytes.len() <= max_len,
            "file len {} > max_len {}",
            bytes.len(),
            max_len
        );
        assert_eq!(read(&bytes), Ok(img));
    });
}

#[test]
fn read_errors() {
    let base_img = Image {
        entry: 0x8000_0000,
        segments: vec![Segment {
            vaddr: 0x8000_0000,
            flags: PF_R | PF_X,
            data: vec![1, 2, 3, 4],
            mem_size: 8,
        }],
        symbols: vec![("main".into(), 0x8000_0000)],
    };
    let good = write(&base_img).unwrap();

    // < 64 bytes -> Truncated (even with bad magic)
    assert_eq!(read(&good[..63]), Err(ElfError::Truncated));
    assert_eq!(read(&[0u8; 10]), Err(ElfError::Truncated));

    // Bad magic
    let mut b = good.clone();
    b[0] = 0;
    assert_eq!(read(&b), Err(ElfError::BadMagic));

    // Unsupported header fields
    for &(off, val) in &[(4usize, 1u8), (5, 2), (6, 2)] {
        let mut b = good.clone();
        b[off] = val;
        assert!(matches!(read(&b), Err(ElfError::Unsupported(_))));
    }
    for &(off, val) in &[(16usize, 3u16), (18, 62), (54, 52)] {
        let mut b = good.clone();
        b[off..off + 2].copy_from_slice(&val.to_le_bytes());
        assert!(matches!(read(&b), Err(ElfError::Unsupported(_))));
    }

    // Program header out of file -> Truncated
    let mut b = good.clone();
    b[56..58].copy_from_slice(&1000u16.to_le_bytes());
    assert_eq!(read(&b), Err(ElfError::Truncated));

    // Segment bytes out of file -> Truncated
    let mut b = good.clone();
    b[64 + 32..64 + 40].copy_from_slice(&1_000_000u64.to_le_bytes());
    assert_eq!(read(&b), Err(ElfError::Truncated));

    // Section header out of file -> Truncated
    let mut b = good.clone();
    b[60..62].copy_from_slice(&1000u16.to_le_bytes());
    assert_eq!(read(&b), Err(ElfError::Truncated));

    // Symbol table out of file -> Truncated
    let e_shoff = u64_le(&good, 40) as usize;
    let mut b = good.clone();
    let sym_sh = e_shoff + 2 * 64;
    b[sym_sh + 32..sym_sh + 40].copy_from_slice(&1_000_000u64.to_le_bytes());
    assert_eq!(read(&b), Err(ElfError::Truncated));

    // String table out of file -> Truncated
    let mut b = good.clone();
    let str_sh = e_shoff + 3 * 64;
    b[str_sh + 32..str_sh + 40].copy_from_slice(&1_000_000u64.to_le_bytes());
    assert_eq!(read(&b), Err(ElfError::Truncated));

    // Symbol st_name out of strtab -> Truncated
    let symtab_off = u64_le(&good, sym_sh + 24) as usize;
    let mut b = good.clone();
    b[symtab_off + 24..symtab_off + 28].copy_from_slice(&9999u32.to_le_bytes());
    assert_eq!(read(&b), Err(ElfError::Truncated));

    // p_filesz > p_memsz -> BadSegment
    let mut b = good.clone();
    b[64 + 40..64 + 48].copy_from_slice(&2u64.to_le_bytes());
    assert!(matches!(read(&b), Err(ElfError::BadSegment(_))));

    // vaddr + memsz overflow -> BadSegment
    let mut b = good.clone();
    b[64 + 16..64 + 24].copy_from_slice(&(u64::MAX - 2).to_le_bytes());
    assert!(matches!(read(&b), Err(ElfError::BadSegment(_))));

    // Overlapping PT_LOAD -> BadSegment
    let mut b = write(&Image {
        entry: 0x8000_0000,
        segments: vec![
            Segment {
                vaddr: 0x8000_0000,
                flags: PF_R,
                data: vec![1, 2],
                mem_size: 16,
            },
            Segment {
                vaddr: 0x8000_1000,
                flags: PF_R,
                data: vec![3, 4],
                mem_size: 16,
            },
        ],
        symbols: vec![],
    })
    .unwrap();
    b[64 + 56 + 16..64 + 56 + 24].copy_from_slice(&0x8000_0008u64.to_le_bytes());
    assert!(matches!(read(&b), Err(ElfError::BadSegment(_))));
}

#[test]
fn lld_fixture_matches_prog_txt_and_runs() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/elf");
    let elf_bytes = std::fs::read(dir.join("prog.elf")).unwrap();
    let txt = std::fs::read_to_string(dir.join("prog.txt")).unwrap();

    let img = read(&elf_bytes).unwrap();
    let mut lines = Vec::new();
    lines.push(format!("entry {:#x}", img.entry));
    for seg in &img.segments {
        lines.push(format!(
            "load {:#018x} {:#08x} {:#08x}",
            seg.vaddr,
            seg.data.len(),
            seg.mem_size
        ));
    }
    for (name, val) in &img.symbols {
        lines.push(format!("sym {name} {val:#018x}"));
    }

    let mut expected_lines = Vec::new();
    for line in txt.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["entry", addr] => expected_lines.push(format!("entry {addr}")),
            ["load", vaddr, _off, filesz, memsz, flags] => {
                let f = flags.parse::<u32>().unwrap();
                let seg_idx = expected_lines
                    .iter()
                    .filter(|l| l.starts_with("load "))
                    .count();
                assert_eq!(img.segments[seg_idx].flags, f);
                expected_lines.push(format!("load {vaddr} {filesz} {memsz}"));
            }
            ["sym", name, val] => expected_lines.push(format!("sym {name} {val}")),
            other => panic!("unexpected line in prog.txt: {other:?}"),
        }
    }
    assert_eq!(lines, expected_lines);

    // Dirty RAM under .bss before loading to verify load zeroes [vaddr+data.len(), vaddr+mem_size)
    let mut m = Machine::new(Config::default());
    let dirty = vec![0xAAu8; 0x2000];
    m.load(RAM_BASE, &dirty).unwrap();
    load(&img, &mut m).unwrap();
    assert_eq!(m.cpu.pc, img.entry);
    assert_eq!(m.run(10_000), Exit::Pass);
    assert_eq!(m.board.take_uart_output(), b"elf ok\n");
}

#[test]
fn assemble_write_read_and_run_on_quartz() {
    let src = r#"
        .text
        .globl _start
    _start:
        li t0, 0x10000000
        li t1, 'H'
        sb t1, 0(t0)
        li t1, 'i'
        sb t1, 0(t0)
        li t1, '\n'
        sb t1, 0(t0)
        li t2, 0x00100000
        li t3, 0x5555
        sw t3, 0(t2)
    "#;
    let entry = RAM_BASE + 0x100;
    let out = assemble(src, entry).unwrap();
    let img = Image {
        entry,
        segments: vec![Segment {
            vaddr: entry,
            flags: PF_R | PF_X,
            data: out.bytes,
            mem_size: 64,
        }],
        symbols: out.symbols,
    };
    let elf_bytes = write(&img).unwrap();
    let parsed = read(&elf_bytes).unwrap();
    assert_eq!(parsed, img);

    let mut m = Machine::new(Config::default());
    load(&parsed, &mut m).unwrap();
    assert_eq!(m.run(1_000), Exit::Pass);
    assert_eq!(m.board.take_uart_output(), b"Hi\n");

    // Out-of-bounds segment returns Fault
    let bad = Image {
        entry: RAM_BASE,
        segments: vec![Segment {
            vaddr: 0x1000,
            flags: PF_R,
            data: vec![1, 2],
            mem_size: 4,
        }],
        symbols: vec![],
    };
    assert!(matches!(load(&bad, &mut m), Err(Fault::Unmapped(0x1000))));
}
