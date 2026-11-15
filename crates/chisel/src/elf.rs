//! ELF64 writer, reader, and quartz machine loader for RV64 executables.

use std::fmt;

/// Executable segment flag.
pub const PF_X: u32 = 1;
/// Writable segment flag.
pub const PF_W: u32 = 2;
/// Readable segment flag.
pub const PF_R: u32 = 4;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_RISCV: u16 = 243;
const EF_RISCV_RVC: u32 = 1;
const PT_LOAD: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHN_ABS: u16 = 0xfff1;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const SHDR_SIZE: usize = 64;
const SYM_SIZE: usize = 24;
const PAGE_ALIGN: u64 = 4096;

/// A loadable ELF64 segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Virtual (and physical) load address.
    pub vaddr: u64,
    /// Segment permission flags (`PF_R`, `PF_W`, `PF_X`).
    pub flags: u32,
    /// File-backed segment bytes.
    pub data: Vec<u8>,
    /// Total size of the segment in memory.
    pub mem_size: u64,
}

/// An in-memory representation of an RV64 ELF64 executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Entry point virtual address.
    pub entry: u64,
    /// Loadable `PT_LOAD` segments in program-header order.
    pub segments: Vec<Segment>,
    /// Named symbols `(name, value)` in symbol-table order.
    pub symbols: Vec<(String, u64)>,
}

/// Errors returned when parsing an ELF64 image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    /// Input ended before a required header, segment, section, or string.
    Truncated,
    /// Missing `\x7fELF` magic bytes.
    BadMagic,
    /// Valid ELF magic, but not a supported little-endian RV64 executable.
    Unsupported(String),
    /// Invalid or overlapping `PT_LOAD` segment.
    BadSegment(String),
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElfError::Truncated => write!(f, "truncated ELF file"),
            ElfError::BadMagic => write!(f, "invalid ELF magic"),
            ElfError::Unsupported(msg) => write!(f, "unsupported ELF: {msg}"),
            ElfError::BadSegment(msg) => write!(f, "invalid ELF segment: {msg}"),
        }
    }
}

impl std::error::Error for ElfError {}

/// Serialize `img` as a little-endian RV64 ELF64 executable.
pub fn write(img: &Image) -> Vec<u8> {
    assert!(img.segments.len() <= u16::MAX as usize, "too many segments");
    let mut ranges = Vec::with_capacity(img.segments.len());
    for seg in &img.segments {
        let data_len = seg.data.len() as u64;
        assert!(seg.mem_size >= data_len, "segment mem_size < data.len()");
        let end = seg
            .vaddr
            .checked_add(seg.mem_size)
            .expect("segment vaddr + mem_size overflow");
        if seg.mem_size > 0 {
            for &(prev_start, prev_end) in &ranges {
                assert!(
                    !(seg.vaddr < prev_end && prev_start < end),
                    "overlapping segments"
                );
            }
            ranges.push((seg.vaddr, end));
        }
    }

    let shstrtab = b"\0.shstrtab\0.symtab\0.strtab\0";
    let mut strtab = vec![0u8];
    let mut sym_names = Vec::with_capacity(img.symbols.len());
    for (name, _) in &img.symbols {
        assert!(!name.is_empty(), "empty symbol name");
        assert!(!name.as_bytes().contains(&0), "symbol name contains NUL");
        let off = u32::try_from(strtab.len()).expect("strtab size overflow");
        sym_names.push(off);
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);
    }

    let phnum = img.segments.len();
    let header_and_phdrs = EHDR_SIZE + PHDR_SIZE * phnum;
    let mut out = vec![0u8; header_and_phdrs];

    let mut ph_offsets = Vec::with_capacity(phnum);
    for seg in &img.segments {
        let cur_mod = out.len() % (PAGE_ALIGN as usize);
        let target_mod = (seg.vaddr % PAGE_ALIGN) as usize;
        let pad = (target_mod + (PAGE_ALIGN as usize) - cur_mod) % (PAGE_ALIGN as usize);
        out.resize(out.len() + pad, 0);
        let p_offset = out.len() as u64;
        out.extend_from_slice(&seg.data);
        ph_offsets.push(p_offset);
    }

    let shstrtab_off = out.len() as u64;
    out.extend_from_slice(shstrtab);

    let sym_pad = (8 - (out.len() % 8)) % 8;
    out.resize(out.len() + sym_pad, 0);
    let symtab_off = out.len() as u64;
    out.extend_from_slice(&[0u8; SYM_SIZE]);
    for (i, &(_, value)) in img.symbols.iter().enumerate() {
        let mut sym = [0u8; SYM_SIZE];
        sym[0..4].copy_from_slice(&sym_names[i].to_le_bytes());
        sym[4] = 0x10; // STB_GLOBAL | STT_NOTYPE
        sym[5] = 0;
        sym[6..8].copy_from_slice(&SHN_ABS.to_le_bytes());
        sym[8..16].copy_from_slice(&value.to_le_bytes());
        sym[16..24].copy_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&sym);
    }
    let symtab_size = (SYM_SIZE * (img.symbols.len() + 1)) as u64;

    let strtab_off = out.len() as u64;
    out.extend_from_slice(&strtab);

    let shdr_pad = (8 - (out.len() % 8)) % 8;
    out.resize(out.len() + shdr_pad, 0);
    let e_shoff = out.len() as u64;

    // Section 0: null, 1: .shstrtab, 2: .symtab, 3: .strtab
    out.extend_from_slice(&[0u8; SHDR_SIZE]);
    push_shdr(
        &mut out,
        1,
        SHT_STRTAB,
        shstrtab_off,
        shstrtab.len() as u64,
        0,
        0,
        1,
        0,
    );
    push_shdr(
        &mut out,
        11,
        SHT_SYMTAB,
        symtab_off,
        symtab_size,
        3,
        1,
        8,
        SYM_SIZE as u64,
    );
    push_shdr(
        &mut out,
        19,
        SHT_STRTAB,
        strtab_off,
        strtab.len() as u64,
        0,
        0,
        1,
        0,
    );

    // Fill ELF header
    out[0..4].copy_from_slice(&ELF_MAGIC);
    out[4] = ELFCLASS64;
    out[5] = ELFDATA2LSB;
    out[6] = EV_CURRENT;
    out[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
    out[18..20].copy_from_slice(&EM_RISCV.to_le_bytes());
    out[20..24].copy_from_slice(&1u32.to_le_bytes());
    out[24..32].copy_from_slice(&img.entry.to_le_bytes());
    out[32..40].copy_from_slice(&(EHDR_SIZE as u64).to_le_bytes());
    out[40..48].copy_from_slice(&e_shoff.to_le_bytes());
    out[48..52].copy_from_slice(&EF_RISCV_RVC.to_le_bytes());
    out[52..54].copy_from_slice(&(EHDR_SIZE as u16).to_le_bytes());
    out[54..56].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
    out[56..58].copy_from_slice(&(phnum as u16).to_le_bytes());
    out[58..60].copy_from_slice(&(SHDR_SIZE as u16).to_le_bytes());
    out[60..62].copy_from_slice(&4u16.to_le_bytes());
    out[62..64].copy_from_slice(&1u16.to_le_bytes());

    // Fill program headers
    for (i, seg) in img.segments.iter().enumerate() {
        let base = EHDR_SIZE + i * PHDR_SIZE;
        out[base..base + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        out[base + 4..base + 8].copy_from_slice(&seg.flags.to_le_bytes());
        out[base + 8..base + 16].copy_from_slice(&ph_offsets[i].to_le_bytes());
        out[base + 16..base + 24].copy_from_slice(&seg.vaddr.to_le_bytes());
        out[base + 24..base + 32].copy_from_slice(&seg.vaddr.to_le_bytes());
        out[base + 32..base + 40].copy_from_slice(&(seg.data.len() as u64).to_le_bytes());
        out[base + 40..base + 48].copy_from_slice(&seg.mem_size.to_le_bytes());
        out[base + 48..base + 56].copy_from_slice(&PAGE_ALIGN.to_le_bytes());
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn push_shdr(
    out: &mut Vec<u8>,
    name: u32,
    sh_type: u32,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
) {
    let mut sh = [0u8; SHDR_SIZE];
    sh[0..4].copy_from_slice(&name.to_le_bytes());
    sh[4..8].copy_from_slice(&sh_type.to_le_bytes());
    sh[24..32].copy_from_slice(&offset.to_le_bytes());
    sh[32..40].copy_from_slice(&size.to_le_bytes());
    sh[40..44].copy_from_slice(&link.to_le_bytes());
    sh[44..48].copy_from_slice(&info.to_le_bytes());
    sh[48..56].copy_from_slice(&addralign.to_le_bytes());
    sh[56..64].copy_from_slice(&entsize.to_le_bytes());
    out.extend_from_slice(&sh);
}

/// Parse a little-endian RV64 ELF64 executable into an [`Image`].
pub fn read(bytes: &[u8]) -> Result<Image, ElfError> {
    if bytes.len() < EHDR_SIZE {
        return Err(ElfError::Truncated);
    }
    if bytes.get(0..4) != Some(&ELF_MAGIC) {
        return Err(ElfError::BadMagic);
    }
    if bytes[4] != ELFCLASS64 {
        return Err(ElfError::Unsupported("not ELF64".into()));
    }
    if bytes[5] != ELFDATA2LSB {
        return Err(ElfError::Unsupported("not little-endian".into()));
    }
    if bytes[6] != EV_CURRENT {
        return Err(ElfError::Unsupported("unsupported ELF version".into()));
    }
    let e_type = read_u16(bytes, 16)?;
    if e_type != ET_EXEC {
        return Err(ElfError::Unsupported("not ET_EXEC".into()));
    }
    let e_machine = read_u16(bytes, 18)?;
    if e_machine != EM_RISCV {
        return Err(ElfError::Unsupported("not RISC-V".into()));
    }
    let entry = read_u64(bytes, 24)?;
    let e_phoff = read_u64(bytes, 32)?;
    let e_shoff = read_u64(bytes, 40)?;
    let e_phentsize = read_u16(bytes, 54)?;
    if e_phentsize != PHDR_SIZE as u16 {
        return Err(ElfError::Unsupported("unexpected e_phentsize".into()));
    }
    let e_phnum = read_u16(bytes, 56)?;
    let e_shentsize = read_u16(bytes, 58)?;
    let e_shnum = read_u16(bytes, 60)?;

    let ph_table = slice_range(bytes, e_phoff, u64::from(e_phnum) * (PHDR_SIZE as u64))?;
    let mut raw_segments = Vec::new();
    for ph in ph_table.as_chunks::<PHDR_SIZE>().0 {
        let p_type = read_u32(ph, 0)?;
        if p_type != PT_LOAD {
            continue;
        }
        let flags = read_u32(ph, 4)?;
        let p_offset = read_u64(ph, 8)?;
        let vaddr = read_u64(ph, 16)?;
        let p_filesz = read_u64(ph, 32)?;
        let mem_size = read_u64(ph, 40)?;
        let data = slice_range(bytes, p_offset, p_filesz)?.to_vec();
        raw_segments.push(Segment {
            vaddr,
            flags,
            data,
            mem_size,
        });
    }

    let symbols = read_symbols(bytes, e_shoff, e_shentsize, e_shnum)?;

    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for seg in &raw_segments {
        if (seg.data.len() as u64) > seg.mem_size {
            return Err(ElfError::BadSegment("p_filesz > p_memsz".into()));
        }
        let end = seg
            .vaddr
            .checked_add(seg.mem_size)
            .ok_or_else(|| ElfError::BadSegment("vaddr + mem_size overflow".into()))?;
        if seg.mem_size > 0 {
            for &(prev_start, prev_end) in &ranges {
                if seg.vaddr < prev_end && prev_start < end {
                    return Err(ElfError::BadSegment("overlapping PT_LOAD segments".into()));
                }
            }
            ranges.push((seg.vaddr, end));
        }
    }

    Ok(Image {
        entry,
        segments: raw_segments,
        symbols,
    })
}

fn read_symbols(
    bytes: &[u8],
    e_shoff: u64,
    e_shentsize: u16,
    e_shnum: u16,
) -> Result<Vec<(String, u64)>, ElfError> {
    if e_shnum == 0 {
        return Ok(Vec::new());
    }
    let sh_table = slice_range(bytes, e_shoff, u64::from(e_shnum) * u64::from(e_shentsize))?;
    if e_shentsize != SHDR_SIZE as u16 {
        return Err(ElfError::Unsupported("unexpected e_shentsize".into()));
    }

    let mut symtab_hdr = None;
    for sh in sh_table.as_chunks::<SHDR_SIZE>().0 {
        if read_u32(sh, 4)? == SHT_SYMTAB {
            symtab_hdr = Some(sh);
            break;
        }
    }
    let Some(sym_sh) = symtab_hdr else {
        return Ok(Vec::new());
    };

    let sh_offset = read_u64(sym_sh, 24)?;
    let sh_size = read_u64(sym_sh, 32)?;
    let sh_link = read_u32(sym_sh, 40)?;
    let sh_entsize = read_u64(sym_sh, 56)?;
    if sh_entsize != SYM_SIZE as u64 {
        return Err(ElfError::Unsupported("unexpected symtab sh_entsize".into()));
    }
    let sym_bytes = slice_range(bytes, sh_offset, sh_size)?;
    if sym_bytes.len() % SYM_SIZE != 0 {
        return Err(ElfError::Truncated);
    }

    let link_idx = usize::try_from(sh_link).map_err(|_| ElfError::Truncated)?;
    let link_off = link_idx.checked_mul(SHDR_SIZE).ok_or(ElfError::Truncated)?;
    let str_sh = sh_table
        .get(link_off..link_off + SHDR_SIZE)
        .ok_or(ElfError::Truncated)?;
    let str_offset = read_u64(str_sh, 24)?;
    let str_size = read_u64(str_sh, 32)?;
    let strtab = slice_range(bytes, str_offset, str_size)?;

    let mut symbols = Vec::new();
    for sym in sym_bytes.as_chunks::<SYM_SIZE>().0 {
        let st_name = read_u32(sym, 0)? as usize;
        let st_info = sym[4];
        let st_value = read_u64(sym, 8)?;
        let tail = strtab.get(st_name..).ok_or(ElfError::Truncated)?;
        let nul = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or(ElfError::Truncated)?;
        let name = std::str::from_utf8(&tail[..nul])
            .map_err(|_| ElfError::Unsupported("non-UTF-8 symbol name".into()))?;
        if !name.is_empty() && (st_info & 0xf) <= 2 {
            symbols.push((name.to_owned(), st_value));
        }
    }
    Ok(symbols)
}

fn slice_range(bytes: &[u8], off: u64, len: u64) -> Result<&[u8], ElfError> {
    let end = off.checked_add(len).ok_or(ElfError::Truncated)?;
    let start = usize::try_from(off).map_err(|_| ElfError::Truncated)?;
    let stop = usize::try_from(end).map_err(|_| ElfError::Truncated)?;
    bytes.get(start..stop).ok_or(ElfError::Truncated)
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

/// Load `img`'s segments into `m` (zeroing BSS) and set `m.cpu.pc` to `img.entry`.
pub fn load(img: &Image, m: &mut quartz::machine::Machine) -> Result<(), quartz::Fault> {
    let zeros = [0u8; 4096];
    for seg in &img.segments {
        m.load(seg.vaddr, &seg.data)?;
        let data_len = seg.data.len() as u64;
        if seg.mem_size > data_len {
            let mut addr = seg
                .vaddr
                .checked_add(data_len)
                .ok_or(quartz::Fault::Unmapped(seg.vaddr))?;
            let mut rem = seg.mem_size - data_len;
            while rem > 0 {
                let chunk = usize::try_from(rem.min(zeros.len() as u64)).unwrap_or(zeros.len());
                m.load(addr, &zeros[..chunk])?;
                addr = addr
                    .checked_add(chunk as u64)
                    .ok_or(quartz::Fault::Unmapped(addr))?;
                rem -= chunk as u64;
            }
        }
    }
    m.cpu.pc = img.entry;
    Ok(())
}
