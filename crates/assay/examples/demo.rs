//! Demo: the assay test kit, with every value fixed by a seed so the output never changes.

use assay::{forall, hex, replay, to_hex, Rng};

fn main() {
    // 1. A seed fixes the whole stream.
    let mut a = Rng::new(42);
    let mut b = Rng::new(42);
    let first: Vec<u64> = (0..3).map(|_| a.u64()).collect();
    let again: Vec<u64> = (0..3).map(|_| b.u64()).collect();
    println!("seed 42, first three values: {first:?}");
    println!("second generator with seed 42 matches: {}", first == again);

    // 2. Bounded draws stay inside their ranges.
    let mut r = Rng::new(7);
    let dice: Vec<u64> = (0..5).map(|_| r.range(1, 7)).collect();
    println!("five draws from 1..7: {dice:?}");
    println!(
        "below(0) returns {} (an empty range gives its lower bound)",
        r.below(0)
    );
    let coin = r.chance(1, 2);
    println!("chance(1, 2) returned {coin}");

    // 3. Random bytes and picks.
    let bytes = r.bytes(16);
    println!("random bytes, length {}: {}", bytes.len(), to_hex(&bytes));
    let names = ["alpha", "beta", "gamma"];
    println!("picked name: {}", r.pick(&names));

    // 4. forall runs a property on many independent streams.
    let mut checked = 0u32;
    forall(500, 2024, |rng| {
        let lo = rng.range(0, 1000);
        let hi = lo + rng.range(1, 1000);
        let v = rng.range(lo, hi);
        assert!((lo..hi).contains(&v), "value {v} outside {lo}..{hi}");
        checked += 1;
    });
    println!("range property held on {checked} cases");

    // 5. Published test vectors: hex in, hex out.
    let vector = hex("de ad:be:ef");
    println!("hex(\"de ad:be:ef\") encodes back to {}", to_hex(&vector));

    // 6. A failing case prints its seed; replay runs one case from that seed.
    replay(0x1234, |rng| {
        println!("replayed case 0x1234 first draw: {}", rng.u64())
    });

    println!("demo finished");
}
