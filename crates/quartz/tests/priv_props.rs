//! Property tests: decode and encode agree on random words, every new instruction round-trips,
//! random programs and page tables never panic, and random CSR writes stay inside the WARL masks.
#![allow(clippy::unwrap_used)]
use assay::forall;
use quartz::inst::{decode, encode, Inst};
use quartz::{Cpu, Mode, Ram};

/// Every CSR address the hart knows, plus one unknown address.
const ADDRS: [u16; 30] = [
    0x100, 0x104, 0x105, 0x106, 0x140, 0x141, 0x142, 0x143, 0x144, 0x180, 0x300, 0x301, 0x302,
    0x303, 0x304, 0x305, 0x306, 0x340, 0x341, 0x342, 0x343, 0x344, 0x3a0, 0x3b0, 0xb00, 0xb02,
    0xc01, 0xf11, 0x7c0, 0xc02,
];

#[test]
fn decode_then_encode_returns_the_word_for_random_words() {
    forall(50_000, 0xC5_12, |r| {
        let w = r.u64() as u32;
        if let Some(inst) = decode(w) {
            // FENCE keeps no pred, succ, rd or rs1 fields, so those words are exempt.
            if w & 0x7f == 0x0f {
                return;
            }
            assert_eq!(encode(inst), w, "{inst:?}");
        }
    });
}

#[test]
fn every_new_instruction_round_trips() {
    forall(2000, 0x7e57, |r| {
        let rd = r.below(32) as u8;
        let rs1 = r.below(32) as u8;
        let rs2 = r.below(32) as u8;
        let uimm = r.below(32) as u8;
        let csr = r.below(4096) as u16;
        let insts = [
            Inst::Csrrw { rd, rs1, csr },
            Inst::Csrrs { rd, rs1, csr },
            Inst::Csrrc { rd, rs1, csr },
            Inst::Csrrwi { rd, uimm, csr },
            Inst::Csrrsi { rd, uimm, csr },
            Inst::Csrrci { rd, uimm, csr },
            Inst::SfenceVma { rs1, rs2 },
            Inst::Mret,
            Inst::Sret,
            Inst::Wfi,
            Inst::FenceI,
        ];
        for inst in insts {
            assert_eq!(decode(encode(inst)), Some(inst), "{inst:?}");
        }
    });
}

#[test]
fn random_programs_never_panic_in_any_mode() {
    forall(300, 0xBEEF, |r| {
        let mut ram = Ram::new(0, 0x4000);
        let mut buf = vec![0u8; 0x4000];
        r.fill(&mut buf);
        ram.load(0, &buf).unwrap();
        let mut cpu = Cpu::new(0);
        cpu.mode = *r.pick(&[Mode::User, Mode::Supervisor, Mode::Machine]);
        for _ in 0..40 {
            let addr = *r.pick(&ADDRS);
            cpu.csr_write(addr, r.u64());
        }
        for _ in 0..200 {
            cpu.tick(&mut ram);
        }
    });
}

#[test]
fn random_page_tables_never_panic_the_walker() {
    forall(300, 0x5A39, |r| {
        let mut ram = Ram::new(0, 0x4000);
        let mut buf = vec![0u8; 0x4000];
        r.fill(&mut buf);
        ram.load(0, &buf).unwrap();
        let mut cpu = Cpu::new(0);
        // Sv39 with a root inside the RAM; the table contents are random.
        cpu.csr_write(0x180, (8 << 60) | r.below(4) as u64);
        cpu.mode = *r.pick(&[Mode::User, Mode::Supervisor]);
        cpu.csr_write(0x300, r.u64());
        for _ in 0..50 {
            let low = r.u64() & ((1 << 39) - 1);
            let vaddr = if r.chance(1, 2) {
                low
            } else {
                low | 0xffff_ff80_0000_0000
            };
            for access in [
                quartz::Access::Fetch,
                quartz::Access::Load,
                quartz::Access::Store,
            ] {
                let _ = cpu.translate(&mut ram, vaddr, access);
            }
        }
        for _ in 0..50 {
            cpu.tick(&mut ram);
        }
    });
}

#[test]
fn csr_writes_stay_inside_the_warl_masks() {
    forall(3000, 0xC511, |r| {
        let mut cpu = Cpu::new(0);
        for _ in 0..20 {
            let addr = *r.pick(&ADDRS);
            cpu.csr_write(addr, r.u64());
        }
        let read = |a: u16| cpu.csr_read(a).unwrap();
        let mstatus = read(0x300);
        assert_eq!(mstatus & !(0x7e_19aa | (2 << 32) | (2 << 34)), 0);
        assert_ne!((mstatus >> 11) & 3, 2, "MPP is never the reserved value");
        assert_eq!(read(0x302) & !0xb3ff, 0);
        assert_eq!(read(0x303) & !0x222, 0);
        assert_eq!(read(0x304) & !0xaaa, 0);
        assert_eq!(read(0x344) & !0x222, 0, "no hardware pending bits were set");
        assert!((read(0x305) & 3) <= 1);
        assert!((read(0x105) & 3) <= 1);
        assert_eq!(read(0x341) & 3, 0);
        assert_eq!(read(0x141) & 3, 0);
        assert!(matches!(read(0x180) >> 60, 0 | 8));
        assert_eq!(read(0x306) & !7, 0);
        assert_eq!(read(0x106) & !7, 0);
        assert_eq!(read(0x3b0), 0);
    });
}
