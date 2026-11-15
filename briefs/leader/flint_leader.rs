//! Leader check for T24: golden transcript properties, determinism, U-mode isolation, the
//! panic path, the unknown-syscall return, and the kernel being assembly only.
#![allow(clippy::unwrap_used, clippy::panic)]
use silicon::machine::{Config, Exit, Machine, RAM_BASE};

fn task_a_body(src: &str) -> u64 {
    let out = chisel::assemble(src, RAM_BASE).unwrap();
    out.symbols
        .iter()
        .find(|(n, _)| n == "task_a_body")
        .unwrap()
        .1
}

fn run_src(src: &str, patch: Option<[u8; 4]>, steps: u64) -> (Exit, String, Machine) {
    let out = chisel::assemble(src, RAM_BASE).unwrap();
    let mut m = Machine::new(Config::default());
    m.load(RAM_BASE, &out.bytes).unwrap();
    if let Some(p) = patch {
        m.load(task_a_body(src), &p).unwrap();
    }
    let exit = m.run(steps);
    let text = String::from_utf8_lossy(&m.board.take_uart_output()).into_owned();
    (exit, text, m)
}

fn check_transcript(t: &str) {
    assert!(
        t.starts_with("lithium: boot\n") && t.ends_with("lithium: halt\n"),
        "{t:?}"
    );
    let lines: Vec<&str> = t.lines().collect();
    let body = &lines[1..lines.len() - 1];
    assert_eq!(body.len(), 10, "{t:?}");
    for task in ["A", "B"] {
        let mine: Vec<&&str> = body.iter().filter(|l| l.starts_with(task)).collect();
        let want: Vec<String> = (0..5).map(|i| format!("{task}{i}")).collect();
        assert_eq!(
            mine.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            want,
            "{t:?}"
        );
    }
    let pos = |l: &str| body.iter().position(|x| *x == l).unwrap();
    assert!(
        pos("A4") > pos("B0") && pos("B4") > pos("A0"),
        "no preemption: {t:?}"
    );
}

#[test]
fn golden_and_deterministic() {
    let (exit, t) = lithium::run(1_000_000);
    assert_eq!(exit, Exit::Pass);
    check_transcript(&String::from_utf8(t.clone()).unwrap());
    assert_eq!(
        t,
        include_bytes!("golden.txt").to_vec(),
        "golden.txt differs"
    );
    let (e2, t2, m) = run_src(lithium::KERNEL_SRC, None, 1_000_000);
    assert_eq!((e2, t2.into_bytes()), (exit, t));
    assert!(m.board.mtime() < 200_000, "took {} steps", m.board.mtime());
    let img = lithium::image().unwrap();
    assert!(!img.is_empty() && img.len() < 64 * 1024);
}

#[test]
fn user_mode_cannot_touch_machine_csrs() {
    // csrr t0, mstatus as task A's first instruction: illegal in U-mode
    let (exit, t, _) = run_src(
        lithium::KERNEL_SRC,
        Some(0x3000_22f3u32.to_le_bytes()),
        1_000_000,
    );
    assert!(t.contains("lithium: panic 0x2\n"), "{t:?}");
    assert_eq!(exit, Exit::Fail(1));
}

#[test]
fn illegal_instruction_panics() {
    let (exit, t, _) = run_src(lithium::KERNEL_SRC, Some([0; 4]), 1_000_000);
    assert!(
        t.starts_with("lithium: boot\n") && t.contains("lithium: panic 0x2\n"),
        "{t:?}"
    );
    assert!(!t.contains("lithium: halt"));
    assert_eq!(exit, Exit::Fail(1));
}

#[test]
fn unknown_syscall_returns_minus_one() {
    let probe = "task_a_body:\n    li a7, 99\n    ecall\n    li t0, -1\n    beq a0, t0, leader_ok\n    .word 0\nleader_ok:\n";
    assert_eq!(lithium::KERNEL_SRC.matches("task_a_body:\n").count(), 1);
    let src = lithium::KERNEL_SRC.replace("task_a_body:\n", probe);
    let (exit, t, _) = run_src(&src, None, 1_000_000);
    assert_eq!(exit, Exit::Pass, "{t:?}");
    check_transcript(&t);
}

#[test]
fn kernel_logic_is_assembly() {
    let lib = include_str!("../src/lib.rs");
    for bad in ["take_uart_output()", "uart_input", "press_key"] {
        let uses = lib.matches(bad).count();
        assert!(uses <= 1, "Rust side does device work: {bad}");
    }
    assert!(
        lib.lines().count() < 80,
        "lib.rs should only assemble, load and run"
    );
}
