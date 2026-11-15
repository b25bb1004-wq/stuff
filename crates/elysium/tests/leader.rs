//! Leader-run checks with fresh seeds: decoder and verifier never panic on hostile bytes.
#![allow(clippy::unwrap_used)]
use assay::forall;
use elysium::{decode, encode, verify, Const, Func, Module, Op};

fn sample() -> Module {
    let code = vec![
        Op::Int(0, -7),
        Op::Const(1, 0),
        Op::Byte(2, 200),
        Op::Bool(3, true),
        Op::JumpIfNot(3, 6),
        Op::Call(4, 1, 0, 1),
        Op::CallHost(4, 0, 1, 1),
        Op::NewVariant(5, 1, 0, 2),
        Op::Slice(5, 1, 0, 0),
        Op::Assert(3, 0),
        Op::Ret(0),
    ];
    Module {
        consts: vec![Const::Str("héllo".into()), Const::Bytes(vec![0, 255, 7])],
        imports: vec!["io.print".into()],
        funcs: vec![
            Func {
                name: "main".into(),
                params: 0,
                regs: 8,
                code,
            },
            Func {
                name: "id".into(),
                params: 1,
                regs: 1,
                code: vec![Op::Ret(0)],
            },
        ],
        entry: 0,
    }
}

#[test]
fn sample_round_trips_and_verifies() {
    let m = sample();
    assert_eq!(verify(&m), Ok(()));
    assert_eq!(decode(&encode(&m)), Ok(m));
}

#[test]
fn hostile_bytes_never_panic_decode_or_verify() {
    let good = encode(&sample());
    forall(40_000, 0x5712_A7A1, |rng| {
        let mut b = good.clone();
        match rng.below(4) {
            0 => b = rng.bytes(96),
            1 => b.truncate(rng.below(good.len() + 1)),
            2 => {
                for _ in 0..=rng.below(4) {
                    let i = rng.below(b.len());
                    b[i] ^= 1 << rng.below(8);
                }
            }
            _ => {
                let i = rng.below(b.len());
                b[i] = rng.below(256) as u8;
                let tail = rng.bytes(8);
                b.extend_from_slice(&tail);
            }
        }
        if let Ok(m) = decode(&b) {
            let _ = verify(&m);
            let total: usize = m.funcs.iter().map(|f| f.code.len()).sum();
            assert!(total <= b.len(), "more instructions than input bytes");
            assert_eq!(
                decode(&encode(&m)),
                Ok(m),
                "decoded module must re-encode stably"
            );
        }
    });
}
