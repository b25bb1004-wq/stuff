#![allow(clippy::unwrap_used)]
//! Behaviour tests for the lithium kernel on silicon.

use assay::forall;
use lithium::{boot, image, run};
use silicon::machine::{Config, Exit};

const GOLDEN: &[u8] = include_bytes!("golden.txt");

#[test]
fn golden_boot() {
    let (exit, out) = run(1_000_000);
    assert_eq!(exit, Exit::Pass);
    assert_eq!(out, GOLDEN, "transcript differs from tests/golden.txt");

    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("lithium: boot\n"));
    assert!(text.ends_with("lithium: halt\n"));
    let lines: Vec<&str> = text.lines().collect();
    for letter in ["A", "B"] {
        for i in 0..5 {
            let want = format!("{letter}{i}");
            assert_eq!(lines.iter().filter(|l| **l == want).count(), 1, "{want}");
        }
    }
    let a: Vec<&&str> = lines.iter().filter(|l| l.starts_with('A')).collect();
    let b: Vec<&&str> = lines.iter().filter(|l| l.starts_with('B')).collect();
    assert_eq!(a, ["A0", "A1", "A2", "A3", "A4"].iter().collect::<Vec<_>>());
    assert_eq!(b, ["B0", "B1", "B2", "B3", "B4"].iter().collect::<Vec<_>>());
    let first_a = lines.iter().position(|l| l.starts_with('A')).unwrap();
    let first_b = lines.iter().position(|l| l.starts_with('B')).unwrap();
    let last_a = lines.iter().rposition(|l| l.starts_with('A')).unwrap();
    let last_b = lines.iter().rposition(|l| l.starts_with('B')).unwrap();
    assert!(first_b < last_a, "no A line after a B line");
    assert!(first_a < last_b, "no B line after an A line");
}

#[test]
fn deterministic() {
    let finish = |mut m: silicon::machine::Machine| {
        for _ in 0..1_000_000 {
            if m.run(1) != Exit::OutOfSteps {
                break;
            }
        }
        (m.board.take_uart_output(), m.board.mtime())
    };
    let one = finish(boot(Config::default()).unwrap());
    let two = finish(boot(Config::default()).unwrap());
    assert_eq!(one, two);
    assert_eq!(one.0, GOLDEN);
}

#[test]
fn output_is_prefix_at_any_step_count() {
    let full = GOLDEN;
    forall(40, 0xF11A7, |rng| {
        let steps = rng.range(1, 40_000);
        let (_, out) = run(steps);
        assert!(full.starts_with(&out), "not a prefix after {steps} steps");
    });
}

#[test]
fn image_shape() {
    let bytes = image().unwrap();
    assert!(!bytes.is_empty());
    assert!(bytes.len() < 64 * 1024);
    let out = chisel::assemble(lithium::KERNEL_SRC, silicon::machine::RAM_BASE).unwrap();
    assert!(out.symbols.iter().any(|(name, _)| name == "task_a_body"));
}
