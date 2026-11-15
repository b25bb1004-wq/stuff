//! Integration tests booting lithium, titan, spy, and faulting guests under `zeus`.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::Bus;
use zeus::{build_machine, split_console, Guest, Image};

const MIB: u64 = 1024 * 1024;
const STEP_LIMIT: u64 = 80_000_000;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn zeus_elf() -> &'static [u8] {
    static ELF: OnceLock<Vec<u8>> = OnceLock::new();
    ELF.get_or_init(|| {
        let root = workspace_root();
        let target_dir = root.join("target").join("zeus-bin");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .args([
                "build",
                "-p",
                "zeus",
                "--bin",
                "zeus",
                "--release",
                "--target",
                "riscv64gc-unknown-none-elf",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "cargo build for zeus binary failed");
        let path = target_dir
            .join("riscv64gc-unknown-none-elf")
            .join("release")
            .join("zeus");
        std::fs::read(path).unwrap()
    })
}

fn titan_elf() -> &'static [u8] {
    static ELF: OnceLock<Vec<u8>> = OnceLock::new();
    ELF.get_or_init(|| {
        let root = workspace_root();
        let target_dir = root.join("target").join("titan-kernel");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir)
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
            .unwrap();
        assert!(status.success(), "cargo build for titan kernel failed");
        let path = target_dir
            .join("riscv64gc-unknown-none-elf")
            .join("release")
            .join("kernel");
        std::fs::read(path).unwrap()
    })
}

fn lithium_guest(size: u64) -> Guest {
    let bytes = lithium::image().unwrap();
    Guest {
        name: "lithium".into(),
        image: Image::Raw {
            bytes,
            gpa: RAM_BASE,
        },
        size,
        a1: 0,
    }
}

fn titan_guest(size: u64) -> Guest {
    Guest {
        name: "titan".into(),
        image: Image::Elf(titan_elf().to_vec()),
        size,
        a1: size,
    }
}

fn bare_lithium_lines() -> Vec<String> {
    let (exit, out) = lithium::run(5_000_000);
    assert_eq!(exit, Exit::Pass);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn bare_titan_lines(ram_size: usize) -> Vec<String> {
    let img = chisel::elf::read(titan_elf()).unwrap();
    let cfg = Config {
        ram_size,
        ..Config::default()
    };
    let mut m = Machine::new(cfg);
    chisel::elf::load(&img, &mut m).unwrap();
    m.cpu.x[10] = 0;
    m.cpu.x[11] = ram_size as u64;
    let exit = m.run(50_000_000);
    let out = m.board.take_uart_output();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(exit, Exit::Pass, "bare titan failed:\n{text}");
    text.lines().map(str::to_owned).collect()
}

fn assert_lithium_transcript(lines: &[String]) {
    let bare = bare_lithium_lines();
    assert_eq!(lines.len(), bare.len(), "lithium line count: {lines:?}");
    assert_eq!(lines.first().unwrap(), "lithium: boot");
    assert_eq!(lines.last().unwrap(), "lithium: halt");

    let mut a = Vec::new();
    let mut b = Vec::new();
    for line in &lines[1..lines.len() - 1] {
        if line.starts_with('A') {
            a.push(line.as_str());
        } else if line.starts_with('B') {
            b.push(line.as_str());
        } else {
            panic!("unexpected lithium line: {line:?}");
        }
    }
    assert_eq!(a, ["A0", "A1", "A2", "A3", "A4"]);
    assert_eq!(b, ["B0", "B1", "B2", "B3", "B4"]);
}

#[test]
fn lithium_alone() {
    let mut m = build_machine(zeus_elf(), &[lithium_guest(4 * MIB)]).unwrap();
    let exit = m.run(STEP_LIMIT);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(exit, Exit::Pass, "zeus failed:\n{text}");

    let grouped = split_console(&out);
    assert_lithium_transcript(grouped.get("lithium").unwrap());
    assert_eq!(
        grouped.get("zeus").unwrap(),
        &vec![
            "zeus: lithium exited 0".to_owned(),
            "zeus: all guests done".to_owned(),
        ]
    );
}

#[test]
fn titan_alone() {
    let size = 16 * MIB;
    let mut m = build_machine(zeus_elf(), &[titan_guest(size)]).unwrap();
    let exit = m.run(STEP_LIMIT);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(exit, Exit::Pass, "zeus failed:\n{text}");

    let grouped = split_console(&out);
    assert_eq!(
        grouped.get("titan").unwrap(),
        &bare_titan_lines(size as usize)
    );
    assert_eq!(
        grouped.get("zeus").unwrap(),
        &vec![
            "zeus: titan exited 0".to_owned(),
            "zeus: all guests done".to_owned(),
        ]
    );
}

#[test]
fn lithium_and_titan_together() {
    // Solo runs under zeus
    let mut solo_l = build_machine(zeus_elf(), &[lithium_guest(4 * MIB)]).unwrap();
    assert_eq!(solo_l.run(STEP_LIMIT), Exit::Pass);
    let solo_l_lines = split_console(&solo_l.board.take_uart_output())
        .remove("lithium")
        .unwrap();

    let mut solo_t = build_machine(zeus_elf(), &[titan_guest(16 * MIB)]).unwrap();
    assert_eq!(solo_t.run(STEP_LIMIT), Exit::Pass);
    let solo_t_lines = split_console(&solo_t.board.take_uart_output())
        .remove("titan")
        .unwrap();

    // Together under zeus
    let mut m =
        build_machine(zeus_elf(), &[lithium_guest(4 * MIB), titan_guest(16 * MIB)]).unwrap();
    let exit = m.run(STEP_LIMIT);
    let steps = m.board.mtime();
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        exit,
        Exit::Pass,
        "combined run failed (steps={steps}):\n{text}"
    );

    let grouped = split_console(&out);
    assert_eq!(grouped.get("lithium").unwrap(), &solo_l_lines);
    assert_eq!(grouped.get("titan").unwrap(), &solo_t_lines);
    let zeus_lines = grouped.get("zeus").unwrap();
    assert!(zeus_lines.contains(&"zeus: lithium exited 0".to_owned()));
    assert!(zeus_lines.contains(&"zeus: titan exited 0".to_owned()));
    assert_eq!(zeus_lines.last().unwrap(), "zeus: all guests done");
}

#[test]
fn isolation_spy_next_to_lithium() {
    let asm_out = chisel::assemble(lithium::KERNEL_SRC, RAM_BASE).unwrap();
    let lithium_bytes = asm_out.bytes;
    let frames_off = (asm_out
        .symbols
        .iter()
        .find(|(s, _)| s == "frames")
        .unwrap()
        .1
        - RAM_BASE) as usize;
    let strings_off = (asm_out
        .symbols
        .iter()
        .find(|(s, _)| s == "str_boot")
        .unwrap()
        .1
        - RAM_BASE) as usize;

    // Reference solo lithium run
    let mut solo_m = build_machine(zeus_elf(), &[lithium_guest(4 * MIB)]).unwrap();
    assert_eq!(solo_m.run(STEP_LIMIT), Exit::Pass);
    let solo_out = solo_m.board.take_uart_output();
    assert_lithium_transcript(split_console(&solo_out).get("lithium").unwrap());

    // Run spy (slot 0) next to lithium (slot 1)
    let spy_src = include_str!("fixtures/spy.s");
    let spy_bytes = chisel::assemble(spy_src, RAM_BASE).unwrap().bytes;
    let spy_guest = Guest {
        name: "spy".into(),
        image: Image::Raw {
            bytes: spy_bytes,
            gpa: RAM_BASE,
        },
        size: 2 * MIB,
        a1: 0,
    };
    let mut m = build_machine(zeus_elf(), &[spy_guest, lithium_guest(4 * MIB)]).unwrap();
    let exit = m.run(STEP_LIMIT);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(exit, Exit::Pass, "isolation run failed:\n{text}");

    let grouped = split_console(&out);
    assert_eq!(grouped.get("spy").unwrap(), &vec!["spy: faults 7"]);
    assert_lithium_transcript(grouped.get("lithium").unwrap());

    // Verify lithium's slot bytes in host memory (0x8200_0000) match the solo run
    // everywhere except lithium's mutable frames/task state and stacks.
    let read_slot_range =
        |board: &mut silicon::machine::Board, base: u64, start: usize, end: usize| -> Vec<u8> {
            (start..end)
                .map(|off| board.read(base + off as u64, 1).unwrap() as u8)
                .collect()
        };
    assert_eq!(
        read_slot_range(&mut m.board, 0x8200_0000, 0, frames_off),
        read_slot_range(&mut solo_m.board, 0x8100_0000, 0, frames_off)
    );
    assert_eq!(
        read_slot_range(&mut m.board, 0x8200_0000, strings_off, lithium_bytes.len()),
        read_slot_range(
            &mut solo_m.board,
            0x8100_0000,
            strings_off,
            lithium_bytes.len()
        )
    );
    // Untouched 4 KiB page at 0x8035_0000 (between stacks) remains zeroed.
    assert_eq!(
        read_slot_range(&mut m.board, 0x8200_0000, 0x35_0000, 0x35_1000),
        vec![0u8; 4096]
    );
}

#[test]
fn faulting_guest_with_zero_mtvec_is_killed_while_other_finishes() {
    // Guest tries to read unmapped GPA 0x0 with mtvec == 0, so it faults immediately.
    let bad_src = "_start:\n    li t0, 0\n    ld t1, 0(t0)\n    j _start\n";
    let bad_bytes = chisel::assemble(bad_src, RAM_BASE).unwrap().bytes;
    let bad_guest = Guest {
        name: "bad".into(),
        image: Image::Raw {
            bytes: bad_bytes,
            gpa: RAM_BASE,
        },
        size: 2 * MIB,
        a1: 0,
    };

    let mut m = build_machine(zeus_elf(), &[bad_guest, lithium_guest(4 * MIB)]).unwrap();
    let exit = m.run(STEP_LIMIT);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        exit,
        Exit::Fail(1),
        "expected Fail(1), got {exit:?}:\n{text}"
    );

    let grouped = split_console(&out);
    assert_lithium_transcript(grouped.get("lithium").unwrap());
    let zeus_lines = grouped.get("zeus").unwrap();
    assert!(
        zeus_lines
            .iter()
            .any(|l| l.starts_with("zeus: bad killed:")),
        "missing killed message:\n{text}"
    );
    assert!(zeus_lines.contains(&"zeus: lithium exited 0".to_owned()));
    assert_eq!(zeus_lines.last().unwrap(), "zeus: all guests done");
}

#[test]
fn nested_traps_over_64_kills_guest() {
    // Handler faults immediately without executing mret -> triggers >64 trap nesting limit.
    let nest_src = "\
_start:
    la t0, handler
    csrw mtvec, t0
handler:
    li t1, 0
    ld t2, 0(t1)
    j handler
";
    let nest_bytes = chisel::assemble(nest_src, RAM_BASE).unwrap().bytes;
    let nest_guest = Guest {
        name: "nest".into(),
        image: Image::Raw {
            bytes: nest_bytes,
            gpa: RAM_BASE,
        },
        size: 2 * MIB,
        a1: 0,
    };

    let mut m = build_machine(zeus_elf(), &[nest_guest, lithium_guest(4 * MIB)]).unwrap();
    let exit = m.run(STEP_LIMIT);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        exit,
        Exit::Fail(1),
        "expected Fail(1), got {exit:?}:\n{text}"
    );

    let grouped = split_console(&out);
    assert_lithium_transcript(grouped.get("lithium").unwrap());
    let zeus_lines = grouped.get("zeus").unwrap();
    assert!(
        zeus_lines
            .iter()
            .any(|l| l == "zeus: nest killed: trap nesting limit"),
        "missing nesting kill message:\n{text}"
    );
    assert!(zeus_lines.contains(&"zeus: lithium exited 0".to_owned()));
    assert_eq!(zeus_lines.last().unwrap(), "zeus: all guests done");
}

#[test]
fn bad_boot_table_fails_machine() {
    let mut m = build_machine(zeus_elf(), &[lithium_guest(4 * MIB)]).unwrap();
    m.load(0x8040_0000, b"BADMAGIC").unwrap();
    let exit = m.run(1_000_000);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(exit, Exit::Fail(1));
    assert!(text.contains("zeus: bad boot table"), "output:\n{text}");
}

#[test]
fn mmio_devices_csrs_compressed_and_wfi() {
    // Tests: M-mode CSRs (misa, mhartid, pmpcfg0, mstatus MPP=2 WARL, RO trap),
    // MMIO to block/keys/display/PLIC/CLINT/UART, compressed loads/stores, wfi wakeup,
    // and non-zero finisher exit (code 7).
    let src = "\
_start:
    li sp, 0x801f0000
    la t0, trap_vec
    csrw mtvec, t0

    # Check misa has no H bit (bit 7 = 0) and has RV64+IMACSU
    .word 0x30102473        # csrr s0, misa (0x301)
    andi t0, s0, 128
    bnez t0, fail

    # Check mhartid/pmpcfg0/menvcfg/mcounteren read 0
    .word 0xf14022f3        # csrr t0, mhartid (0xf14)
    bnez t0, fail
    .word 0x3a0022f3        # csrr t0, pmpcfg0 (0x3a0)
    bnez t0, fail
    .word 0x30a022f3        # csrr t0, menvcfg (0x30a)
    bnez t0, fail
    .word 0x306022f3        # csrr t0, mcounteren (0x306)
    bnez t0, fail

    # Writing read-only mhartid must trap (s1 increments)
    li s1, 0
    .word 0xf1401073        # csrw mhartid, zero
    li t0, 1
    bne s1, t0, fail

    # mstatus MPP=2 (0x1000) is WARL and ignored
    li t0, 0x1800
    csrw mstatus, t0
    li t0, 0x1000
    csrw mstatus, t0
    csrr t0, mstatus
    li t1, 0x1800
    and t0, t0, t1
    bne t0, t1, fail

    # Stubbed devices read 0 (block, keys, display, PLIC, CLINT msip)
    li t0, 0x10001000
    lw t1, 0(t0)
    bnez t1, fail
    li t0, 0x10002000
    lw t1, 0(t0)
    bnez t1, fail
    li t0, 0x10003000
    lw t1, 0(t0)
    bnez t1, fail
    li t0, 0x0c000000
    lw t1, 0(t0)
    bnez t1, fail
    li t0, 0x02000000
    lw t1, 0(t0)
    bnez t1, fail

    # Compressed loads/stores against CLINT mtimecmp (0x0200_4000)
    li a0, 0x02004000
    li a1, 0x12345678
    .word 0x0001c10c        # c.sw a1, 0(a0) + c.nop
    li a1, 0
    .word 0x0001410c        # c.lw a1, 0(a0) + c.nop
    li t0, 0x12345678
    bne a1, t0, fail

    la t0, val_64_a
    ld a1, 0(t0)
    .word 0x0001e10c        # c.sd a1, 0(a0) + c.nop
    li a1, 0
    .word 0x0001610c        # c.ld a1, 0(a0) + c.nop
    la t0, val_64_a
    ld t0, 0(t0)
    bne a1, t0, fail

    # c.swsp / c.lwsp / c.sdsp / c.ldsp using sp = 0x0200_4000 temporarily
    mv s2, sp
    mv sp, a0
    li a1, 0x0abcdef0
    .word 0x0001c02e        # c.swsp a1, 0(sp) + c.nop
    li a1, 0
    .word 0x00014582        # c.lwsp a1, 0(sp) + c.nop
    li t0, 0x0abcdef0
    bne a1, t0, fail

    la t0, val_64_b
    ld a1, 0(t0)
    .word 0x0001e02e        # c.sdsp a1, 0(sp) + c.nop
    li a1, 0
    .word 0x00016582        # c.ldsp a1, 0(sp) + c.nop
    la t0, val_64_b
    ld t0, 0(t0)
    bne a1, t0, fail
    mv sp, s2

    # WFI wakeup on virtual timer
    li t0, 0x0200bff8
    ld t1, 0(t0)
    li t2, 2000
    add t1, t1, t2
    li t0, 0x02004000
    sd t1, 0(t0)
    li t0, 0x80
    csrs mie, t0
    wfi
    csrw mie, zero

    # Print ok and exit with code 7: (7 << 16) | 0x3333
    li t0, 0x10000000
    li t1, 79
    sb t1, 0(t0)
    li t1, 75
    sb t1, 0(t0)
    li t1, 10
    sb t1, 0(t0)
    li t0, 0x00100000
    li t1, 0x00073333
    sw t1, 0(t0)
spin:
    j spin

fail:
    li t0, 0x00100000
    li t1, 0x00013333
    sw t1, 0(t0)
    j spin

.balign 4
trap_vec:
    addi s1, s1, 1
    csrr t0, mepc
    addi t0, t0, 4
    csrw mepc, t0
    mret

.balign 8
val_64_a:
    .quad 0x1122334455667788
val_64_b:
    .quad 0x0102030405060708
";
    let bytes = chisel::assemble(src, RAM_BASE).unwrap().bytes;
    let guest = Guest {
        name: "probe".into(),
        image: Image::Raw {
            bytes,
            gpa: RAM_BASE,
        },
        size: 2 * MIB,
        a1: 0,
    };
    let mut m = build_machine(zeus_elf(), &[guest]).unwrap();
    let exit = m.run(5_000_000);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(exit, Exit::Fail(1), "probe output:\n{text}");
    let grouped = split_console(&out);
    assert_eq!(grouped.get("probe").unwrap(), &vec!["OK"]);
    assert_eq!(
        grouped.get("zeus").unwrap(),
        &vec![
            "zeus: probe exited 7".to_owned(),
            "zeus: all guests done".to_owned(),
        ]
    );
}

/// Leader check: a guest scanning its whole guest-physical space (2 MiB stride below 4 GiB, 1 GiB stride up to
/// 2^41, and three addresses above) can read only its own RAM and the emulated device windows. Expected readable:
/// CLINT (1), PLIC 64 MiB (32), UART (1), RAM (1) in the first loop and RAM (1) in the second.
#[test]
fn leader_gpa_scan_reads_only_own_ram_and_devices() {
    let spy = include_str!("fixtures/spy.s");
    let start = spy.find("    li t0, 0x80200000").unwrap();
    let end = spy.find("    la a0, str_prefix").unwrap();
    let scan = "    li s6, 0
    li s7, 0x200000
    li s8, 0x100000000
scan1:
    ld t1, 0(s6)
    add s6, s6, s7
    bltu s6, s8, scan1
    li s6, 0
    li s7, 0x40000000
    li s8, 0x20000000000
scan2:
    ld t1, 0(s6)
    add s6, s6, s7
    bltu s6, s8, scan2
    li s6, 0x20000000000
    ld t1, 0(s6)
    li s6, 0x4000000000000
    ld t1, 0(s6)
    li s6, -8
    ld t1, 0(s6)
";
    let pd_start = spy.find("print_dec:").unwrap();
    let pd_end = spy
        .find(
            ".balign 8
fault_count:",
        )
        .unwrap();
    // spy's print_dec prints one digit; print up to 4 digits here (divide by 1000, 100, 10, 1).
    let pd4 = "print_dec:
    mv s3, ra
    mv s9, a0
    li s10, 1000
pd_loop:
    divu a0, s9, s10
    remu s9, s9, s10
    addi a0, a0, 48
    call uart_putc
    li t0, 10
    divu s10, s10, t0
    bnez s10, pd_loop
    jr s3

";
    let tail = &spy[end..];
    let (pd_a, pd_b) = (pd_start - end, pd_end - end);
    let tail = format!("{}{}{}", &tail[..pd_a], pd4, &tail[pd_b..]);
    let src = format!("{}{}{}", &spy[..start], scan, tail);
    let bytes = chisel::assemble(&src, RAM_BASE).unwrap().bytes;
    let g = Guest {
        name: "scan".into(),
        image: Image::Raw {
            bytes,
            gpa: RAM_BASE,
        },
        size: 2 * MIB,
        a1: 0,
    };
    let mut m = build_machine(zeus_elf(), &[g, lithium_guest(4 * MIB)]).unwrap();
    let exit = m.run(STEP_LIMIT * 4);
    let out = m.board.take_uart_output();
    let text = String::from_utf8_lossy(&out);
    assert_eq!(exit, Exit::Pass, "{text}");
    let grouped = split_console(&out);
    let faults = (2048 - 35) + (2048 - 1) + 3;
    assert_eq!(
        grouped.get("scan").unwrap(),
        &vec![format!("spy: faults {faults}")],
        "{text}"
    );
    assert_lithium_transcript(grouped.get("lithium").unwrap());
}
