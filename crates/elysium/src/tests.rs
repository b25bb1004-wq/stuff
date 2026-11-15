//! Round trips, decoder fuzzing and directed decoder tests.

#![allow(clippy::unwrap_used)]

use crate::op::Op;
use crate::opcode;
use crate::testgen::random_module;
use crate::{decode, encode, verify, Const, DecodeError, Func, Module};
use assay::forall;

fn total_ops(m: &Module) -> usize {
    m.funcs.iter().map(|f| f.code.len()).sum()
}

/// Decoding hostile bytes must return `Ok` or `Err`; a decoded module must stay small and verifiable without panics.
fn check_hostile(b: &[u8]) {
    if let Ok(m) = decode(b) {
        assert!(total_ops(&m) <= b.len(), "more instructions than bytes");
        let _ = verify(&m);
    }
}

#[test]
fn generated_modules_verify_and_round_trip_and_cover_every_op() {
    let mut seen = [false; opcode::COUNT as usize];
    forall(3000, 1001, |r| {
        let m = random_module(r);
        assert_eq!(verify(&m), Ok(()));
        for f in &m.funcs {
            for op in &f.code {
                seen[usize::from(opcode::of(op))] = true;
            }
        }
        assert_eq!(decode(&encode(&m)), Ok(m));
    });
    let missing: Vec<usize> = (0..seen.len()).filter(|&k| !seen[k]).collect();
    assert!(missing.is_empty(), "op kinds never generated: {missing:?}");
}

#[test]
fn extreme_operands_round_trip() {
    let m = Module {
        consts: vec![Const::Str(String::new()), Const::Bytes(vec![0xff])],
        imports: vec!["io.print".into()],
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            regs: 2,
            code: vec![
                Op::Int(0, i64::MIN),
                Op::Int(1, i64::MAX),
                Op::Byte(0, u8::MAX),
                Op::Bool(1, true),
                Op::Trap(u32::MAX),
            ],
        }],
        entry: 0,
    };
    assert_eq!(decode(&encode(&m)), Ok(m));
}

#[test]
fn random_bytes_never_panic() {
    forall(3000, 2002, |r| {
        let mut b = vec![0u8; r.below(300)];
        r.fill(&mut b);
        if b.len() >= 4 && r.chance(1, 2) {
            b[..4].copy_from_slice(b"MBC1");
        }
        check_hostile(&b);
    });
}

#[test]
fn bit_flipped_encodings_never_panic() {
    forall(3000, 3003, |r| {
        let mut b = encode(&random_module(r));
        for _ in 0..1 + r.below(4) {
            let i = r.below(b.len());
            b[i] ^= 1u8 << r.below(8);
        }
        check_hostile(&b);
    });
}

#[test]
fn truncated_encodings_are_errors() {
    forall(3000, 4004, |r| {
        let b = encode(&random_module(r));
        for n in 0..b.len() {
            assert!(decode(&b[..n]).is_err(), "prefix of {n} bytes decoded");
        }
    });
}

#[test]
fn sixty_four_byte_inputs_hold_at_most_64_instructions() {
    forall(3000, 5005, |r| {
        let mut b = vec![0u8; 64];
        r.fill(&mut b);
        if r.chance(3, 4) {
            b[..4].copy_from_slice(b"MBC1");
        }
        if let Ok(m) = decode(&b) {
            assert!(total_ops(&m) <= 64);
        }
        check_hostile(&b);
    });
}

#[test]
fn bad_magic_and_short_input() {
    assert_eq!(decode(b"MBC2\0\0\0\0"), Err(DecodeError::BadMagic));
    assert_eq!(decode(b"MB"), Err(DecodeError::Truncated));
    assert_eq!(decode(b""), Err(DecodeError::Truncated));
}

#[test]
fn huge_count_is_refused_before_allocating() {
    let mut b = b"MBC1".to_vec();
    b.extend([0xff, 0xff, 0xff, 0xff, 0x0f]); // constant count u32::MAX, nothing after it
    assert_eq!(decode(&b), Err(DecodeError::TooLarge));
}

#[test]
fn invalid_utf8_name_is_refused() {
    // no constants, one import of length 1 whose byte is 0xff
    let b = [b'M', b'B', b'C', b'1', 0, 1, 1, 0xff, 0, 0];
    assert_eq!(decode(&b), Err(DecodeError::BadUtf8));
}

#[test]
fn unknown_opcode_is_refused() {
    // one function "f" with one instruction whose opcode is 200
    let b = [b'M', b'B', b'C', b'1', 0, 0, 1, 1, b'f', 0, 1, 1, 200, 0];
    assert_eq!(decode(&b), Err(DecodeError::BadOpcode(200)));
}

#[test]
fn overlong_operand_is_refused() {
    // Ret whose register is 0x80 0x00 (a redundant zero group)
    let b = [
        b'M', b'B', b'C', b'1', 0, 0, 1, 1, b'f', 0, 1, 1, 34, 0x80, 0x00, 0,
    ];
    assert_eq!(decode(&b), Err(DecodeError::Overlong));
}

#[test]
fn decode_does_not_verify() {
    // Ret(5) in a function with 1 register: invalid, but it encodes and decodes unchanged.
    let m = Module {
        consts: vec![],
        imports: vec![],
        funcs: vec![Func {
            name: "f".into(),
            params: 0,
            regs: 1,
            code: vec![Op::Ret(5)],
        }],
        entry: 0,
    };
    assert!(verify(&m).is_err());
    assert_eq!(decode(&encode(&m)), Ok(m));
}
