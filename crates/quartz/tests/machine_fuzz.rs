//! Property-based MMIO fuzz test ensuring random accesses never panic.
#![allow(clippy::unwrap_used)]

use assay::forall;
use quartz::machine::{Config, Machine, RAM_BASE};
use quartz::{Bus, Fault};

const BASES: [u64; 10] = [
    0x0000_0000,
    0x0010_0000,
    0x0200_0000,
    0x0C00_0000,
    0x0C20_0000,
    0x1000_0000,
    0x1000_1000,
    0x1000_2000,
    0x1000_3000,
    0x3000_0000,
];

#[test]
fn mmio_fuzz_never_panics() {
    // 400 cases * 100 ops = 40,000 random MMIO accesses + device interactions
    forall(400, 0x57A7_A023, |rng| {
        let mut m = Machine::new(Config {
            ram_size: 4096,
            disk: vec![0u8; 1024],
            fb_width: 8,
            fb_height: 8,
        });
        for _ in 0..100 {
            let base = if rng.chance(1, 10) {
                RAM_BASE
            } else {
                *rng.pick(&BASES)
            };
            let off = match rng.range(0, 4) {
                0 => rng.range(0, 64),
                1 => rng.range(0, 0x1008),
                2 => rng.range(0, 0x1_0008),
                _ => rng.u64() & 0xFFFF_FFFF,
            };
            let addr = base.wrapping_add(off);
            let size = *rng.pick(&[0u8, 1, 2, 3, 4, 8, 16]);
            if rng.chance(1, 2) {
                let _ = m.board.read(addr, size);
            } else {
                let val = rng.u64();
                let _ = m.board.write(addr, size, val);
            }
            if rng.chance(1, 20) {
                m.board.uart_input(&[rng.u64() as u8]);
                m.board.press_key(rng.u64() as u32);
                let _ = m.board.take_uart_output();
                let _ = m.board.framebuffer().len();
                let _ = m.board.disk().len();
                let _ = m.board.mtime();
            }
        }
        assert_eq!(
            m.board.read(0xDEAD_BEE0, 4),
            Err(Fault::Unmapped(0xDEAD_BEE0))
        );
    });
}
