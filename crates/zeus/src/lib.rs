//! zeus: a type-1 hypervisor on the Silicon H extension (design in docs/ZEUS.md; built by T32).
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]

#[cfg(not(target_os = "none"))]
mod host {
    use std::collections::BTreeMap;

    use silicon::machine::{Config, Machine, RAM_BASE};

    const MIB: u64 = 1024 * 1024;
    const TWO_MIB: u64 = 2 * MIB;
    const MAX_GUEST_SIZE: u64 = 16 * MIB;
    const ZEUS_RAM_END: u64 = 0x8040_0000;
    const BOOT_TABLE_ADDR: u64 = 0x8040_0000;
    const BOOT_TABLE_SIZE: usize = 4096;
    const SLOT_BASE: u64 = 0x8100_0000;
    const SLOT_STRIDE: u64 = 0x0100_0000;
    const HOST_RAM_SIZE: usize = 64 * 1024 * 1024;
    const BOOT_MAGIC: &[u8; 8] = b"ZEUSBOOT";

    /// A guest image to load into guest-physical memory.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Image {
        /// Raw flat binary placed at guest-physical address `gpa`, which is also its entry point.
        Raw {
            /// Raw machine-code and data bytes.
            bytes: Vec<u8>,
            /// Guest-physical load and entry address.
            gpa: u64,
        },
        /// RV64 ELF executable whose segments are loaded at their physical addresses.
        Elf(Vec<u8>),
    }

    /// Description of one guest VM to boot under zeus.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Guest {
        /// Guest name (`1..=16` bytes, ASCII `a-z0-9-`).
        pub name: String,
        /// Guest image (ELF or raw binary).
        pub image: Image,
        /// Guest RAM size in bytes (nonzero, at most 16 MiB, multiple of 2 MiB).
        pub size: u64,
        /// Initial value passed in register `a1` at guest entry.
        pub a1: u64,
    }

    /// Build a 64 MiB Silicon machine with zeus, all guests, and the boot table loaded.
    pub fn build_machine(zeus_elf: &[u8], guests: &[Guest]) -> Result<Machine, String> {
        if guests.is_empty() || guests.len() > 3 {
            return Err(format!("expected 1..=3 guests, got {}", guests.len()));
        }

        let zeus_img = chisel::elf::read(zeus_elf).map_err(|e| format!("invalid zeus ELF: {e}"))?;
        for seg in &zeus_img.segments {
            let end = seg
                .vaddr
                .checked_add(seg.mem_size)
                .ok_or_else(|| "zeus segment overflow".to_owned())?;
            if seg.vaddr < RAM_BASE || end > ZEUS_RAM_END {
                return Err(format!(
                    "zeus segment {:#x}..{:#x} outside {:#x}..{:#x}",
                    seg.vaddr, end, RAM_BASE, ZEUS_RAM_END
                ));
            }
        }

        let cfg = Config {
            ram_size: HOST_RAM_SIZE,
            ..Config::default()
        };
        let mut machine = Machine::new(cfg);
        chisel::elf::load(&zeus_img, &mut machine)
            .map_err(|f| format!("failed to load zeus ELF: {f:?}"))?;

        let mut table = vec![0u8; BOOT_TABLE_SIZE];
        table[0..8].copy_from_slice(BOOT_MAGIC);
        table[8..16].copy_from_slice(&(guests.len() as u64).to_le_bytes());

        for (i, guest) in guests.iter().enumerate() {
            validate_name(&guest.name)?;
            if guest.size == 0 || guest.size > MAX_GUEST_SIZE || guest.size % TWO_MIB != 0 {
                return Err(format!(
                    "guest {}: invalid RAM size {} (must be 2 MiB..=16 MiB, multiple of 2 MiB)",
                    guest.name, guest.size
                ));
            }

            let slot = SLOT_BASE + (i as u64) * SLOT_STRIDE;
            let entry = load_guest(&mut machine, slot, guest)?;

            let off = 16 + i * 48;
            let name_bytes = guest.name.as_bytes();
            table[off..off + name_bytes.len()].copy_from_slice(name_bytes);
            table[off + 16..off + 24].copy_from_slice(&slot.to_le_bytes());
            table[off + 24..off + 32].copy_from_slice(&guest.size.to_le_bytes());
            table[off + 32..off + 40].copy_from_slice(&entry.to_le_bytes());
            table[off + 40..off + 48].copy_from_slice(&guest.a1.to_le_bytes());
        }

        machine
            .load(BOOT_TABLE_ADDR, &table)
            .map_err(|f| format!("failed to write boot table: {f:?}"))?;

        Ok(machine)
    }

    /// Group console output lines by `[<name>] ` prefix, placing zeus's own lines under `"zeus"`.
    pub fn split_console(out: &[u8]) -> BTreeMap<String, Vec<String>> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let text = String::from_utf8_lossy(out);
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix('[') {
                if let Some((name, msg)) = rest.split_once("] ") {
                    if is_valid_name(name) {
                        map.entry(name.to_owned()).or_default().push(msg.to_owned());
                        continue;
                    }
                }
            }
            map.entry("zeus".to_owned())
                .or_default()
                .push(line.to_owned());
        }
        map
    }

    fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 16
            && name
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    }

    fn validate_name(name: &str) -> Result<(), String> {
        if !is_valid_name(name) {
            return Err(format!(
                "invalid guest name {name:?} (must be 1..=16 ASCII a-z0-9-)"
            ));
        }
        Ok(())
    }

    fn load_guest(machine: &mut Machine, slot: u64, guest: &Guest) -> Result<u64, String> {
        let gpa_end = RAM_BASE + guest.size;
        match &guest.image {
            Image::Raw { bytes, gpa } => {
                let end = gpa
                    .checked_add(bytes.len() as u64)
                    .ok_or_else(|| format!("guest {}: raw image overflow", guest.name))?;
                if *gpa < RAM_BASE || end > gpa_end || (bytes.is_empty() && *gpa >= gpa_end) {
                    return Err(format!(
                        "guest {}: raw image {:#x}..{:#x} outside guest RAM {:#x}..{:#x}",
                        guest.name, gpa, end, RAM_BASE, gpa_end
                    ));
                }
                let hpa = slot + (*gpa - RAM_BASE);
                machine
                    .load(hpa, bytes)
                    .map_err(|f| format!("guest {}: load failed: {f:?}", guest.name))?;
                Ok(*gpa)
            }
            Image::Elf(elf_bytes) => {
                let img = chisel::elf::read(elf_bytes)
                    .map_err(|e| format!("guest {}: bad ELF: {e}", guest.name))?;
                let zeros = [0u8; 4096];
                for seg in &img.segments {
                    let end = seg
                        .vaddr
                        .checked_add(seg.mem_size)
                        .ok_or_else(|| format!("guest {}: segment overflow", guest.name))?;
                    if seg.vaddr < RAM_BASE || end > gpa_end {
                        return Err(format!(
                            "guest {}: segment {:#x}..{:#x} outside guest RAM {:#x}..{:#x}",
                            guest.name, seg.vaddr, end, RAM_BASE, gpa_end
                        ));
                    }
                    let mut hpa = slot + (seg.vaddr - RAM_BASE);
                    machine
                        .load(hpa, &seg.data)
                        .map_err(|f| format!("guest {}: load failed: {f:?}", guest.name))?;
                    let data_len = seg.data.len() as u64;
                    if seg.mem_size > data_len {
                        hpa += data_len;
                        let mut rem = seg.mem_size - data_len;
                        while rem > 0 {
                            let chunk =
                                usize::try_from(rem.min(zeros.len() as u64)).unwrap_or(zeros.len());
                            machine.load(hpa, &zeros[..chunk]).map_err(|f| {
                                format!("guest {}: bss zero failed: {f:?}", guest.name)
                            })?;
                            hpa += chunk as u64;
                            rem -= chunk as u64;
                        }
                    }
                }
                Ok(img.entry)
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
pub use host::{build_machine, split_console, Guest, Image};

#[cfg(not(target_os = "none"))]
pub mod scene;
