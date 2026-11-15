//! Leader check for T31 (hypervisor extension). Copied to crates/quartz/tests/hyp_leader.rs.
//! The two-stage translation model below is the leader's own reading of briefs/T31.md.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use quartz::inst::{encode, Inst};
use quartz::{Access, Bus, Cpu, Mode, Ram, Step, Trap};

const RAM: usize = 4 << 20;
const PPN: u64 = (1 << 44) - 1;
const V: u64 = 1;
const R: u64 = 2;
const W: u64 = 4;
const X: u64 = 8;
const U: u64 = 16;
const A: u64 = 64;
const D: u64 = 128;
const G_ROOT: u64 = 0x1_0000;
const VS_GPA: u64 = 0x4000_0000;
const VS_HOST: u64 = 0x8_0000;
const SUM: u64 = 1 << 18;
const MXR: u64 = 1 << 19;

fn r64(m: &[u8], a: u64) -> Option<u64> {
    let a = usize::try_from(a).ok()?;
    m.get(a..a.checked_add(8)?)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}
fn w64(m: &mut [u8], a: u64, v: u64) {
    let a = a as usize;
    m[a..a + 8].copy_from_slice(&v.to_le_bytes());
}
fn pte(pa: u64, flags: u64) -> u64 {
    ((pa >> 12) << 10) | flags
}
fn index(va: u64, level: u32, g: bool) -> u64 {
    let bits = if g && level == 2 { 0x7ff } else { 0x1ff };
    (va >> (12 + 9 * level)) & bits
}

/// Sets the entry for `va` at `level` in the tree at `root` (table-space addresses; `host` maps
/// them to RAM), making pointer tables on the way with `alloc`.
fn set_leaf(
    m: &mut [u8],
    (root, g): (u64, bool),
    va: u64,
    level: u32,
    entry: u64,
    host: &dyn Fn(u64) -> u64,
    alloc: &mut dyn FnMut() -> u64,
) {
    let mut table = root;
    for l in ((level + 1)..3).rev() {
        let at = host(table + index(va, l, g) * 8);
        let cur = r64(m, at).unwrap();
        if cur & V != 0 && cur & (R | X) == 0 {
            table = ((cur >> 10) & PPN) << 12;
        } else {
            let t = alloc();
            w64(m, at, pte(t, V));
            table = t;
        }
    }
    w64(m, host(table + index(va, level, g) * 8), entry);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Acc {
    F,
    L,
    S,
}
#[derive(PartialEq, Debug)]
enum Out {
    Pa(u64),
    Pf,
    Gpf(u64),
}
#[derive(Clone, Copy, Debug)]
struct Ctx {
    vsatp: u64,
    hgatp: u64,
    vs_sum: bool,
    vs_mxr: bool,
    m_mxr: bool,
    vu: bool,
}

fn g_stage(m: &[u8], c: &Ctx, gpa: u64, acc: Acc) -> Option<u64> {
    if c.hgatp >> 60 == 0 {
        return Some(gpa);
    }
    if gpa >> 41 != 0 {
        return None;
    }
    let mut table = (c.hgatp & PPN & !3) << 12;
    for level in (0..3u32).rev() {
        let e = r64(m, table + index(gpa, level, true) * 8)?;
        if e & V == 0 || e >> 54 != 0 || (e & W != 0 && e & R == 0) {
            return None;
        }
        if e & (R | X) == 0 {
            if level == 0 {
                return None;
            }
            table = ((e >> 10) & PPN) << 12;
            continue;
        }
        let ppn = (e >> 10) & PPN;
        if ppn & ((1 << (9 * level)) - 1) != 0 || e & U == 0 {
            return None;
        }
        let ok = match acc {
            Acc::F => e & X != 0,
            Acc::L => e & R != 0 || (c.m_mxr && e & X != 0),
            Acc::S => e & W != 0,
        };
        if !ok {
            return None;
        }
        let mask = (1u64 << (12 + 9 * level)) - 1;
        return Some(((ppn << 12) & !mask) | (gpa & mask));
    }
    None
}

fn vs_stage(m: &[u8], c: &Ctx, gva: u64, acc: Acc) -> Result<u64, Out> {
    if c.vsatp >> 60 == 0 {
        return Ok(gva);
    }
    let top = gva >> 38;
    if top != 0 && top != 0x3ff_ffff {
        return Err(Out::Pf);
    }
    let mut table = (c.vsatp & PPN) << 12;
    for level in (0..3u32).rev() {
        let at = table + index(gva, level, false) * 8;
        let pa = g_stage(m, c, at, Acc::L).ok_or(Out::Gpf(at))?;
        let e = r64(m, pa).ok_or(Out::Pf)?;
        if e & V == 0 || e >> 54 != 0 || (e & W != 0 && e & R == 0) {
            return Err(Out::Pf);
        }
        if e & (R | X) == 0 {
            if level == 0 {
                return Err(Out::Pf);
            }
            table = ((e >> 10) & PPN) << 12;
            continue;
        }
        let ppn = (e >> 10) & PPN;
        if ppn & ((1 << (9 * level)) - 1) != 0 {
            return Err(Out::Pf);
        }
        let ok = match acc {
            Acc::F => e & X != 0,
            Acc::L => e & R != 0 || ((c.vs_mxr || c.m_mxr) && e & X != 0),
            Acc::S => e & W != 0,
        };
        let user = e & U != 0;
        let priv_ok = if c.vu {
            user
        } else {
            !user || (acc != Acc::F && c.vs_sum)
        };
        if !ok || !priv_ok {
            return Err(Out::Pf);
        }
        let mask = (1u64 << (12 + 9 * level)) - 1;
        return Ok(((ppn << 12) & !mask) | (gva & mask));
    }
    Err(Out::Pf)
}

fn model(m: &[u8], c: &Ctx, gva: u64, acc: Acc) -> Out {
    match vs_stage(m, c, gva, acc) {
        Err(o) => o,
        Ok(gpa) => g_stage(m, c, gpa, acc).map_or(Out::Gpf(gpa), Out::Pa),
    }
}

fn quartz_says(cpu: &mut Cpu, ram: &mut Ram, gva: u64, acc: Acc) -> Out {
    let access = match acc {
        Acc::F => Access::Fetch,
        Acc::L => Access::Load,
        Acc::S => Access::Store,
    };
    match (cpu.translate(ram, gva, access), acc) {
        (Ok(pa), _) => Out::Pa(pa),
        (Err(Trap::InstPageFault(a)), Acc::F)
        | (Err(Trap::LoadPageFault(a)), Acc::L)
        | (Err(Trap::StorePageFault(a)), Acc::S)
            if a == gva =>
        {
            Out::Pf
        }
        (Err(Trap::InstGuestPageFault { gva: a, gpa }), Acc::F)
        | (Err(Trap::LoadGuestPageFault { gva: a, gpa }), Acc::L)
        | (Err(Trap::StoreGuestPageFault { gva: a, gpa }), Acc::S)
            if a == gva =>
        {
            Out::Gpf(gpa)
        }
        (other, _) => panic!("{acc:?} at {gva:#x}: unexpected {other:?}"),
    }
}

fn flags(r: &mut Rng, g: bool) -> u64 {
    let rwx = [R, R | W, X, R | X, R | W | X, R | W | X, R | W, W][r.below(8)];
    let mut f = rwx | A | D;
    if r.below(12) != 0 {
        f |= V;
    }
    if r.below(if g { 6 } else { 2 }) != 0 {
        f |= U;
    }
    if r.below(30) == 0 {
        f |= 1 << 60;
    }
    f
}

/// One random two-stage table set: (RAM image, the guest virtual or physical addresses worth asking about).
fn table_set(r: &mut Rng) -> (Vec<u8>, Ctx, Vec<u64>) {
    let mut m = vec![0u8; RAM];
    let g_on = r.below(5) != 0;
    let vs_on = r.below(5) != 0;
    // With G bare the VS tables must sit at a GPA that is also a RAM address.
    let vs_root = if g_on { VS_GPA } else { VS_HOST };
    let c = Ctx {
        vsatp: if vs_on {
            (8 << 60) | (vs_root >> 12)
        } else {
            0
        },
        hgatp: if g_on { (8 << 60) | (G_ROOT >> 12) } else { 0 },
        vs_sum: r.below(2) == 0,
        vs_mxr: r.below(3) == 0,
        m_mxr: r.below(4) == 0,
        vu: r.below(3) == 0,
    };
    let mut next_g = 0x2_0000u64;
    let mut galloc = || {
        next_g += 0x1000;
        next_g
    };
    let id = |a: u64| a;
    let vs_host = |gpa: u64| if g_on { VS_HOST + (gpa - VS_GPA) } else { gpa };
    let gmap = |m: &mut [u8], gpa: u64, level: u32, e: u64, galloc: &mut dyn FnMut() -> u64| {
        set_leaf(m, (G_ROOT, true), gpa, level, e, &id, galloc);
    };
    // VS table pages: 16 guest pages, G-mapped to host pages unless bare.
    if g_on {
        for k in 0..16 {
            gmap(
                &mut m,
                VS_GPA + k * 0x1000,
                0,
                pte(VS_HOST + k * 0x1000, V | R | W | U | A | D),
                &mut galloc,
            );
        }
    }
    let mut ask = Vec::new();
    let mut data = Vec::new();
    for _ in 0..30 {
        let gpa = 0x8000_0000 + r.below(64) as u64 * 0x1000;
        let host = 0x10_0000 + r.below(0x2f0) as u64 * 0x1000;
        if g_on {
            gmap(&mut m, gpa, 0, pte(host, flags(r, true)), &mut galloc);
        }
        data.push(gpa);
    }
    if g_on {
        let host = if r.below(4) == 0 {
            0x20_1000
        } else {
            0x20_0000
        };
        gmap(
            &mut m,
            0x8020_0000,
            1,
            pte(host, flags(r, true)),
            &mut galloc,
        );
        data.push(0x8020_0000 + r.below(512) as u64 * 0x1000);
        if r.below(3) == 0 {
            gmap(&mut m, 0xc000_0000, 2, pte(0, flags(r, true)), &mut galloc);
            data.push(0xc000_0000 + r.below(0x3ff) as u64 * 0x1000);
        }
    }
    let mut gpas = data.clone();
    gpas.extend([
        0x9000_0000,
        VS_GPA + 0x5000,
        (1 << 41) | 0x8000_0000,
        0x8000_0000 + (1 << 39),
    ]);
    if vs_on {
        let mut next_vs = vs_root;
        let mut vsalloc = || {
            next_vs += 0x1000;
            assert!(next_vs < vs_root + 0x1_0000, "VS table pool");
            next_vs
        };
        for _ in 0..30 {
            let high = r.below(4) == 0;
            let gva = if high {
                0xffff_ffc0_0000_0000
            } else {
                0x10_0000
            } + r.below(64) as u64 * 0x1000;
            let gpa = gpas[r.below(gpas.len())] & !0xfff;
            set_leaf(
                &mut m,
                (vs_root, false),
                gva,
                0,
                pte(gpa, flags(r, false)),
                &vs_host,
                &mut vsalloc,
            );
            ask.push(gva);
        }
        if r.below(2) == 0 {
            let gva = 0x4000_0000;
            set_leaf(
                &mut m,
                (vs_root, false),
                gva,
                1,
                pte(0x8020_0000, flags(r, false)),
                &vs_host,
                &mut vsalloc,
            );
            ask.push(gva + r.below(512) as u64 * 0x1000);
        }
        ask.extend([0x7f_ffff_f000, 0x80_0000_0000, 0xdead_0000]);
        if g_on && r.below(5) == 0 {
            // Break one VS table page's G mapping: implicit guest-page faults.
            let k = r.below(4) as u64;
            gmap(&mut m, VS_GPA + k * 0x1000, 0, 0, &mut galloc);
        }
    } else {
        ask = gpas;
    }
    (m, c, ask)
}

#[test]
fn leader_two_stage_matches_model() {
    let mut r = Rng::new(0x31_2575_0001);
    let (mut pa, mut pf, mut gpf) = (0, 0, 0);
    for set in 0..150 {
        let (m, c, ask) = table_set(&mut r);
        let mut ram = Ram::new(0, RAM);
        ram.load(0, &m).unwrap();
        let mut cpu = Cpu::new(0);
        cpu.csr_write(0x680, c.hgatp);
        cpu.csr_write(0x280, c.vsatp);
        cpu.csr_write(
            0x200,
            if c.vs_sum { SUM } else { 0 } | if c.vs_mxr { MXR } else { 0 },
        );
        cpu.csr_write(0x300, if c.m_mxr { MXR } else { 0 });
        cpu.virt = true;
        cpu.mode = if c.vu { Mode::User } else { Mode::Supervisor };
        for _ in 0..120 {
            let gva = ask[r.below(ask.len())] + r.below(0x1000) as u64;
            let acc = [Acc::F, Acc::L, Acc::S][r.below(3)];
            let want = model(&m, &c, gva, acc);
            let got = quartz_says(&mut cpu, &mut ram, gva, acc);
            assert_eq!(got, want, "set {set} {c:?}: {acc:?} at {gva:#x}");
            match want {
                Out::Pa(_) => pa += 1,
                Out::Pf => pf += 1,
                Out::Gpf(_) => gpf += 1,
            }
        }
    }
    let n = pa + pf + gpf;
    assert!(
        pa * 10 > n && pf * 20 > n && gpf * 20 > n,
        "coverage: {pa} ok, {pf} pf, {gpf} gpf"
    );
}

fn at(ram: &mut Ram, addr: u64, words: &[u32]) {
    for (i, w) in words.iter().enumerate() {
        ram.load(addr + 4 * i as u64, &w.to_le_bytes()).unwrap();
    }
}
fn csrr(rd: u8, csr: u16) -> u32 {
    encode(Inst::Csrrs { rd, rs1: 0, csr })
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Want {
    Ok,
    Ill,
    Virt,
}

#[test]
fn leader_csr_masks_and_redirection() {
    let mut cpu = Cpu::new(0);
    assert_ne!(cpu.csr_read(0x301).unwrap() & (1 << 7), 0, "misa.H");
    let rw = |cpu: &mut Cpu, a: u16, v: u64| {
        assert!(cpu.csr_write(a, v), "{a:#x} write");
        cpu.csr_read(a).unwrap()
    };
    assert_eq!(
        rw(&mut cpu, 0x600, u64::MAX),
        (2 << 32) | (7 << 20) | (0xf << 6)
    );
    assert_eq!(rw(&mut cpu, 0x602, u64::MAX), 0xb1ff);
    assert_eq!(rw(&mut cpu, 0x603, u64::MAX), 0x444);
    assert_eq!(
        rw(&mut cpu, 0x303, 0),
        0x1444,
        "VS and SGEI bits of mideleg read 1"
    );
    assert_eq!(rw(&mut cpu, 0x645, u64::MAX), 0x444);
    assert_eq!(cpu.csr_read(0x644).unwrap() & 0x1444, 0x444);
    assert_eq!(
        cpu.csr_read(0x344).unwrap() & 0x444,
        0x444,
        "mip shows hvip"
    );
    assert_eq!(rw(&mut cpu, 0x604, u64::MAX), 0x1444);
    assert_eq!(
        cpu.csr_read(0x304).unwrap() & 0x1444,
        0x1444,
        "mie shows hie"
    );
    assert_eq!(
        cpu.csr_read(0x104).unwrap() & 0x1444,
        0,
        "sie never shows them"
    );
    assert_eq!(
        cpu.csr_read(0x144).unwrap() & 0x1444,
        0,
        "sip never shows them"
    );
    assert_eq!(
        rw(&mut cpu, 0x680, (8 << 60) | (0x3fff << 44) | 0xfff),
        (8 << 60) | 0xffc
    );
    assert_eq!(
        rw(&mut cpu, 0x680, (1 << 60) | 5),
        (8 << 60) | 0xffc,
        "bad hgatp mode ignored"
    );
    assert_eq!(rw(&mut cpu, 0x280, (8 << 60) | 7), (8 << 60) | 7);
    assert_eq!(
        rw(&mut cpu, 0x280, (9 << 60) | 3),
        (8 << 60) | 7,
        "bad vsatp mode ignored"
    );
    assert_eq!(rw(&mut cpu, 0x241, 0xfff), 0xffe);
    assert_eq!(
        rw(&mut cpu, 0x300, 3 << 38) & (3 << 38),
        3 << 38,
        "MPV and GVA"
    );

    // VS redirection through CSR instructions.
    let mut ram = Ram::new(0, 0x10000);
    let prog = [
        csrr(10, 0x140),
        encode(Inst::Csrrw {
            rd: 0,
            rs1: 11,
            csr: 0x140,
        }),
        csrr(12, 0x144),
        csrr(13, 0x100),
    ];
    at(&mut ram, 0, &prog);
    let mut cpu = Cpu::new(0);
    cpu.csr_write(0x340, 0x1111);
    cpu.csr_write(0x140, 0x2222);
    cpu.csr_write(0x240, 0x3333);
    cpu.csr_write(0x603, 0x444);
    cpu.csr_write(0x645, 0x4);
    cpu.csr_write(0x200, SUM | 2);
    cpu.virt = true;
    cpu.mode = Mode::Supervisor;
    cpu.x[11] = 0x4444;
    for _ in 0..4 {
        cpu.step(&mut ram).unwrap();
    }
    assert_eq!(cpu.x[10], 0x3333, "sscratch reads vsscratch in VS");
    assert_eq!(cpu.csr_read(0x240), Some(0x4444));
    assert_eq!(cpu.csr_read(0x140), Some(0x2222), "HS sscratch untouched");
    assert_eq!(
        cpu.x[12], 0x2,
        "sip in VS: SSIP = hvip.VSSIP through hideleg"
    );
    assert_eq!(cpu.x[13] & (SUM | 2), SUM | 2, "sstatus reads vsstatus");
}

struct Row {
    virt: bool,
    mode: Mode,
    word: u32,
    hstatus: u64,
    mstatus: u64,
    want: Want,
}

#[test]
fn leader_virtual_instruction_table() {
    const VTSR: u64 = 1 << 22;
    const VTW: u64 = 1 << 21;
    const VTVM: u64 = 1 << 20;
    const HU: u64 = 1 << 9;
    const TW: u64 = 1 << 21;
    const TVM: u64 = 1 << 20;
    let (vs, vu, hs, u) = (
        (true, Mode::Supervisor),
        (true, Mode::User),
        (false, Mode::Supervisor),
        (false, Mode::User),
    );
    let hlv_d = 0x6c05_c573;
    let hfence_gvma = 0x62c5_8073;
    let hfence_vvma = 0x22c5_8073;
    let sret = encode(Inst::Sret);
    let wfi = encode(Inst::Wfi);
    let sfence = encode(Inst::SfenceVma { rs1: 0, rs2: 0 });
    let rows = [
        (vs, csrr(10, 0x600), 0, 0, Want::Virt),
        (vs, csrr(10, 0x200), 0, 0, Want::Virt),
        (vu, csrr(10, 0x100), 0, 0, Want::Virt),
        (vs, csrr(10, 0x300), 0, 0, Want::Ill),
        (vu, csrr(10, 0x300), 0, 0, Want::Ill),
        (u, csrr(10, 0x600), 0, 0, Want::Ill),
        (hs, csrr(10, 0x600), 0, 0, Want::Ok),
        (vs, csrr(10, 0x180), VTVM, 0, Want::Virt),
        (vs, csrr(10, 0x180), 0, 0, Want::Ok),
        (vs, sret, VTSR, 0, Want::Virt),
        (vu, sret, 0, 0, Want::Virt),
        (vu, wfi, 0, 0, Want::Virt),
        (vu, wfi, 0, TW, Want::Ill),
        (vs, wfi, VTW, 0, Want::Virt),
        (vs, wfi, 0, 0, Want::Ok),
        (vs, sfence, VTVM, 0, Want::Virt),
        (vs, sfence, 0, 0, Want::Ok),
        (vu, sfence, 0, 0, Want::Virt),
        (vs, hlv_d, 0, 0, Want::Virt),
        (vs, hfence_gvma, 0, 0, Want::Virt),
        (vs, hfence_vvma, 0, 0, Want::Virt),
        (u, hlv_d, 0, 0, Want::Ill),
        (u, hlv_d, HU, 0, Want::Ok),
        (u, hfence_vvma, HU, 0, Want::Ill),
        (hs, hfence_gvma, 0, TVM, Want::Ill),
        (hs, hfence_gvma, 0, 0, Want::Ok),
        (hs, hlv_d, 0, 0, Want::Ok),
    ];
    let rows = rows.map(|((virt, mode), word, hstatus, mstatus, want)| Row {
        virt,
        mode,
        word,
        hstatus,
        mstatus,
        want,
    });
    for (i, row) in rows.iter().enumerate() {
        let mut ram = Ram::new(0, 0x10000);
        at(&mut ram, 0x1000, &[row.word]);
        let mut cpu = Cpu::new(0x1000);
        cpu.csr_write(0x600, row.hstatus);
        cpu.csr_write(0x300, row.mstatus);
        cpu.csr_write(0x200, 1 << 8); // vsstatus.SPP = S, so SRET in VS has somewhere to go
        cpu.x[11] = 0x2000;
        cpu.virt = row.virt;
        cpu.mode = row.mode;
        let got = match cpu.step(&mut ram) {
            Ok(()) => Want::Ok,
            Err(Trap::IllegalInstruction(w)) => {
                assert_eq!(w, row.word, "row {i}: trap value");
                Want::Ill
            }
            Err(Trap::VirtualInstruction(w)) => {
                assert_eq!(w, row.word, "row {i}: trap value");
                Want::Virt
            }
            Err(t) => panic!("row {i}: unexpected {t:?}"),
        };
        assert_eq!(
            got, row.want,
            "row {i}: {:#010x} virt {} {:?}",
            row.word, row.virt, row.mode
        );
    }
    // Counters from VS: both mcounteren and hcounteren must allow.
    for (mc, hc, want) in [(0, 0, Want::Ill), (1, 0, Want::Virt), (1, 1, Want::Ok)] {
        let mut ram = Ram::new(0, 0x10000);
        at(&mut ram, 0, &[csrr(10, 0xc00)]);
        let mut cpu = Cpu::new(0);
        cpu.csr_write(0x306, mc);
        cpu.csr_write(0x606, hc);
        cpu.virt = true;
        cpu.mode = Mode::Supervisor;
        let got = match cpu.step(&mut ram) {
            Ok(()) => Want::Ok,
            Err(Trap::IllegalInstruction(_)) => Want::Ill,
            Err(Trap::VirtualInstruction(_)) => Want::Virt,
            Err(t) => panic!("counter: {t:?}"),
        };
        assert_eq!(got, want, "cycle from VS, mcounteren {mc} hcounteren {hc}");
    }
}

fn tvecs(cpu: &mut Cpu) {
    cpu.csr_write(0x305, 0x100);
    cpu.csr_write(0x105, 0x200);
    cpu.csr_write(0x205, 0x300);
}
fn hstatus(cpu: &Cpu) -> (u64, u64, u64) {
    let h = cpu.csr_read(0x600).unwrap();
    ((h >> 7) & 1, (h >> 8) & 1, (h >> 6) & 1)
}

#[test]
fn leader_trap_routing() {
    let ecall = encode(Inst::Ecall);
    // 1. VS ecall delegated by medeleg (hedeleg cannot take 10): HS.
    let mut ram = Ram::new(0, 0x10_0000);
    at(&mut ram, 0x1000, &[ecall]);
    let mut cpu = Cpu::new(0x1000);
    tvecs(&mut cpu);
    cpu.csr_write(0x302, 1 << 10);
    cpu.csr_write(0x602, u64::MAX);
    cpu.virt = true;
    cpu.mode = Mode::Supervisor;
    assert!(matches!(cpu.tick(&mut ram), Step::Exception(_)));
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, false, 0x200)
    );
    assert_eq!(
        (cpu.csr_read(0x142), cpu.csr_read(0x141)),
        (Some(10), Some(0x1000))
    );
    assert_eq!(hstatus(&cpu), (1, 1, 0), "SPV, SPVP, GVA");
    // 2. Not delegated: M.
    let mut cpu = Cpu::new(0x1000);
    tvecs(&mut cpu);
    cpu.virt = true;
    cpu.mode = Mode::Supervisor;
    cpu.tick(&mut ram);
    let ms = cpu.csr_read(0x300).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::Machine, false, 0x100));
    assert_eq!(cpu.csr_read(0x342), Some(10));
    assert_eq!(
        ((ms >> 39) & 1, (ms >> 38) & 1, (ms >> 11) & 3),
        (1, 0, 1),
        "MPV, GVA, MPP"
    );
    // 3. VU ecall through hedeleg: VS.
    let mut cpu = Cpu::new(0x1000);
    tvecs(&mut cpu);
    cpu.csr_write(0x302, 1 << 8);
    cpu.csr_write(0x602, 1 << 8);
    cpu.virt = true;
    cpu.mode = Mode::User;
    cpu.tick(&mut ram);
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, true, 0x300)
    );
    assert_eq!(
        (cpu.csr_read(0x242), cpu.csr_read(0x241)),
        (Some(8), Some(0x1000))
    );
    assert_eq!(
        cpu.csr_read(0x200).unwrap() & (1 << 8),
        0,
        "vsstatus.SPP = U"
    );
    assert_eq!(cpu.csr_read(0x142), Some(0), "scause untouched");

    // Load page faults from VS: VS-stage Sv39 maps only the code page.
    let gva = 0x55_5000u64;
    let ld = encode(Inst::Ld {
        rd: 10,
        rs1: 11,
        imm: 0,
    });
    let vs_mapped = || {
        let mut ram = Ram::new(0, 0x10_0000);
        at(&mut ram, 0x1000, &[ld]);
        let mut m = vec![0u8; 0x10_0000];
        let mut next = 0x4000u64;
        let mut alloc = || {
            next += 0x1000;
            next
        };
        set_leaf(
            &mut m,
            (0x4000, false),
            0x1000,
            0,
            pte(0x1000, V | R | X | A | D),
            &|a| a,
            &mut alloc,
        );
        ram.load(0x4000, &m[0x4000..0x9000]).unwrap();
        let mut cpu = Cpu::new(0x1000);
        tvecs(&mut cpu);
        cpu.csr_write(0x280, (8 << 60) | 4);
        cpu.x[11] = gva;
        cpu.virt = true;
        cpu.mode = Mode::Supervisor;
        (cpu, ram)
    };
    // 4. Through hedeleg: VS.
    let (mut cpu, mut ram) = vs_mapped();
    cpu.csr_write(0x302, 1 << 13);
    cpu.csr_write(0x602, 1 << 13);
    cpu.tick(&mut ram);
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, true, 0x300)
    );
    assert_eq!(
        (cpu.csr_read(0x242), cpu.csr_read(0x243)),
        (Some(13), Some(gva))
    );
    // 5. medeleg only: HS with GVA.
    let (mut cpu, mut ram) = vs_mapped();
    cpu.csr_write(0x302, 1 << 13);
    cpu.tick(&mut ram);
    assert_eq!((cpu.mode, cpu.virt), (Mode::Supervisor, false));
    assert_eq!(
        (cpu.csr_read(0x142), cpu.csr_read(0x143)),
        (Some(13), Some(gva))
    );
    assert_eq!(hstatus(&cpu), (1, 1, 1));
    assert_eq!(
        cpu.csr_read(0x643),
        Some(0),
        "htval 0 for an ordinary page fault"
    );

    // Guest-page faults: vsatp bare, G-stage maps only the code page.
    let gpa = 0x8000_1238u64;
    let g_mapped = || {
        let mut ram = Ram::new(0, 0x10_0000);
        at(&mut ram, 0x1000, &[ld]);
        let mut m = vec![0u8; 0x10_0000];
        let mut next = 0x2_0000u64;
        let mut alloc = || {
            next += 0x1000;
            next
        };
        set_leaf(
            &mut m,
            (G_ROOT, true),
            0x1000,
            0,
            pte(0x1000, V | R | X | U | A | D),
            &|a| a,
            &mut alloc,
        );
        ram.load(G_ROOT, &m[G_ROOT as usize..0x2_8000]).unwrap();
        let mut cpu = Cpu::new(0x1000);
        tvecs(&mut cpu);
        cpu.csr_write(0x680, (8 << 60) | (G_ROOT >> 12));
        cpu.x[11] = gpa;
        cpu.virt = true;
        cpu.mode = Mode::Supervisor;
        (cpu, ram)
    };
    // 6. To HS.
    let (mut cpu, mut ram) = g_mapped();
    cpu.csr_write(0x302, 1 << 21);
    match cpu.tick(&mut ram) {
        Step::Exception(Trap::LoadGuestPageFault { gva: a, gpa: b }) => {
            assert_eq!((a, b), (gpa, gpa))
        }
        s => panic!("guest-page fault expected, got {s:?}"),
    }
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, false, 0x200)
    );
    assert_eq!(
        (cpu.csr_read(0x142), cpu.csr_read(0x143)),
        (Some(21), Some(gpa))
    );
    assert_eq!(
        (cpu.csr_read(0x643), cpu.csr_read(0x64a)),
        (Some(gpa >> 2), Some(0))
    );
    assert_eq!(hstatus(&cpu), (1, 1, 1));
    // 7. To M.
    let (mut cpu, mut ram) = g_mapped();
    cpu.tick(&mut ram);
    let ms = cpu.csr_read(0x300).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::Machine, false, 0x100));
    assert_eq!(
        (cpu.csr_read(0x342), cpu.csr_read(0x343)),
        (Some(21), Some(gpa))
    );
    assert_eq!(
        (cpu.csr_read(0x34b), cpu.csr_read(0x34a)),
        (Some(gpa >> 2), Some(0))
    );
    assert_eq!(((ms >> 39) & 1, (ms >> 38) & 1), (1, 1));
    // 8. HS illegal instruction to M: MPV 0, GVA 0.
    let mut ram = Ram::new(0, 0x10_0000);
    at(&mut ram, 0x1000, &[0xffff_ffff]);
    let mut cpu = Cpu::new(0x1000);
    tvecs(&mut cpu);
    cpu.csr_write(0x300, 3 << 38);
    cpu.mode = Mode::Supervisor;
    cpu.tick(&mut ram);
    let ms = cpu.csr_read(0x300).unwrap();
    assert_eq!(
        (cpu.mode, (ms >> 39) & 1, (ms >> 38) & 1),
        (Mode::Machine, 0, 0)
    );
}

#[test]
fn leader_vs_interrupts() {
    let nop = encode(Inst::Addi {
        rd: 0,
        rs1: 0,
        imm: 0,
    });
    let setup = |mode: Mode, virt: bool, sie: bool, hideleg: u64| {
        let mut ram = Ram::new(0, 0x10000);
        at(&mut ram, 0x1000, &[nop, nop]);
        let mut cpu = Cpu::new(0x1000);
        tvecs(&mut cpu);
        cpu.csr_write(0x603, hideleg);
        cpu.csr_write(0x645, 1 << 6);
        cpu.csr_write(0x604, 1 << 6);
        cpu.csr_write(0x200, if sie { 2 } else { 0 });
        cpu.virt = virt;
        cpu.mode = mode;
        (cpu, ram)
    };
    let (mut cpu, mut ram) = setup(Mode::Supervisor, true, true, 0x444);
    assert!(matches!(cpu.tick(&mut ram), Step::Interrupt(_)));
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, true, 0x300)
    );
    assert_eq!(
        (cpu.csr_read(0x242), cpu.csr_read(0x241)),
        (Some((1 << 63) | 5), Some(0x1000))
    );
    assert_eq!(
        cpu.csr_read(0x200).unwrap() & 0x22,
        0x20,
        "SIE cleared, SPIE set"
    );
    let (mut cpu, mut ram) = setup(Mode::Supervisor, true, false, 0x444);
    assert_eq!(cpu.tick(&mut ram), Step::Retired, "VS with SIE clear waits");
    let (mut cpu, mut ram) = setup(Mode::User, true, false, 0x444);
    assert!(
        matches!(cpu.tick(&mut ram), Step::Interrupt(_)),
        "VU always takes it"
    );
    assert_eq!(cpu.csr_read(0x242), Some((1 << 63) | 5));
    let (mut cpu, mut ram) = setup(Mode::Supervisor, false, true, 0x444);
    assert_eq!(cpu.tick(&mut ram), Step::Retired, "not taken outside virt");
    let (mut cpu, mut ram) = setup(Mode::Supervisor, true, false, 0);
    assert!(
        matches!(cpu.tick(&mut ram), Step::Interrupt(_)),
        "to HS without hideleg"
    );
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, false, 0x200)
    );
    assert_eq!(cpu.csr_read(0x142), Some((1 << 63) | 6));
    assert_eq!(hstatus(&cpu).0, 1);
    let (mut cpu, mut ram) = setup(Mode::Supervisor, true, true, 0x444);
    cpu.csr_write(0x205, 0x300 | 1);
    cpu.tick(&mut ram);
    assert_eq!(cpu.pc, 0x300 + 4 * 5, "vectored vstvec uses code 5");
}

#[test]
fn leader_xret_with_v() {
    let mut ram = Ram::new(0, 0x10000);
    at(&mut ram, 0x1000, &[encode(Inst::Mret)]);
    at(&mut ram, 0x1100, &[encode(Inst::Sret)]);
    for (mpp, virt) in [(1u64, true), (0, true), (3, false)] {
        let mut cpu = Cpu::new(0x1000);
        cpu.csr_write(0x300, (1 << 39) | (mpp << 11));
        cpu.csr_write(0x341, 0x2000);
        cpu.step(&mut ram).unwrap();
        assert_eq!((cpu.virt, cpu.pc), (virt, 0x2000), "mret MPP {mpp}");
        assert_eq!(cpu.csr_read(0x300).unwrap() & (1 << 39), 0, "MPV cleared");
    }
    let mut cpu = Cpu::new(0x1100);
    cpu.mode = Mode::Supervisor;
    cpu.csr_write(0x600, 1 << 7);
    cpu.csr_write(0x141, 0x3000);
    cpu.step(&mut ram).unwrap();
    assert_eq!((cpu.mode, cpu.virt, cpu.pc), (Mode::User, true, 0x3000));
    assert_eq!(hstatus(&cpu).0, 0, "SPV cleared");
    let mut cpu = Cpu::new(0x1100);
    cpu.mode = Mode::Supervisor;
    cpu.virt = true;
    cpu.csr_write(0x241, 0x4000);
    cpu.csr_write(0x141, 0x5000);
    cpu.step(&mut ram).unwrap();
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::User, true, 0x4000),
        "VS sret uses vsepc"
    );
}

#[test]
fn leader_hlv_hlvx_hsv() {
    let (hlv_d, hlv_b, hlv_bu, hlvx_wu, hsv_d) = (
        0x6c05_c573u32,
        0x6005_c573u32,
        0x6015_c573u32,
        0x6835_c573u32,
        0x6ec5_c073u32,
    );
    let gva = 0x10_0000u64;
    let build = |vs_flags: u64| {
        let mut m = vec![0u8; 0x40_0000];
        let mut next_g = 0x2_0000u64;
        let mut galloc = || {
            next_g += 0x1000;
            next_g
        };
        for k in 0..4 {
            set_leaf(
                &mut m,
                (G_ROOT, true),
                VS_GPA + k * 0x1000,
                0,
                pte(VS_HOST + k * 0x1000, V | R | W | U | A | D),
                &|a| a,
                &mut galloc,
            );
        }
        set_leaf(
            &mut m,
            (G_ROOT, true),
            0x8000_0000,
            0,
            pte(0x30_0000, V | R | W | X | U | A | D),
            &|a| a,
            &mut galloc,
        );
        let mut next_vs = VS_GPA;
        let mut vsalloc = || {
            next_vs += 0x1000;
            next_vs
        };
        let host = |g: u64| VS_HOST + (g - VS_GPA);
        set_leaf(
            &mut m,
            (VS_GPA, false),
            gva,
            0,
            pte(0x8000_0000, vs_flags | V | A | D),
            &host,
            &mut vsalloc,
        );
        m[0x30_0010..0x30_0018].copy_from_slice(&0x8877_6655_4433_2291u64.to_le_bytes());
        let mut ram = Ram::new(0, 0x40_0000);
        ram.load(0, &m).unwrap();
        let mut cpu = Cpu::new(0x1000);
        tvecs(&mut cpu);
        cpu.csr_write(0x680, (8 << 60) | (G_ROOT >> 12));
        cpu.csr_write(0x280, (8 << 60) | (VS_GPA >> 12));
        cpu.csr_write(0x600, 1 << 8); // SPVP = VS
        cpu.mode = Mode::Supervisor;
        cpu.x[11] = gva + 0x10;
        cpu.x[12] = 0x0102_0304_0506_0708;
        (cpu, ram)
    };
    let run = |cpu: &mut Cpu, ram: &mut Ram, w: u32| {
        at(ram, 0x1000, &[w]);
        cpu.pc = 0x1000;
        cpu.tick(ram)
    };
    let (mut cpu, mut ram) = build(R | W);
    assert_eq!(run(&mut cpu, &mut ram, hlv_d), Step::Retired);
    assert_eq!(cpu.x[10], 0x8877_6655_4433_2291);
    run(&mut cpu, &mut ram, hlv_b);
    assert_eq!(cpu.x[10], 0xffff_ffff_ffff_ff91, "hlv.b sign-extends");
    run(&mut cpu, &mut ram, hlv_bu);
    assert_eq!(cpu.x[10], 0x91);
    assert_eq!(run(&mut cpu, &mut ram, hsv_d), Step::Retired);
    assert_eq!(ram.read(0x30_0010, 8).unwrap(), 0x0102_0304_0506_0708);
    assert!(
        !cpu.virt && cpu.mode == Mode::Supervisor,
        "HLV/HSV do not change V"
    );
    // HLVX needs X: on an R/W page it is a load page fault (to M, nothing delegated) with GVA.
    match run(&mut cpu, &mut ram, hlvx_wu) {
        Step::Exception(Trap::LoadPageFault(a)) => assert_eq!(a, gva + 0x10),
        s => panic!("hlvx on R/W page: {s:?}"),
    }
    let ms = cpu.csr_read(0x300).unwrap();
    assert_eq!(
        ((ms >> 39) & 1, (ms >> 38) & 1),
        (0, 1),
        "MPV 0 (from HS), GVA 1"
    );
    let (mut cpu, mut ram) = build(X);
    assert_eq!(
        run(&mut cpu, &mut ram, hlvx_wu),
        Step::Retired,
        "hlvx on X-only page"
    );
    assert_eq!(cpu.x[10], 0x4433_2291);
    assert!(
        matches!(
            run(&mut cpu, &mut ram, hlv_d),
            Step::Exception(Trap::LoadPageFault(_))
        ),
        "hlv on X-only"
    );
    // SPVP = 0 (VU) on a page without U: page fault.
    let (mut cpu, mut ram) = build(R | W);
    cpu.csr_write(0x600, 0);
    assert!(matches!(
        run(&mut cpu, &mut ram, hlv_d),
        Step::Exception(Trap::LoadPageFault(_))
    ));
    // From U with HU.
    let (mut cpu, mut ram) = build(R | W | U);
    cpu.csr_write(0x600, 1 << 9);
    cpu.mode = Mode::User;
    assert_eq!(run(&mut cpu, &mut ram, hlv_d), Step::Retired);
    assert_eq!(cpu.x[10], 0x8877_6655_4433_2291);
}

#[test]
fn leader_guest_runs_under_sv39x4() {
    let mut m = vec![0u8; 0x40_0000];
    let mut next = 0x2_0000u64;
    let mut alloc = || {
        next += 0x1000;
        next
    };
    set_leaf(
        &mut m,
        (G_ROOT, true),
        0x8000_0000,
        0,
        pte(0x20_0000, V | R | X | U | A | D),
        &|a| a,
        &mut alloc,
    );
    set_leaf(
        &mut m,
        (G_ROOT, true),
        0x8000_1000,
        0,
        pte(0x21_0000, V | R | W | U | A | D),
        &|a| a,
        &mut alloc,
    );
    let mut ram = Ram::new(0, 0x40_0000);
    ram.load(0, &m).unwrap();
    let guest = [
        encode(Inst::Sd {
            rs1: 11,
            rs2: 12,
            imm: 8,
        }),
        encode(Inst::Ld {
            rd: 13,
            rs1: 11,
            imm: 8,
        }),
        encode(Inst::Addi {
            rd: 13,
            rs1: 13,
            imm: 1,
        }),
        encode(Inst::Sd {
            rs1: 11,
            rs2: 13,
            imm: 16,
        }),
        encode(Inst::Ecall),
    ];
    at(&mut ram, 0x20_0000, &guest);
    let mut cpu = Cpu::new(0x8000_0000);
    tvecs(&mut cpu);
    cpu.csr_write(0x302, 1 << 10);
    cpu.csr_write(0x680, (8 << 60) | (G_ROOT >> 12));
    cpu.virt = true;
    cpu.mode = Mode::Supervisor;
    cpu.x[11] = 0x8000_1000;
    cpu.x[12] = 0x1234_5678_9abc_def0;
    let mut out = None;
    for _ in 0..10 {
        if let Step::Exception(t) = cpu.tick(&mut ram) {
            out = Some(t);
            break;
        }
    }
    assert_eq!(out, Some(Trap::Ecall));
    assert_eq!(
        (cpu.mode, cpu.virt, cpu.pc),
        (Mode::Supervisor, false, 0x200)
    );
    assert_eq!(
        (cpu.csr_read(0x142), cpu.csr_read(0x141)),
        (Some(10), Some(0x8000_0010))
    );
    assert_eq!(ram.read(0x21_0008, 8).unwrap(), 0x1234_5678_9abc_def0);
    assert_eq!(ram.read(0x21_0010, 8).unwrap(), 0x1234_5678_9abc_def1);
}
