//! Demo: build an Sv39 page table in a host frame pool, map, translate and unmap pages, then decode trap causes.
use mantle::sv39::{FramePool, PageTable, Pte};
use mantle::trap::Cause;

fn main() {
    let pool = FramePool::new(0x8000_0000, 16);
    println!(
        "frame pool: {} frames, {} free",
        pool.total_count(),
        pool.free_count()
    );

    let mut pt = match PageTable::new(pool) {
        Ok(pt) => pt,
        Err(e) => {
            println!("error creating root table: {e}");
            return;
        }
    };
    println!("root table at {:#x}, satp {:#x}", pt.root(), pt.satp());
    println!("frames in use after root: {}", pt.allocator().used_count());

    let rw = Pte::R | Pte::W;
    let rx = Pte::R | Pte::X;
    let mappings = [
        (0x0000_0000_0040_0000u64, 0x8010_0000u64, rw),
        (0x0000_0000_0040_1000, 0x8010_1000, rx),
        (0x0000_0000_1000_0000, 0x8020_0000, rw),
    ];
    for (va, pa, flags) in mappings {
        match pt.map(va, pa, flags) {
            Ok(()) => println!("map {va:#x} -> {pa:#x} ok, flags {:#04x}", flags.bits()),
            Err(e) => println!("map {va:#x} failed: {e}"),
        }
    }
    println!(
        "frames in use after 3 maps: {}",
        pt.allocator().used_count()
    );

    match pt.map(0x0000_0000_0050_0001, 0x8030_0000, rw) {
        Ok(()) => println!("misaligned map accepted (unexpected)"),
        Err(e) => println!("misaligned map rejected: {e}"),
    }
    match pt.map(0x0000_0000_0060_0000, 0x8030_0000, Pte::W) {
        Ok(()) => println!("write-only map accepted (unexpected)"),
        Err(e) => println!("write-only map rejected: {e}"),
    }

    for va in [
        0x0000_0000_0040_0000u64,
        0x0000_0000_0040_1000,
        0x0000_0000_0070_0000,
    ] {
        match pt.translate(va) {
            Some((pa, flags)) => {
                println!("translate {va:#x} -> {pa:#x} flags {:#04x}", flags.bits())
            }
            None => println!("translate {va:#x} -> not mapped"),
        }
    }

    match pt.unmap(0x0000_0000_1000_0000) {
        Ok((pa, _)) => println!("unmapped 0x10000000, was {pa:#x}"),
        Err(e) => println!("unmap failed: {e}"),
    }
    println!("frames in use after unmap: {}", pt.allocator().used_count());

    for scause in [13u64, 15, 2, (1 << 63) | 5, 42] {
        let cause = Cause::decode(scause);
        println!(
            "scause {scause:#x}: {cause} (page fault: {})",
            cause.is_page_fault()
        );
    }
}
