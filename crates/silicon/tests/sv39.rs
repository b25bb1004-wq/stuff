//! Sv39 tests: page tables built in RAM by the test, covering 4 KiB, 2 MiB and 1 GiB pages,
//! every permission fault, SUM, MXR, MPRV, A/D updates and non-canonical addresses.
#![allow(clippy::unwrap_used)]
use silicon::inst::{encode, Inst};
use silicon::{Access, Bus, Cpu, Mode, Ram, Step, Trap};

const V: u64 = 1;
const R: u64 = 2;
const W: u64 = 4;
const X: u64 = 8;
const U: u64 = 16;
const A: u64 = 64;
const D: u64 = 128;
const SUM: u64 = 1 << 18;
const MXR: u64 = 1 << 19;
const MPRV: u64 = 1 << 17;

/// Root table, level-1 table and level-0 table, all in RAM.
const ROOT: u64 = 0x1000;
const L1: u64 = 0x2000;
const L0: u64 = 0x3000;
/// A physical page that the translations point at; only translation is checked, not data.
const DATA_PA: u64 = 0x10_0000;

/// A PTE pointing at physical address `pa` with `flags`.
fn pte(pa: u64, flags: u64) -> u64 {
    ((pa >> 12) << 10) | flags
}

/// Virtual address from the three VPN fields and a 12-bit offset.
fn va(v2: u64, v1: u64, v0: u64, off: u64) -> u64 {
    (v2 << 30) | (v1 << 21) | (v0 << 12) | off
}

fn put(ram: &mut Ram, addr: u64, value: u64) {
    ram.load(addr, &value.to_le_bytes()).unwrap();
}

/// Supervisor-mode hart with Sv39 enabled and the root table at `ROOT`.
fn sv39_cpu() -> (Cpu, Ram) {
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x180, (8 << 60) | (ROOT >> 12));
    cpu.mode = Mode::Supervisor;
    (cpu, Ram::new(0, 0x40_0000))
}

/// Map one 4 KiB page at VPN index (1, 2, `v0`) to `pa` with `flags`; returns its address.
fn map4k(ram: &mut Ram, v0: u64, pa: u64, flags: u64) -> u64 {
    put(ram, ROOT + 8, pte(L1, V));
    put(ram, L1 + 16, pte(L0, V));
    put(ram, L0 + 8 * v0, pte(pa, flags));
    va(1, 2, v0, 0)
}

fn fault(access: Access, vaddr: u64) -> Trap {
    match access {
        Access::Fetch => Trap::InstPageFault(vaddr),
        Access::Load => Trap::LoadPageFault(vaddr),
        Access::Store => Trap::StorePageFault(vaddr),
    }
}

#[test]
fn four_kilobyte_page_translates_with_its_offset() {
    let (mut cpu, mut ram) = sv39_cpu();
    put(&mut ram, ROOT + 8, pte(L1, V));
    put(&mut ram, L1 + 16, pte(L0, V));
    put(&mut ram, L0 + 24, pte(0x20_0000, V | R | W | X | A | D));
    let vaddr = va(1, 2, 3, 0x456);
    assert_eq!(cpu.translate(&mut ram, vaddr, Access::Load), Ok(0x20_0456));
}

#[test]
fn two_mebibyte_superpage_keeps_the_low_vpn_bits() {
    let (mut cpu, mut ram) = sv39_cpu();
    put(&mut ram, ROOT + 8, pte(L1, V));
    put(&mut ram, L1 + 40, pte(0x20_0000, V | R | W | X | A | D));
    let vaddr = va(1, 5, 7, 0x10);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Fetch),
        Ok(0x20_0000 | (7 << 12) | 0x10)
    );
}

#[test]
fn one_gibibyte_superpage_at_the_root() {
    let (mut cpu, mut ram) = sv39_cpu();
    put(&mut ram, ROOT + 24, pte(0x4000_0000, V | R | W | X | A | D));
    let vaddr = va(3, 7, 9, 0x9abc);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Ok(0x4000_0000 | (vaddr & 0x3fff_ffff))
    );
}

#[test]
fn misaligned_superpages_are_page_faults() {
    let (mut cpu, mut ram) = sv39_cpu();
    put(&mut ram, ROOT + 8, pte(L1, V));
    put(&mut ram, L1 + 48, pte(0x20_1000, V | R | A | D));
    let vaddr = va(1, 6, 0, 0);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Err(Trap::LoadPageFault(vaddr))
    );
    put(&mut ram, ROOT + 16, pte(0x4000_1000, V | R | A | D));
    let vaddr = va(2, 0, 0, 0);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Err(Trap::LoadPageFault(vaddr))
    );
}

#[test]
fn permission_matrix() {
    // (flags, access, mode, extra mstatus bits, allowed)
    let cases: [(u64, Access, Mode, u64, bool); 16] = [
        (V | R | A | D, Access::Fetch, Mode::Supervisor, 0, false),
        (V | X | A | D, Access::Fetch, Mode::Supervisor, 0, true),
        (V | R | A | D, Access::Load, Mode::Supervisor, 0, true),
        (V | X | A | D, Access::Load, Mode::Supervisor, 0, false),
        (V | X | A | D, Access::Load, Mode::Supervisor, MXR, true),
        (V | R | A | D, Access::Store, Mode::Supervisor, 0, false),
        (V | R | W | A | D, Access::Store, Mode::Supervisor, 0, true),
        (V | W | A | D, Access::Load, Mode::Supervisor, 0, false),
        (V | R | A | D | U, Access::Load, Mode::Supervisor, 0, false),
        (V | R | A | D | U, Access::Load, Mode::Supervisor, SUM, true),
        (
            V | X | A | D | U,
            Access::Fetch,
            Mode::Supervisor,
            SUM,
            false,
        ),
        (V | R | A | D, Access::Load, Mode::User, 0, false),
        (V | R | A | D | U, Access::Load, Mode::User, 0, true),
        (V | R | W | A | D | U, Access::Store, Mode::User, SUM, true),
        (V | X | A | D | U, Access::Fetch, Mode::User, 0, true),
        (V | R | A | D | U, Access::Fetch, Mode::User, 0, false),
    ];
    for (i, (flags, access, mode, status, allowed)) in cases.into_iter().enumerate() {
        let (mut cpu, mut ram) = sv39_cpu();
        let v0 = 1 + i as u64;
        let vaddr = map4k(&mut ram, v0, DATA_PA, flags);
        cpu.mode = mode;
        cpu.csr_write(0x300, status);
        let got = cpu.translate(&mut ram, vaddr, access);
        if allowed {
            assert_eq!(got, Ok(DATA_PA), "case {i}");
        } else {
            assert_eq!(got, Err(fault(access, vaddr)), "case {i}");
        }
    }
}

#[test]
fn invalid_reserved_and_non_canonical_entries_fault() {
    let (mut cpu, mut ram) = sv39_cpu();
    let vaddr = map4k(&mut ram, 1, DATA_PA, R | A | D);
    put(&mut ram, L0 + 8, pte(DATA_PA, 0) | (1 << 60));
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Err(Trap::LoadPageFault(vaddr)),
        "reserved bit set and V clear"
    );
    let vaddr = map4k(&mut ram, 2, DATA_PA, V | W | A | D);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Err(Trap::LoadPageFault(vaddr)),
        "W without R is reserved"
    );
    let vaddr = map4k(&mut ram, 3, DATA_PA, V | R | A | D);
    let bad = vaddr | (1 << 38);
    assert_eq!(
        cpu.translate(&mut ram, bad, Access::Load),
        Err(Trap::LoadPageFault(bad)),
        "bit 38 set with bits 63:39 clear"
    );
    assert_eq!(cpu.translate(&mut ram, vaddr, Access::Load), Ok(DATA_PA));
}

#[test]
fn pointer_at_the_last_level_is_a_fault() {
    let (mut cpu, mut ram) = sv39_cpu();
    put(&mut ram, ROOT + 8, pte(L1, V));
    put(&mut ram, L1 + 16, pte(L0, V));
    put(&mut ram, L0 + 24, pte(DATA_PA, V));
    let vaddr = va(1, 2, 3, 0);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Err(Trap::LoadPageFault(vaddr))
    );
}

#[test]
fn table_outside_ram_is_a_page_fault() {
    let (mut cpu, mut ram) = sv39_cpu();
    put(&mut ram, ROOT + 8, pte(0x80_0000, V));
    let vaddr = va(1, 0, 0, 0);
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Store),
        Err(Trap::StorePageFault(vaddr))
    );
}

#[test]
fn hardware_sets_accessed_then_dirty() {
    let (mut cpu, mut ram) = sv39_cpu();
    let vaddr = map4k(&mut ram, 4, DATA_PA, V | R | W);
    let slot = L0 + 32;
    cpu.translate(&mut ram, vaddr, Access::Load).unwrap();
    assert_eq!(ram.read(slot, 8).unwrap() & (A | D), A, "load sets A only");
    cpu.translate(&mut ram, vaddr, Access::Store).unwrap();
    assert_eq!(ram.read(slot, 8).unwrap() & (A | D), A | D, "store sets D");
}

#[test]
fn mprv_makes_machine_loads_and_stores_use_mpp() {
    let (mut cpu, mut ram) = sv39_cpu();
    let vaddr = map4k(&mut ram, 5, DATA_PA, V | R | W | U | A | D);
    cpu.mode = Mode::Machine;
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Ok(vaddr),
        "bare in M"
    );
    // MPRV with MPP = S: a user page needs SUM, which is clear.
    cpu.csr_write(0x300, MPRV | (1 << 11));
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Load),
        Err(Trap::LoadPageFault(vaddr))
    );
    assert_eq!(
        cpu.translate(&mut ram, vaddr, Access::Fetch),
        Ok(vaddr),
        "instruction fetch ignores MPRV"
    );
}

#[test]
fn bare_mode_is_the_identity() {
    let mut ram = Ram::new(0, 0x40_0000);
    let mut cpu = Cpu::new(0);
    cpu.mode = Mode::Supervisor;
    assert_eq!(cpu.translate(&mut ram, 0x1234, Access::Load), Ok(0x1234));
}

#[test]
fn paged_fetch_load_and_store_run_end_to_end() {
    let (mut cpu, mut ram) = sv39_cpu();
    // Code at virtual 0 (physical 0x5000), data at virtual 0x1000 (physical 0x6000).
    put(&mut ram, ROOT, pte(L1, V));
    put(&mut ram, L1, pte(L0, V));
    put(&mut ram, L0, pte(0x5000, V | R | X | A | D));
    put(&mut ram, L0 + 8, pte(0x6000, V | R | W | A | D));
    let code = [
        Inst::Lui { rd: 2, imm: 0x1000 },
        Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: 42,
        },
        Inst::Sd {
            rs1: 2,
            rs2: 1,
            imm: 0,
        },
        Inst::Ld {
            rd: 3,
            rs1: 2,
            imm: 0,
        },
    ];
    for (i, inst) in code.iter().enumerate() {
        ram.load(0x5000 + 4 * i as u64, &encode(*inst).to_le_bytes())
            .unwrap();
    }
    for _ in 0..4 {
        assert_eq!(cpu.tick(&mut ram), Step::Retired);
    }
    assert_eq!(cpu.x[3], 42);
    assert_eq!(ram.read(0x6000, 8), Ok(42));
    assert_eq!(cpu.pc, 16);
}
