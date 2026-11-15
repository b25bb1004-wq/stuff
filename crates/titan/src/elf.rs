//! Minimal ELF64 RISC-V executable parser for Titan user programs.

use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use crate::uaccess::{USER_MAX, USER_MIN};

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_RISCV: u16 = 243;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

/// Parsed ELF64 RISC-V user executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Entry point virtual address inside user space.
    pub entry: u64,
    /// Loadable `PT_LOAD` segments.
    pub segments: Vec<Seg>,
}

/// One loadable `PT_LOAD` segment from an ELF64 executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seg {
    /// Segment starting virtual address.
    pub vaddr: u64,
    /// Byte range of initialized segment data inside the input ELF slice.
    pub file: Range<usize>,
    /// Total segment size in memory (zero-filled beyond `file.len()`).
    pub mem_size: u64,
    /// Readable flag (`PF_R`).
    pub r: bool,
    /// Writable flag (`PF_W`).
    pub w: bool,
    /// Executable flag (`PF_X`).
    pub x: bool,
}

/// Errors returned by [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    /// Input ended before a required header, program header, or segment payload.
    Truncated,
    /// Missing `\x7fELF` magic bytes.
    BadMagic,
    /// Valid ELF magic, but not a little-endian RV64 `ET_EXEC` binary.
    Unsupported,
    /// Malformed ELF header sizes.
    BadHeader,
    /// Segment has `p_filesz > p_memsz`.
    FileGreaterThanMem,
    /// Two loadable segments overlap in virtual address space.
    OverlappingSegments,
    /// Address or size arithmetic overflowed `u64`.
    AddressOverflow,
    /// Entry point or segment falls outside user space `0x1_0000..0x3F_FFFF_F000`.
    OutsideUserSpace,
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "truncated ELF binary"),
            Self::BadMagic => write!(f, "invalid ELF magic"),
            Self::Unsupported => write!(f, "unsupported ELF format"),
            Self::BadHeader => write!(f, "malformed ELF header"),
            Self::FileGreaterThanMem => write!(f, "segment file size exceeds memory size"),
            Self::OverlappingSegments => write!(f, "overlapping ELF segments"),
            Self::AddressOverflow => write!(f, "ELF address overflow"),
            Self::OutsideUserSpace => write!(f, "ELF address outside user space"),
        }
    }
}

/// Parse and validate an ELF64 little-endian RISC-V executable from `bytes`.
pub fn parse(bytes: &[u8]) -> Result<Program, ElfError> {
    if let Some(magic) = bytes.get(0..4) {
        if magic != ELF_MAGIC {
            return Err(ElfError::BadMagic);
        }
    } else {
        return Err(ElfError::Truncated);
    }
    if bytes.len() < EHDR_SIZE {
        return Err(ElfError::Truncated);
    }
    if bytes[4] != ELFCLASS64 || bytes[5] != ELFDATA2LSB || bytes[6] != EV_CURRENT {
        return Err(ElfError::Unsupported);
    }

    let e_type = read_u16(bytes, 16)?;
    let e_machine = read_u16(bytes, 18)?;
    let e_version = read_u32(bytes, 20)?;
    if e_type != ET_EXEC || e_machine != EM_RISCV || e_version != 1 {
        return Err(ElfError::Unsupported);
    }

    let entry = read_u64(bytes, 24)?;
    if !(USER_MIN..USER_MAX).contains(&entry) {
        return Err(ElfError::OutsideUserSpace);
    }

    let e_phoff = read_u64(bytes, 32)?;
    let e_ehsize = read_u16(bytes, 52)?;
    let e_phentsize = read_u16(bytes, 54)?;
    let e_phnum = read_u16(bytes, 56)?;
    if usize::from(e_ehsize) != EHDR_SIZE || usize::from(e_phentsize) != PHDR_SIZE {
        return Err(ElfError::BadHeader);
    }

    let ph_len = u64::from(e_phnum) * (PHDR_SIZE as u64);
    let ph_end = e_phoff
        .checked_add(ph_len)
        .ok_or(ElfError::AddressOverflow)?;
    let ph_start_u = usize::try_from(e_phoff).map_err(|_| ElfError::Truncated)?;
    let ph_end_u = usize::try_from(ph_end).map_err(|_| ElfError::Truncated)?;
    let ph_table = bytes.get(ph_start_u..ph_end_u).ok_or(ElfError::Truncated)?;

    let mut segments = Vec::new();
    let mut ranges: Vec<(u64, u64)> = Vec::new();

    for ph in ph_table.as_chunks::<PHDR_SIZE>().0 {
        let p_type = read_u32(ph, 0)?;
        if p_type != PT_LOAD {
            continue;
        }
        let p_flags = read_u32(ph, 4)?;
        let p_offset = read_u64(ph, 8)?;
        let p_vaddr = read_u64(ph, 16)?;
        let p_filesz = read_u64(ph, 32)?;
        let p_memsz = read_u64(ph, 40)?;

        if p_filesz > p_memsz {
            return Err(ElfError::FileGreaterThanMem);
        }

        let f_end = p_offset
            .checked_add(p_filesz)
            .ok_or(ElfError::AddressOverflow)?;
        let f_start_u = usize::try_from(p_offset).map_err(|_| ElfError::Truncated)?;
        let f_end_u = usize::try_from(f_end).map_err(|_| ElfError::Truncated)?;
        if f_end_u > bytes.len() {
            return Err(ElfError::Truncated);
        }

        let m_end = p_vaddr
            .checked_add(p_memsz)
            .ok_or(ElfError::AddressOverflow)?;
        if p_memsz == 0 {
            if !(USER_MIN..USER_MAX).contains(&p_vaddr) {
                return Err(ElfError::OutsideUserSpace);
            }
        } else if p_vaddr < USER_MIN || m_end > USER_MAX {
            return Err(ElfError::OutsideUserSpace);
        }

        if p_memsz > 0 {
            for &(prev_s, prev_e) in &ranges {
                if p_vaddr < prev_e && prev_s < m_end {
                    return Err(ElfError::OverlappingSegments);
                }
            }
            ranges.push((p_vaddr, m_end));
        }

        segments.push(Seg {
            vaddr: p_vaddr,
            file: f_start_u..f_end_u,
            mem_size: p_memsz,
            r: (p_flags & PF_R) != 0,
            w: (p_flags & PF_W) != 0,
            x: (p_flags & PF_X) != 0,
        });
    }

    Ok(Program { entry, segments })
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, ElfError> {
    let s = bytes.get(off..off + 2).ok_or(ElfError::Truncated)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, ElfError> {
    let s = bytes.get(off..off + 4).ok_or(ElfError::Truncated)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_u64(bytes: &[u8], off: usize) -> Result<u64, ElfError> {
    let s = bytes.get(off..off + 8).ok_or(ElfError::Truncated)?;
    Ok(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use alloc::vec;
    use assay::forall;
    use chisel::elf::{write, Image, Segment, PF_R, PF_W, PF_X};

    fn sample_image() -> Image {
        Image {
            entry: 0x1_0000,
            segments: vec![
                Segment {
                    vaddr: 0x1_0000,
                    flags: PF_R | PF_X,
                    data: vec![0x13, 0x00, 0x00, 0x00],
                    mem_size: 4,
                },
                Segment {
                    vaddr: 0x1_1000,
                    flags: PF_R | PF_W,
                    data: vec![0x42],
                    mem_size: 64,
                },
            ],
            symbols: vec![],
        }
    }

    #[test]
    fn parses_valid_chisel_image() {
        let bytes = write(&sample_image());
        let prog = parse(&bytes).unwrap();
        assert_eq!(prog.entry, 0x1_0000);
        assert_eq!(prog.segments.len(), 2);

        let s0 = &prog.segments[0];
        assert_eq!(s0.vaddr, 0x1_0000);
        assert_eq!(s0.mem_size, 4);
        assert_eq!(&bytes[s0.file.clone()], &[0x13, 0x00, 0x00, 0x00]);
        assert!(s0.r && !s0.w && s0.x);

        let s1 = &prog.segments[1];
        assert_eq!(s1.vaddr, 0x1_1000);
        assert_eq!(s1.mem_size, 64);
        assert_eq!(&bytes[s1.file.clone()], &[0x42]);
        assert!(s1.r && s1.w && !s1.x);
    }

    #[test]
    fn rejects_invalid_headers_and_bounds() {
        let valid = write(&sample_image());

        // Bad magic & truncation.
        assert_eq!(parse(&valid[..3]), Err(ElfError::Truncated));
        assert_eq!(parse(&valid[..40]), Err(ElfError::Truncated));
        let mut bad_magic = valid.clone();
        bad_magic[0] = b'X';
        assert_eq!(parse(&bad_magic), Err(ElfError::BadMagic));

        // Not ELF64 / LE / RV64 / ET_EXEC.
        for (off, val) in [(4usize, 1u8), (5, 2), (6, 2), (16, 3), (18, 62), (20, 2)] {
            let mut b = valid.clone();
            b[off] = val;
            assert_eq!(parse(&b), Err(ElfError::Unsupported));
        }

        // Entry outside user space.
        for bad_entry in [0u64, 0xFFFF, USER_MAX, 0x8000_0000_0000_0000] {
            let mut b = valid.clone();
            b[24..32].copy_from_slice(&bad_entry.to_le_bytes());
            assert_eq!(parse(&b), Err(ElfError::OutsideUserSpace));
        }

        // Segment outside user space (kernel address 0x8000_0000 -> wait, > USER_MAX or < USER_MIN).
        let high_seg = write(&Image {
            entry: 0x1_0000,
            segments: vec![Segment {
                vaddr: USER_MAX - 4,
                flags: PF_R | PF_X,
                data: vec![1, 2, 3, 4],
                mem_size: 8,
            }],
            symbols: vec![],
        });
        assert_eq!(parse(&high_seg), Err(ElfError::OutsideUserSpace));

        let low_seg = write(&Image {
            entry: 0x1_0000,
            segments: vec![Segment {
                vaddr: 0x1000,
                flags: PF_R | PF_X,
                data: vec![1],
                mem_size: 1,
            }],
            symbols: vec![],
        });
        assert_eq!(parse(&low_seg), Err(ElfError::OutsideUserSpace));

        // p_filesz > p_memsz.
        let mut bad_sz = valid.clone();
        bad_sz[64 + 40..64 + 48].copy_from_slice(&2u64.to_le_bytes());
        assert_eq!(parse(&bad_sz), Err(ElfError::FileGreaterThanMem));

        // Address overflow in vaddr + memsz.
        let mut ovf = valid.clone();
        ovf[64 + 16..64 + 24].copy_from_slice(&(u64::MAX - 4).to_le_bytes());
        ovf[64 + 40..64 + 48].copy_from_slice(&16u64.to_le_bytes());
        assert_eq!(parse(&ovf), Err(ElfError::AddressOverflow));

        // Overlapping segments.
        let mut overlap = valid.clone();
        overlap[64 + 56 + 16..64 + 56 + 24].copy_from_slice(&0x1_0002u64.to_le_bytes());
        assert_eq!(parse(&overlap), Err(ElfError::OverlappingSegments));

        // Truncated segment payload (last segment data ends at offset 8193).
        assert_eq!(parse(&valid[..8192]), Err(ElfError::Truncated));
    }

    #[test]
    fn fuzz_random_and_mutated_never_panics() {
        let base = write(&sample_image());
        forall(300, 0xE1F6_4001, |rng| {
            let candidate = if rng.chance(1, 3) {
                rng.bytes(256)
            } else {
                let mut mutated = base.clone();
                let ops = rng.range(1, 8);
                for _ in 0..ops {
                    if !mutated.is_empty() && rng.chance(4, 5) {
                        let idx = rng.below(mutated.len());
                        mutated[idx] = rng.u64() as u8;
                    } else {
                        let new_len = rng.below(mutated.len() + 1);
                        mutated.truncate(new_len);
                    }
                }
                mutated
            };
            if let Ok(prog) = parse(&candidate) {
                assert!((USER_MIN..USER_MAX).contains(&prog.entry));
                for (i, seg) in prog.segments.iter().enumerate() {
                    assert!(seg.vaddr >= USER_MIN);
                    let end = seg.vaddr.checked_add(seg.mem_size).unwrap();
                    assert!(end <= USER_MAX);
                    assert!(seg.file.len() as u64 <= seg.mem_size);
                    assert!(seg.file.end <= candidate.len());
                    for other in &prog.segments[..i] {
                        if seg.mem_size > 0 && other.mem_size > 0 {
                            let o_end = other.vaddr + other.mem_size;
                            assert!(!(seg.vaddr < o_end && other.vaddr < end));
                        }
                    }
                }
            }
        });
    }
}
