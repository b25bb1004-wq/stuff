//! Error-case line-number tests, property tests, and corpus timing check.
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use assay::forall;
use chisel::assemble;

#[test]
fn required_error_cases_and_line_numbers() {
    let cases: &[(&str, usize, &str)] = &[
        ("nop\nfoo a0, a1", 2, "unknown mnemonic"),
        ("add r99, a0, a1", 1, "unknown register"),
        ("add a0, a1", 1, "wrong operand count"),
        ("addi a0, a1, 2048", 1, "immediate out of range"),
        ("addi a0, a1, -2049", 1, "immediate out of range neg"),
        ("lui a0, 1048576", 1, "lui out of range"),
        ("slli a0, a1, 64", 1, "shift amount out of range"),
        ("slliw a0, a1, 32", 1, "shift w out of range"),
        ("c.slli a0, 0", 1, "c.slli 0"),
        ("j missing_lbl", 1, "undefined symbol"),
        ("dup:\nnop\ndup: nop", 3, "duplicate label"),
        ("beq a0, a1, 4096", 1, "branch out of range"),
        ("beq a0, a1, 3", 1, "branch unaligned"),
        ("jal 1048576", 1, "jal out of range"),
        ("c.beqz a0, 256", 1, "c.beqz out of range"),
        ("c.j 2048", 1, "c.j out of range"),
        ("c.lw ra, 0(a0)", 1, "c.lw constraint"),
        ("c.addi a0, 0", 1, "c.addi 0 constraint"),
        ("c.addi zero, 1", 1, "c.addi x0 constraint"),
        ("c.lui sp, 1", 1, "c.lui sp constraint"),
        ("c.mv a0, zero", 1, "c.mv zero constraint"),
        ("c.addi4spn a0, a1, 4", 1, "c.addi4spn non-sp"),
        ("c.lwsp a0, 0(a1)", 1, "c.lwsp non-sp"),
        (".bogus 1, 2", 1, "unknown directive"),
        ("nop\n.macro M a\nadd \\a, \\a, \\a", 2, "missing .endm"),
        ("j 1f\nnop", 1, "Nf with no later N:"),
        ("j 1b", 1, "Nb with no earlier N:"),
        (
            "x: nop\naddi a0, a0, %pcrel_lo(x)",
            2,
            "bad %pcrel_lo non-auipc",
        ),
        (
            "x: auipc a0, 1\naddi a0, a0, %pcrel_lo(x)",
            2,
            "bad %pcrel_lo plain auipc",
        ),
        (".byte 256", 1, "data out of range .byte high"),
        (".byte -129", 1, "data out of range .byte low"),
        (".half 65536", 1, "data out of range .half"),
        (".word 4294967296", 1, "data out of range .word"),
        (".ascii \"unterminated", 1, "unterminated string"),
        ("L1: li a0, L1", 1, "li with label"),
        ("addi a0, a0, 1 / 0", 1, "div by zero"),
        (".balign 3", 1, "non-power-of-two balign"),
    ];
    for &(src, expected_line, tag) in cases {
        let err = assemble(src, 0).expect_err(tag);
        assert_eq!(err.line, expected_line, "wrong line for {tag}: {err}");
    }
}

#[test]
fn first_error_by_line_ordering() {
    // Pass 2 error on line 1 beats Pass 1 error on line 3.
    let e1 = assemble("j missing\nnop\nbad_insn a0", 0).unwrap_err();
    assert_eq!(e1.line, 1);

    // Pass 1 error on line 2 beats Pass 2 error on line 4.
    let e2 = assemble("nop\nbad_insn\nnop\nj missing", 0).unwrap_err();
    assert_eq!(e2.line, 2);

    // Error inside macro expansion reports invoking line.
    let e3 = assemble(".macro BAD r\nbad_op \\r\n.endm\nnop\nBAD a0", 0).unwrap_err();
    assert_eq!(e3.line, 5);

    // Pass 1 error on line 1 beats missing .endm on line 3.
    let e4 = assemble("bad_op\nnop\n.macro M\nnop", 0).unwrap_err();
    assert_eq!(e4.line, 1);
}

#[test]
fn prop_random_programs_deterministic_and_labels_resolve() {
    let regs = ["zero", "ra", "sp", "t0", "t1", "a0", "a1", "s0", "s1"];
    let alu = ["add", "sub", "xor", "or", "and", "slt", "sltu"];
    forall(200, 0x51a7_a021, |rng| {
        let count = rng.range(10, 45) as usize;
        let mut lines = Vec::new();
        let mut label_positions: Vec<(String, usize)> = Vec::new();
        let mut cur_off = 0usize;

        for i in 0..count {
            if rng.chance(1, 3) {
                let lbl = format!("L_{i}");
                label_positions.push((lbl.clone(), cur_off));
                lines.push(format!("{lbl}:"));
            }
            match rng.below(5) {
                0 => {
                    let op = *rng.pick(&alu);
                    let rd = *rng.pick(&regs);
                    let r1 = *rng.pick(&regs);
                    let r2 = *rng.pick(&regs);
                    lines.push(format!("  {op} {rd}, {r1}, {r2}"));
                    cur_off += 4;
                }
                1 => {
                    let rd = *rng.pick(&regs);
                    let r1 = *rng.pick(&regs);
                    let imm = (rng.range(0, 4096) as i64) - 2048;
                    lines.push(format!("  addi {rd}, {r1}, {imm}"));
                    cur_off += 4;
                }
                2 => {
                    let r = if rng.chance(1, 2) { "a0" } else { "s0" };
                    let imm = (rng.range(0, 62) as i64) - 31;
                    lines.push(format!("  c.li {r}, {imm}"));
                    cur_off += 2;
                }
                3 => {
                    let b = rng.range(0, 256);
                    lines.push(format!("  .byte {b}"));
                    cur_off += 1;
                }
                _ => {
                    let even_targets: Vec<&(String, usize)> = label_positions
                        .iter()
                        .filter(|(_, off)| (cur_off - off).is_multiple_of(2))
                        .collect();
                    if !even_targets.is_empty() {
                        let (target, _) = *rng.pick(&even_targets);
                        lines.push(format!("  beq a0, a1, {target}"));
                        cur_off += 4;
                    } else {
                        lines.push("  nop".to_string());
                        cur_off += 4;
                    }
                }
            }
        }
        let src = lines.join("\n");
        let base = 0x8000_0000u64;
        let out1 = assemble(&src, base).unwrap();
        let out2 = assemble(&src, base).unwrap();
        assert_eq!(out1, out2);
        assert_eq!(out1.bytes.len(), cur_off);
        let expected_syms: Vec<(String, u64)> = label_positions
            .into_iter()
            .map(|(n, o)| (n, base + o as u64))
            .collect();
        assert_eq!(out1.symbols, expected_syms);
    });
}

#[test]
fn corpus_x20_under_10_seconds() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus.s");
    let src = std::fs::read_to_string(path).unwrap();
    let start = Instant::now();
    for _ in 0..20 {
        let out = assemble(&src, 0x8000_0000).unwrap();
        assert!(!out.bytes.is_empty());
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "corpus x20 took {:.2}s (>= 10s)",
        elapsed.as_secs_f64()
    );
}
