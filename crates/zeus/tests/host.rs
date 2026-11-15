//! Host unit tests for `zeus` (boot table layout, `build_machine` validation, `split_console`).
#![allow(clippy::unwrap_used)]

use chisel::elf::{Image as ElfImage, Segment, PF_R, PF_W, PF_X};
use silicon::Bus;
use zeus::{build_machine, split_console, Guest, Image};

const MIB: u64 = 1024 * 1024;

fn dummy_zeus_elf() -> Vec<u8> {
    chisel::elf::write(&ElfImage {
        entry: 0x8000_0000,
        segments: vec![Segment {
            vaddr: 0x8000_0000,
            flags: PF_R | PF_X,
            data: vec![0x13, 0x00, 0x00, 0x00],
            mem_size: 4,
        }],
        symbols: Vec::new(),
    })
}

fn dummy_guest_elf(vaddr: u64, data: Vec<u8>, mem_size: u64, entry: u64) -> Vec<u8> {
    chisel::elf::write(&ElfImage {
        entry,
        segments: vec![Segment {
            vaddr,
            flags: PF_R | PF_W | PF_X,
            data,
            mem_size,
        }],
        symbols: Vec::new(),
    })
}

fn read_bytes(board: &mut impl Bus, addr: u64, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| board.read(addr + i as u64, 1).unwrap() as u8)
        .collect()
}

fn read_u64(board: &mut impl Bus, addr: u64) -> u64 {
    board.read(addr, 8).unwrap()
}

#[test]
fn boot_table_and_guest_placement() {
    let zeus_elf = dummy_zeus_elf();
    let raw_bytes = vec![0xde, 0xad, 0xbe, 0xef];
    let elf_bytes = dummy_guest_elf(0x8000_1000, vec![0x11, 0x22, 0x33, 0x44], 64, 0x8000_1004);

    let guests = vec![
        Guest {
            name: "alpha-1".into(),
            image: Image::Raw {
                bytes: raw_bytes.clone(),
                gpa: 0x8000_0200,
            },
            size: 2 * MIB,
            a1: 0x1234,
        },
        Guest {
            name: "beta".into(),
            image: Image::Elf(elf_bytes),
            size: 4 * MIB,
            a1: 4 * MIB,
        },
    ];

    let mut m = build_machine(&zeus_elf, &guests).unwrap();
    assert_eq!(m.cpu.pc, 0x8000_0000);

    // Boot table header at 0x8040_0000
    assert_eq!(&read_bytes(&mut m.board, 0x8040_0000, 8), b"ZEUSBOOT");
    assert_eq!(read_u64(&mut m.board, 0x8040_0008), 2);

    // Entry 0 (offset 16)
    let mut expected_name0 = [0u8; 16];
    expected_name0[..7].copy_from_slice(b"alpha-1");
    assert_eq!(read_bytes(&mut m.board, 0x8040_0010, 16), expected_name0);
    assert_eq!(read_u64(&mut m.board, 0x8040_0020), 0x8100_0000);
    assert_eq!(read_u64(&mut m.board, 0x8040_0028), 2 * MIB);
    assert_eq!(read_u64(&mut m.board, 0x8040_0030), 0x8000_0200);
    assert_eq!(read_u64(&mut m.board, 0x8040_0038), 0x1234);

    // Entry 1 (offset 64)
    let mut expected_name1 = [0u8; 16];
    expected_name1[..4].copy_from_slice(b"beta");
    assert_eq!(read_bytes(&mut m.board, 0x8040_0040, 16), expected_name1);
    assert_eq!(read_u64(&mut m.board, 0x8040_0050), 0x8200_0000);
    assert_eq!(read_u64(&mut m.board, 0x8040_0058), 4 * MIB);
    assert_eq!(read_u64(&mut m.board, 0x8040_0060), 0x8000_1004);
    assert_eq!(read_u64(&mut m.board, 0x8040_0068), 4 * MIB);

    // Guest bytes placed at slot + (gpa - 0x8000_0000)
    assert_eq!(read_bytes(&mut m.board, 0x8100_0200, 4), raw_bytes);
    assert_eq!(
        read_bytes(&mut m.board, 0x8200_1000, 4),
        vec![0x11, 0x22, 0x33, 0x44]
    );
}

#[test]
fn build_machine_rejects_invalid_guest_counts() {
    let zeus_elf = dummy_zeus_elf();
    assert!(build_machine(&zeus_elf, &[]).is_err());

    let one = Guest {
        name: "g".into(),
        image: Image::Raw {
            bytes: vec![0; 4],
            gpa: 0x8000_0000,
        },
        size: 2 * MIB,
        a1: 0,
    };
    let four = vec![one.clone(), one.clone(), one.clone(), one];
    assert!(build_machine(&zeus_elf, &four).is_err());
}

#[test]
fn build_machine_rejects_bad_names() {
    let zeus_elf = dummy_zeus_elf();
    for bad in ["", "0123456789abcdef0", "Titan", "my_vm", "a b", "a\0b"] {
        let g = Guest {
            name: bad.into(),
            image: Image::Raw {
                bytes: vec![0; 4],
                gpa: 0x8000_0000,
            },
            size: 2 * MIB,
            a1: 0,
        };
        assert!(build_machine(&zeus_elf, &[g]).is_err(), "accepted {bad:?}");
    }
    // Exactly 16 valid chars is allowed.
    let ok = Guest {
        name: "0123456789abcdef".into(),
        image: Image::Raw {
            bytes: vec![0; 4],
            gpa: 0x8000_0000,
        },
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(&zeus_elf, &[ok]).is_ok());
}

#[test]
fn build_machine_rejects_bad_sizes() {
    let zeus_elf = dummy_zeus_elf();
    for bad in [0, 4096, 3 * MIB, 18 * MIB, 32 * MIB] {
        let g = Guest {
            name: "vm".into(),
            image: Image::Raw {
                bytes: vec![0; 4],
                gpa: 0x8000_0000,
            },
            size: bad,
            a1: 0,
        };
        assert!(
            build_machine(&zeus_elf, &[g]).is_err(),
            "accepted size {bad}"
        );
    }
}

#[test]
fn build_machine_rejects_out_of_range_segments_and_bad_elfs() {
    let zeus_elf = dummy_zeus_elf();

    // Raw below RAM_BASE
    let g1 = Guest {
        name: "vm".into(),
        image: Image::Raw {
            bytes: vec![0; 4],
            gpa: 0x7fff_fffc,
        },
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(&zeus_elf, &[g1]).is_err());

    // Raw past end of guest RAM
    let g2 = Guest {
        name: "vm".into(),
        image: Image::Raw {
            bytes: vec![0; 8],
            gpa: 0x8000_0000 + 2 * MIB - 4,
        },
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(&zeus_elf, &[g2]).is_err());

    // ELF segment below RAM_BASE
    let g3 = Guest {
        name: "vm".into(),
        image: Image::Elf(dummy_guest_elf(0x7000_0000, vec![0; 4], 4, 0x8000_0000)),
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(&zeus_elf, &[g3]).is_err());

    // ELF segment BSS extending past guest RAM
    let g4 = Guest {
        name: "vm".into(),
        image: Image::Elf(dummy_guest_elf(0x801f_f000, vec![0; 4], 8192, 0x801f_f000)),
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(&zeus_elf, &[g4]).is_err());

    // Bad guest ELF
    let g5 = Guest {
        name: "vm".into(),
        image: Image::Elf(b"not an elf".to_vec()),
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(&zeus_elf, &[g5]).is_err());

    // Bad zeus ELF
    let valid_guest = Guest {
        name: "vm".into(),
        image: Image::Raw {
            bytes: vec![0; 4],
            gpa: 0x8000_0000,
        },
        size: 2 * MIB,
        a1: 0,
    };
    assert!(build_machine(b"bad", std::slice::from_ref(&valid_guest)).is_err());

    // Zeus ELF spilling into boot table
    let huge_zeus = dummy_guest_elf(0x803f_f000, vec![0; 4], 8192, 0x803f_f000);
    assert!(build_machine(&huge_zeus, &[valid_guest]).is_err());
}

#[test]
fn split_console_groups_mixed_lines() {
    let raw = b"\
[lithium] lithium: boot
[titan] Titan v0.1
zeus: starting
[lithium] A0
[titan] 
[BAD] not a guest
[lithium] lithium: halt
zeus: lithium exited 0
zeus: all guests done
";
    let grouped = split_console(raw);
    assert_eq!(
        grouped.get("lithium").unwrap(),
        &vec!["lithium: boot", "A0", "lithium: halt"]
    );
    assert_eq!(grouped.get("titan").unwrap(), &vec!["Titan v0.1", ""]);
    assert_eq!(
        grouped.get("zeus").unwrap(),
        &vec![
            "zeus: starting",
            "[BAD] not a guest",
            "zeus: lithium exited 0",
            "zeus: all guests done",
        ]
    );
}
