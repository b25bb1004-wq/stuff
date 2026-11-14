// Shared by the rustc-built RISC-V smoke program and the host test: same code, same answer.
// Sorting, atomics (amo*, lr/sc loops), u128 multiply/divide; no floats.
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::SeqCst};

static C: AtomicU64 = AtomicU64::new(7);
static I: AtomicI32 = AtomicI32::new(-3);
static U: AtomicU32 = AtomicU32::new(0xdead_beef);

pub fn smoke() -> u64 {
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut v = [0u64; 96];
    for x in v.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *x = s;
    }
    v.sort_unstable();
    let mut h: u64 = 0;
    for (i, x) in v.iter().enumerate() {
        h = h.rotate_left(5) ^ x.wrapping_mul(i as u64 + 1);
    }
    for i in 0..1000u64 {
        C.fetch_add(i ^ (h & 0xff), SeqCst);
    }
    I.fetch_max(-7, SeqCst);
    I.fetch_min(-100, SeqCst);
    I.fetch_max(5, SeqCst);
    I.fetch_add(-11, SeqCst);
    U.fetch_xor(0x1234_5678, SeqCst);
    U.fetch_and(0xff0f_f0ff, SeqCst);
    U.fetch_or(0x0000_8001, SeqCst);
    let old = U.swap(0x7777_0000, SeqCst);
    for _ in 0..50 {
        let mut cur = C.load(SeqCst);
        while let Err(now) = C.compare_exchange_weak(cur, cur.wrapping_mul(3) ^ 1, SeqCst, SeqCst) {
            cur = now;
        }
    }
    let p = u128::from(h) * u128::from(C.load(SeqCst));
    let q = p / 0x123_4567;
    let r = p % 1_000_000_007;
    h ^= (q as u64) ^ ((q >> 64) as u64) ^ (r as u64);
    h ^= u64::from(old) << 7;
    h ^= (I.load(SeqCst) as i64 as u64).rotate_left(29);
    h ^ u64::from(U.load(SeqCst))
}
