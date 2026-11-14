//! Leader check for T23: seven LLVM-assembled programs (fixtures/machine/*.S, built by
//! briefs/leader/build_quartz_machine.sh) drive the UART, CLINT, PLIC (both contexts), block
//! device, framebuffer and keys on a `Machine`; register-level PLIC/CLINT/UART rules; MMIO fuzz.
#![allow(clippy::unwrap_used)]
use assay::Rng;
use quartz::machine::{Config, Exit, Machine, RAM_BASE};
use quartz::{Bus, Fault, Mode};

const LOOP: [u8; 4] = 0x0000_006fu32.to_le_bytes(); // j .

fn boot(bin: &[u8], cfg: Config) -> Machine {
    let mut m = Machine::new(cfg);
    m.load(RAM_BASE, bin).unwrap();
    m
}

fn out(m: &mut Machine) -> Vec<u8> {
    m.board.take_uart_output()
}

#[test]
fn leader_hello_uart() {
    let mut m = boot(
        include_bytes!("fixtures/machine/hello.bin"),
        Config::default(),
    );
    assert_eq!(m.run(100_000), Exit::Pass);
    assert_eq!(out(&mut m), b"Hello from quartz\n");
}

#[test]
fn leader_uart_rx_interrupt_echo() {
    let mut m = boot(
        include_bytes!("fixtures/machine/echo_irq.bin"),
        Config::default(),
    );
    m.board.uart_input(b"ab");
    assert_eq!(m.run(20_000), Exit::OutOfSteps);
    assert_eq!(out(&mut m), b"AB");
    m.board.uart_input(b"c-Z\n");
    assert_eq!(m.run(20_000), Exit::Pass);
    assert_eq!(out(&mut m), b"C-Z\n");
}

#[test]
fn leader_timer_and_software_interrupts() {
    let mut m = boot(
        include_bytes!("fixtures/machine/timer.bin"),
        Config::default(),
    );
    assert_eq!(m.run(100_000), Exit::Pass);
    assert_eq!(out(&mut m), b"5");
    assert!(m.board.mtime() >= 2500);
    let mut m = boot(
        include_bytes!("fixtures/machine/msi.bin"),
        Config::default(),
    );
    assert_eq!(m.run(10_000), Exit::Pass);
}

fn disk_byte(sector: usize, j: usize) -> u8 {
    ((sector * 7 + j * 3) & 0xff) as u8
}

#[test]
fn leader_block_device_dma_and_irq() {
    let disk: Vec<u8> = (0..16 * 512).map(|i| disk_byte(i / 512, i % 512)).collect();
    let mut m = boot(
        include_bytes!("fixtures/machine/block.bin"),
        Config {
            disk: disk.clone(),
            ..Config::default()
        },
    );
    assert_eq!(m.run(200_000), Exit::Pass);
    let sum: u32 = disk[3 * 512..5 * 512].iter().map(|&b| u32::from(b)).sum();
    assert_eq!(out(&mut m), sum.to_le_bytes());
    let now = m.board.disk();
    assert_eq!(now.len(), disk.len());
    for i in 0..disk.len() {
        let s = i / 512;
        let want = if (9..11).contains(&s) {
            disk[i - 6 * 512].wrapping_add(1)
        } else {
            disk[i]
        };
        assert_eq!(now[i], want, "disk byte {i}");
    }
}

#[test]
fn leader_framebuffer_pattern() {
    let mut m = boot(
        include_bytes!("fixtures/machine/fb.bin"),
        Config {
            fb_width: 64,
            fb_height: 48,
            ..Config::default()
        },
    );
    assert_eq!(m.run(500_000), Exit::Pass);
    let fb = m.board.framebuffer();
    assert_eq!(fb.len(), 64 * 48 * 4);
    for y in 0..48usize {
        for x in 0..64usize {
            let p = &fb[(y * 64 + x) * 4..][..4];
            assert_eq!(p, [x as u8, y as u8, (x ^ y) as u8, 255], "pixel {x},{y}");
        }
    }
}

#[test]
fn leader_keys_in_supervisor_mode() {
    let mut m = boot(
        include_bytes!("fixtures/machine/keys_s.bin"),
        Config::default(),
    );
    m.board.press_key(0x41);
    m.board.press_key(0x142);
    assert_eq!(m.run(20_000), Exit::OutOfSteps);
    assert_eq!(m.cpu.mode, Mode::Supervisor);
    assert_eq!(out(&mut m), b"AB");
    m.board.press_key(0x1B);
    assert_eq!(m.run(20_000), Exit::Pass);
    assert_eq!(out(&mut m), b"\x1b");
}

const PLIC: u64 = 0x0C00_0000;
const CLAIM0: u64 = 0x0C20_0004;
const CLAIM1: u64 = 0x0C20_1004;

fn idle(cfg: Config) -> Machine {
    boot(&LOOP, cfg)
}

fn w(m: &mut Machine, a: u64, size: u8, v: u64) {
    m.board.write(a, size, v).unwrap();
}

fn r(m: &mut Machine, a: u64, size: u8) -> u64 {
    m.board.read(a, size).unwrap()
}

fn mip(m: &Machine) -> u64 {
    m.cpu.csr_read(0x344).unwrap()
}

#[test]
fn leader_plic_priority_claim_complete_threshold() {
    let mut m = idle(Config {
        disk: vec![0; 4 * 512],
        ..Config::default()
    });
    for (src, pri) in [(10u64, 3u64), (8, 3), (11, 5)] {
        w(&mut m, PLIC + 4 * src, 4, pri);
    }
    w(&mut m, PLIC + 0x2000, 4, (1 << 8) | (1 << 10) | (1 << 11));
    w(&mut m, PLIC + 0x20_0000, 4, 0);
    m.board.uart_input(b"x");
    w(&mut m, 0x1000_0001, 1, 1);
    m.board.press_key(7);
    w(&mut m, 0x1000_2008, 4, 1);
    w(&mut m, 0x1000_101C, 4, 1);
    w(&mut m, 0x1000_1000, 8, 0);
    w(&mut m, 0x1000_1008, 8, RAM_BASE + 0x1000);
    w(&mut m, 0x1000_1010, 4, 1);
    w(&mut m, 0x1000_1014, 4, 1);
    assert_eq!(r(&mut m, 0x1000_1018, 4), 0);
    m.step();
    let pend = r(&mut m, PLIC + 0x1000, 4);
    assert_eq!(pend & 0xd00, 0xd00, "pending {pend:#x}");
    assert_ne!(mip(&m) & (1 << 11), 0, "MEIP");
    let claims: Vec<u64> = (0..4).map(|_| r(&mut m, CLAIM0, 4)).collect();
    assert_eq!(
        claims,
        [11, 8, 10, 0],
        "highest priority first, ties to the lowest id"
    );
    for _ in 0..3 {
        m.step();
    }
    assert_eq!(r(&mut m, CLAIM0, 4), 0, "in-flight sources do not re-pend");
    w(&mut m, CLAIM0, 4, 99);
    w(&mut m, CLAIM0, 4, 8);
    m.step();
    assert_eq!(r(&mut m, CLAIM0, 4), 8, "line still high after complete");
    w(&mut m, 0x1000_1020, 4, 0);
    w(&mut m, CLAIM0, 4, 8);
    m.step();
    assert_eq!(r(&mut m, CLAIM0, 4), 0, "line low after ack");
    w(&mut m, CLAIM0, 4, 11);
    w(&mut m, CLAIM0, 4, 10);
    w(&mut m, PLIC + 0x20_0000, 4, 5);
    m.step();
    m.step();
    assert_eq!(mip(&m) & (1 << 11), 0, "priority must exceed threshold");
    w(&mut m, PLIC + 0x20_0000, 4, 4);
    m.step();
    assert_ne!(mip(&m) & (1 << 11), 0);
    assert_eq!(r(&mut m, CLAIM0, 4), 11);
    // priority is 3 bits; source 0 does not exist.
    w(&mut m, PLIC + 4 * 5, 4, 0xffff_ffff);
    assert_eq!(r(&mut m, PLIC + 4 * 5, 4), 7);
    w(&mut m, PLIC, 4, 5);
    assert_eq!(r(&mut m, PLIC, 4), 0);
    // Context 1 (S-mode) only.
    let mut m = idle(Config::default());
    w(&mut m, PLIC + 40, 4, 1);
    w(&mut m, PLIC + 0x2080, 4, 1 << 10);
    w(&mut m, PLIC + 0x20_1000, 4, 0);
    m.board.uart_input(b"y");
    w(&mut m, 0x1000_0001, 1, 1);
    m.step();
    assert_ne!(mip(&m) & (1 << 9), 0, "SEIP");
    assert_eq!(mip(&m) & (1 << 11), 0, "not MEIP");
    assert_eq!(r(&mut m, CLAIM0, 4), 0);
    assert_eq!(r(&mut m, CLAIM1, 4), 10);
}

#[test]
fn leader_clint_uart_display_registers() {
    let mut m = idle(Config {
        fb_width: 20,
        fb_height: 10,
        ..Config::default()
    });
    assert_eq!(
        r(&mut m, 0x0200_4000, 8),
        u64::MAX,
        "mtimecmp resets to max"
    );
    w(&mut m, 0x0200_4000, 4, 0x1234_5678);
    w(&mut m, 0x0200_4004, 4, 0x9abc_def0);
    assert_eq!(r(&mut m, 0x0200_4000, 8), 0x9abc_def0_1234_5678);
    let t = r(&mut m, 0x0200_BFF8, 8);
    assert_eq!(t, m.board.mtime());
    assert_eq!(r(&mut m, 0x0200_BFF8, 4), t & 0xffff_ffff);
    w(&mut m, 0x0200_4000, 8, t + 3);
    for _ in 0..6 {
        m.step();
    }
    assert_ne!(mip(&m) & (1 << 7), 0, "MTIP once mtime >= mtimecmp");
    w(&mut m, 0x0200_4000, 8, u64::MAX);
    m.step();
    assert_eq!(mip(&m) & (1 << 7), 0);
    w(&mut m, 0x0200_0000, 4, 1);
    m.step();
    assert_ne!(mip(&m) & (1 << 3), 0, "MSIP");
    w(&mut m, 0x0200_0000, 4, 0);
    m.step();
    assert_eq!(mip(&m) & (1 << 3), 0);
    // UART 16550 subset.
    assert_eq!(r(&mut m, 0x1000_0005, 1), 0x60);
    m.board.uart_input(b"Q");
    assert_eq!(r(&mut m, 0x1000_0005, 1), 0x61);
    w(&mut m, 0x1000_0001, 1, 1);
    assert_eq!(r(&mut m, 0x1000_0002, 1), 0xC4);
    w(&mut m, 0x1000_0001, 1, 2);
    assert_eq!(r(&mut m, 0x1000_0002, 1), 0xC2);
    w(&mut m, 0x1000_0001, 1, 0);
    assert_eq!(r(&mut m, 0x1000_0002, 1), 0xC1);
    assert_eq!(r(&mut m, 0x1000_0000, 1), u64::from(b'Q'));
    assert_eq!(r(&mut m, 0x1000_0005, 1), 0x60);
    w(&mut m, 0x1000_0007, 1, 0xA5);
    assert_eq!(r(&mut m, 0x1000_0007, 1), 0xA5);
    w(&mut m, 0x1000_0000, 1, u64::from(b'!'));
    assert_eq!(out(&mut m), b"!");
    assert!(m.board.read(0x1000_0000, 4).is_err(), "UART is byte-wide");
    // Display and unmapped addresses.
    assert_eq!(r(&mut m, 0x1000_3000, 4), 20);
    assert_eq!(r(&mut m, 0x1000_3004, 4), 10);
    w(&mut m, 0x3000_0000 + 4 * 21, 4, 0x0403_0201);
    assert_eq!(&m.board.framebuffer()[84..88], [1, 2, 3, 4]);
    assert!(m.board.read(0x3000_0000 + 20 * 10 * 4, 4).is_err());
    assert_eq!(
        m.board.read(0x5000_0000, 4),
        Err(Fault::Unmapped(0x5000_0000))
    );
    let end = RAM_BASE + Config::default().ram_size as u64;
    assert!(m.board.read(end, 1).is_err());
    assert!(m.board.read(end - 8, 8).is_ok());
    // Keys FIFO.
    assert_eq!(r(&mut m, 0x1000_2000, 4), 0);
    assert_eq!(r(&mut m, 0x1000_2004, 4), 0);
    for k in 1..=70u32 {
        m.board.press_key(k);
    }
    assert_eq!(r(&mut m, 0x1000_2000, 4), 64, "FIFO holds 64");
    assert_eq!(r(&mut m, 0x1000_2004, 4), 1);
    assert_eq!(r(&mut m, 0x1000_2000, 4), 63);
}

#[test]
fn leader_mmio_fuzz_never_panics() {
    let mut r = Rng::new(0x23_2026_1115);
    for round in 0..20 {
        let mut m = idle(Config {
            disk: vec![0x5a; 8 * 512],
            fb_width: 16,
            fb_height: 8,
            ..Config::default()
        });
        for _ in 0..2000 {
            let base = [
                0x0200_0000u64,
                0x0200_4000,
                0x0200_BFF8,
                PLIC,
                PLIC + 0x1000,
                PLIC + 0x2000,
                PLIC + 0x20_0000,
                PLIC + 0x20_1000,
                0x1000_0000,
                0x1000_1000,
                0x1000_2000,
                0x1000_3000,
                0x3000_0000,
            ][r.below(13)];
            let a = base + r.below(64) as u64;
            let size = [1u8, 2, 4, 8][r.below(4)];
            let mut v = r.u64();
            if base == 0x1000_1000 && r.chance(1, 2) {
                v = RAM_BASE + r.below(0x2000) as u64; // plausible DMA address or sector
            }
            if r.chance(1, 2) {
                let _ = m.board.read(a, size);
            } else {
                let _ = m.board.write(a, size, v);
            }
            if r.chance(1, 50) {
                m.board.uart_input(&[r.below(256) as u8]);
                m.board.press_key(r.u64() as u32);
            }
        }
        m.load(RAM_BASE, &LOOP).unwrap();
        for _ in 0..200 {
            m.step();
        }
        assert_eq!(m.board.disk().len(), 8 * 512, "round {round}");
    }
}
