//! Seeded fuzz soak for every onyx function that parses or opens bytes.
//!
//! Half the cases are random bytes, half are mutations of valid inputs that the crate itself
//! produced. Tampered input must return `Err`; nothing may panic or take over a second.
//! Env: `STRATA_SOAK_SEED` (hex with `0x`, or decimal; default `0x5EED_79D`) and
//! `STRATA_SOAK_MS` (budget per target in milliseconds; default 300). Run with `--nocapture`
//! to see the seed and case counts. Every input is at most a few KiB, so no case comes near
//! the 64 MiB allocation limit.
#![allow(clippy::unwrap_used)]

use assay::{hex, Rng};
use onyx::{aead, ed25519, hkdf, sealed, x25519, Error};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const DEFAULT_SEED: u64 = 0x05EE_D79D;
const DEFAULT_MS: u64 = 300;
const MAX_CASE_MS: u64 = 1000;
const POOL: usize = 4;
/// Low-order u-coordinates from RFC 7748 section 6.1 (the crate's own tests use these too).
const LOW_ORDER: [&str; 3] = [
    "0000000000000000000000000000000000000000000000000000000000000000",
    "e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800",
    "5f9c95bca3508c24b1d0b1559c83ef5b04445cc4581c8e86d8224eddd09f1157",
];

type Target = fn(&mut Rng, Duration, usize) -> u64;
/// One valid AEAD box with the key, nonce and AAD that opened it.
type AeadCase = ([u8; 32], [u8; 12], Vec<u8>, Vec<u8>);
const TARGETS: [(&str, Target); 5] = [
    ("sealed::open", sealed_target as Target),
    ("aead::open", aead_target as Target),
    ("ed25519 verify and keys", ed25519_target as Target),
    ("x25519::shared_secret", x25519_target as Target),
    ("hkdf extract and expand", hkdf_target as Target),
];

/// Index of the target now running, for the failure report.
static TARGET: AtomicUsize = AtomicUsize::new(0);
/// Seed of the case now running, so a failure can be replayed.
static CASE_SEED: AtomicU64 = AtomicU64::new(0);
/// Milliseconds since `START` when the running case began; 0 while idle.
static CASE_BEGUN_MS: AtomicU64 = AtomicU64::new(0);
static START: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

fn env_seed() -> u64 {
    match std::env::var("STRATA_SOAK_SEED") {
        Ok(s) => {
            let t = s.trim().replace('_', "");
            match t.strip_prefix("0x").or(t.strip_prefix("0X")) {
                Some(h) => u64::from_str_radix(h, 16).expect("STRATA_SOAK_SEED hex"),
                None => t.parse().expect("STRATA_SOAK_SEED decimal"),
            }
        }
        Err(_) => DEFAULT_SEED,
    }
}

fn env_ms() -> u64 {
    std::env::var("STRATA_SOAK_MS")
        .map(|s| s.trim().parse().expect("STRATA_SOAK_MS"))
        .unwrap_or(DEFAULT_MS)
}

/// Print the failing target and case seed before the panic message.
fn install_hook() {
    std::panic::set_hook(Box::new(|info| {
        CASE_BEGUN_MS.store(0, Ordering::SeqCst);
        let idx = TARGET.load(Ordering::SeqCst);
        eprintln!(
            "SOAK FAILURE target {} case_seed {:#x}: {info}",
            TARGETS[idx].0,
            CASE_SEED.load(Ordering::SeqCst)
        );
    }));
}

/// Kill the process if one case runs past the per-case limit (a hang never returns to assert).
fn watchdog() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(20));
        let begun = CASE_BEGUN_MS.load(Ordering::SeqCst);
        if begun != 0 && now_ms().saturating_sub(begun) > MAX_CASE_MS {
            eprintln!(
                "SOAK HANG target {} case_seed {:#x} ran over {MAX_CASE_MS} ms",
                TARGETS[TARGET.load(Ordering::SeqCst)].0,
                CASE_SEED.load(Ordering::SeqCst)
            );
            std::process::exit(101);
        }
    });
}

/// Run `case` on fresh streams derived from `master` until `budget` is spent (at least once).
fn drive(idx: usize, budget: Duration, master: &mut Rng, mut case: impl FnMut(&mut Rng)) -> u64 {
    TARGET.store(idx, Ordering::SeqCst);
    let start = Instant::now();
    let mut cases = 0u64;
    loop {
        let case_seed = master.u64();
        CASE_SEED.store(case_seed, Ordering::SeqCst);
        CASE_BEGUN_MS.store(now_ms(), Ordering::SeqCst);
        let began = Instant::now();
        case(&mut Rng::new(case_seed));
        CASE_BEGUN_MS.store(0, Ordering::SeqCst);
        assert!(
            began.elapsed() < Duration::from_millis(MAX_CASE_MS),
            "case {case_seed:#x} took {:?}",
            began.elapsed()
        );
        cases += 1;
        if start.elapsed() >= budget {
            return cases;
        }
    }
}

/// Mutate `base` (a valid input) by bit flips, truncation, a splice of `other`, a repeated
/// chunk, or one overwritten byte. The result may equal `base`; callers check that.
fn mutate(r: &mut Rng, base: &[u8], other: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    match r.below(5) {
        0 => {
            for _ in 0..=r.below(3) {
                if !v.is_empty() {
                    let bit = r.below(v.len() * 8);
                    v[bit / 8] ^= 1 << (bit % 8);
                }
            }
        }
        1 => {
            let cut = r.below(v.len() + 1);
            v.truncate(cut);
        }
        2 => {
            let piece = &other[..r.below(other.len() + 1)];
            let at = r.below(v.len() + 1);
            v.splice(at..at, piece.iter().copied());
        }
        3 => {
            let start = r.below(v.len() + 1);
            let len = r.below(v.len() - start + 1);
            let chunk = v[start..start + len].to_vec();
            let at = r.below(v.len() + 1);
            v.splice(at..at, chunk);
        }
        _ => {
            if !v.is_empty() {
                let i = r.below(v.len());
                v[i] = r.u64() as u8;
            }
        }
    }
    v
}

fn arr32(r: &mut Rng) -> [u8; 32] {
    let mut a = [0u8; 32];
    r.fill(&mut a);
    a
}

fn arr12(r: &mut Rng) -> [u8; 12] {
    let mut a = [0u8; 12];
    r.fill(&mut a);
    a
}

fn arr64(r: &mut Rng) -> [u8; 64] {
    let mut a = [0u8; 64];
    r.fill(&mut a);
    a
}

fn low_order(i: usize) -> [u8; 32] {
    let mut a = [0u8; 32];
    a.copy_from_slice(&hex(LOW_ORDER[i]));
    a
}

fn sealed_target(master: &mut Rng, budget: Duration, idx: usize) -> u64 {
    let secret = arr32(master);
    let public = x25519::public_key(&secret);
    let pool: Vec<Vec<u8>> = (0..POOL)
        .map(|_| {
            let msg = master.bytes(256);
            sealed::seal(&public, &arr32(master), &msg)
        })
        .collect();
    drive(idx, budget, master, |r| {
        let msg = r.bytes(512);
        let boxed = sealed::seal(&public, &arr32(r), &msg);
        assert_eq!(sealed::open(&secret, &boxed).unwrap(), msg);

        let base = r.pick(&pool);
        let other = r.pick(&pool);
        let bad = mutate(r, base, other);
        if bad != base.as_slice() {
            assert!(sealed::open(&secret, &bad).is_err(), "tampered box opened");
        }

        // Low-order ephemeral key: the shared secret is all zero, so `open` must say WeakKey.
        let mut forged = low_order(r.below(3)).to_vec();
        forged.extend_from_slice(&[0u8; 16]);
        forged.extend(r.bytes(64));
        assert_eq!(sealed::open(&secret, &forged), Err(Error::WeakKey));

        let junk = r.bytes(200);
        let _ = sealed::open(&secret, &junk);
        let short = vec![0u8; r.below(sealed::OVERHEAD)];
        assert_eq!(sealed::open(&secret, &short), Err(Error::BadLength));
    })
}

fn aead_target(master: &mut Rng, budget: Duration, idx: usize) -> u64 {
    let pool: Vec<AeadCase> = (0..POOL)
        .map(|_| {
            let key = arr32(master);
            let nonce = arr12(master);
            let aad = master.bytes(64);
            let msg = master.bytes(256);
            let boxed = aead::seal(&key, &nonce, &aad, &msg);
            (key, nonce, aad, boxed)
        })
        .collect();
    drive(idx, budget, master, |r| {
        let (k, n) = (arr32(r), arr12(r));
        let a = r.bytes(128);
        let msg = r.bytes(512);
        let boxed = aead::seal(&k, &n, &a, &msg);
        assert_eq!(aead::open(&k, &n, &a, &boxed).unwrap(), msg);

        // A flipped tag bit is always BadTag.
        let mut tagged = boxed.clone();
        let at = tagged.len() - 16 + r.below(16);
        tagged[at] ^= 1 << r.below(8);
        assert_eq!(aead::open(&k, &n, &a, &tagged), Err(Error::BadTag));

        // Mutated pool boxes never open, and neither does a box with one AAD bit flipped.
        let (pk, pn, paad, pbox) = r.pick(&pool);
        let other = &r.pick(&pool).3;
        let bad = mutate(r, pbox, other);
        if bad != pbox.as_slice() {
            assert!(
                aead::open(pk, pn, paad, &bad).is_err(),
                "tampered box opened"
            );
        }
        if !paad.is_empty() {
            let mut aad2 = paad.clone();
            let i = r.below(aad2.len());
            aad2[i] ^= 1;
            assert_eq!(aead::open(pk, pn, &aad2, pbox), Err(Error::BadTag));
        }

        let short = vec![0u8; r.below(16)];
        assert_eq!(aead::open(&k, &n, &a, &short), Err(Error::BadLength));
        let junk = r.bytes(300);
        let _ = aead::open(&k, &n, &a, &junk);
    })
}

fn ed25519_target(master: &mut Rng, budget: Duration, idx: usize) -> u64 {
    let pool: Vec<([u8; 32], Vec<u8>, [u8; 64])> = (0..POOL)
        .map(|_| {
            let seed = arr32(master);
            let msg = master.bytes(128);
            (
                ed25519::public_key(&seed),
                msg.clone(),
                ed25519::sign(&seed, &msg),
            )
        })
        .collect();
    drive(idx, budget, master, |r| {
        let seed = arr32(r);
        let public = ed25519::public_key(&seed);
        let msg = r.bytes(256);
        let sig = ed25519::sign(&seed, &msg);
        assert_eq!(ed25519::verify(&public, &msg, &sig), Ok(()));

        // Any change to the message or to one signature bit fails.
        let mut longer = msg.clone();
        longer.push(r.u64() as u8);
        assert_eq!(
            ed25519::verify(&public, &longer, &sig),
            Err(Error::BadSignature)
        );
        let mut sig2 = sig;
        let bit = r.below(512);
        sig2[bit / 8] ^= 1 << (bit % 8);
        assert!(
            ed25519::verify(&public, &msg, &sig2).is_err(),
            "tampered signature verified"
        );

        // S at or above L is rejected before any point work.
        let mut big_s = sig;
        big_s[32..].fill(0xff);
        assert_eq!(
            ed25519::verify(&public, &msg, &big_s),
            Err(Error::BadSignature)
        );

        // The zero key (low order) never verifies a signature made by another key.
        assert!(ed25519::verify(&[0u8; 32], &msg, &sig).is_err());

        // Random keys and signatures, and pooled keys with a flipped bit: no panic.
        let _ = ed25519::verify(&arr32(r), &msg, &arr64(r));
        let (pp, pm, ps) = r.pick(&pool);
        let mut tweaked = *pp;
        let tb = r.below(256);
        tweaked[tb / 8] ^= 1 << (tb % 8);
        let _ = ed25519::verify(&tweaked, pm, ps);
        let _ = ed25519::verify(pp, &msg, ps);
    })
}

fn x25519_target(master: &mut Rng, budget: Duration, idx: usize) -> u64 {
    drive(idx, budget, master, |r| {
        let secret = arr32(r);
        let peer = arr32(r);
        if let Err(e) = x25519::shared_secret(&secret, &peer) {
            assert_eq!(e, Error::WeakKey);
        }

        // Diffie-Hellman is symmetric on valid keys, and the result matches on both sides.
        let other = arr32(r);
        let pa = x25519::public_key(&secret);
        let pb = x25519::public_key(&other);
        assert_eq!(
            x25519::shared_secret(&secret, &pb),
            x25519::shared_secret(&other, &pa)
        );

        assert_eq!(
            x25519::shared_secret(&secret, &low_order(r.below(3))),
            Err(Error::WeakKey)
        );

        let mut tweaked = pb;
        let bit = r.below(256);
        tweaked[bit / 8] ^= 1 << (bit % 8);
        let _ = x25519::shared_secret(&secret, &tweaked);
    })
}

fn hkdf_target(master: &mut Rng, budget: Duration, idx: usize) -> u64 {
    drive(idx, budget, master, |r| {
        let salt = r.bytes(300);
        let ikm = r.bytes(300);
        let prk = hkdf::extract(&salt, &ikm);
        assert_eq!(hkdf::extract(&salt, &ikm), prk);

        // Output lengths 0..=8200: past 255 * 32 must be BadLength, otherwise a prefix-consistent
        // stream.
        let info = r.bytes(100);
        let n = r.below(255 * 32 + 41);
        let mut okm = vec![0u8; n];
        let res = hkdf::expand(&prk, &info, &mut okm);
        if n > 255 * 32 {
            assert_eq!(res, Err(Error::BadLength));
        } else {
            assert_eq!(res, Ok(()));
            let m = r.below(n + 1);
            let mut short = vec![0u8; m];
            hkdf::expand(&prk, &info, &mut short).unwrap();
            assert_eq!(short[..], okm[..m]);
        }
    })
}

#[test]
fn soak_every_parser() {
    let seed = env_seed();
    let ms = env_ms();
    let budget = Duration::from_millis(ms);
    START.get_or_init(Instant::now);
    install_hook();
    watchdog();
    for (idx, (name, run)) in TARGETS.iter().enumerate() {
        let mut master = Rng::new(seed.wrapping_add(idx as u64 * 0x9E37_79B9));
        let cases = run(&mut master, budget, idx);
        println!("soak {name}: seed {seed:#x} cases {cases} budget {ms} ms");
    }
}
