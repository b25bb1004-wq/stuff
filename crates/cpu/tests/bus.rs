#![allow(clippy::unwrap_used)]
//! RAM device: bounds, sizes and little-endian layout.
use strata_cpu::{Bus, Fault, Ram};
use strata_testkit::forall;

#[test]
fn reads_and_writes_are_little_endian() {
    let mut ram = Ram::new(0x8000, 16);
    ram.write(0x8000, 8, 0x0807_0605_0403_0201).unwrap();
    assert_eq!(ram.read(0x8000, 1), Ok(0x01));
    assert_eq!(ram.read(0x8001, 2), Ok(0x0302));
    assert_eq!(ram.read(0x8000, 4), Ok(0x0403_0201));
    assert_eq!(ram.read(0x8004, 4), Ok(0x0807_0605));
    assert_eq!(ram.read(0x8000, 8), Ok(0x0807_0605_0403_0201));
}

#[test]
fn accesses_outside_the_device_are_unmapped() {
    let mut ram = Ram::new(0x8000, 16);
    assert_eq!(ram.read(0x7fff, 1), Err(Fault::Unmapped(0x7fff)));
    assert_eq!(
        ram.read(0x800c, 8),
        Err(Fault::Unmapped(0x800c)),
        "straddles the end"
    );
    assert_eq!(ram.read(0x800f, 1), Ok(0));
    assert_eq!(ram.read(0x8010, 1), Err(Fault::Unmapped(0x8010)));
    assert_eq!(ram.write(0x8010, 1, 1), Err(Fault::Unmapped(0x8010)));
    assert_eq!(
        ram.read(0x8000, 3),
        Err(Fault::Unmapped(0x8000)),
        "size must be 1, 2, 4 or 8"
    );
}

#[test]
fn load_is_bounds_checked() {
    let mut ram = Ram::new(0x100, 4);
    assert!(ram.load(0x100, &[1, 2, 3, 4]).is_ok());
    assert_eq!(
        ram.load(0x100, &[1, 2, 3, 4, 5]),
        Err(Fault::Unmapped(0x100))
    );
    assert_eq!(ram.read(0x100, 4), Ok(0x0403_0201));
}

#[test]
fn write_then_read_round_trips() {
    forall(300, 0xBEEF, |r| {
        let size = [1u8, 2, 4, 8][r.below(4)];
        let mut ram = Ram::new(0x4000, 256);
        let addr = 0x4000 + r.range(0, 256 - u64::from(size) + 1);
        let v = r.u64();
        let mask = u64::MAX >> (64 - 8 * u32::from(size));
        ram.write(addr, size, v).unwrap();
        assert_eq!(ram.read(addr, size), Ok(v & mask));
    });
}
