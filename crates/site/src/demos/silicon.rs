//! Silicon RV64 machine emulator demo.

use std::fmt::Write as _;

use silicon::machine::{Config, Exit, Machine, RAM_BASE};

use super::{check_input, Demo, Output};

const RAM_SIZE: usize = 1024 * 1024;
const MAX_STEPS: u64 = 1_000_000;

const DEFAULT_ASM: &str = "\
    li t0, 0x10000000
    la t1, msg
loop:
    lbu t2, 0(t1)
    beqz t2, done
    sb t2, 0(t0)
    addi t1, t1, 1
    j loop
done:
    li a0, 42
    li a1, 100
    li a2, 0x1234
    li t0, 0x00100000
    li t1, 0x5555
    sw t1, 0(t0)
halt:
    j halt
msg:
    .asciz \"hello from silicon\\n\"";

/// Registered Silicon RV64 machine demo.
pub const DEMO: Demo = Demo {
    id: "silicon",
    title: "Silicon RV64",
    default_input: DEFAULT_ASM,
    run,
};

/// Assemble RV64 source, load into a 1 MiB Silicon machine, and run up to 1 M steps.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    if input.trim().is_empty() {
        return Err(String::from("empty assembly source"));
    }
    let assembled = chisel::assemble(input, RAM_BASE).map_err(|e| e.to_string())?;
    if assembled.bytes.is_empty() {
        return Err(String::from("assembly produced 0 bytes"));
    }
    if assembled.bytes.len() > RAM_SIZE {
        return Err(format!(
            "program size {} exceeds 1 MiB RAM limit",
            assembled.bytes.len()
        ));
    }

    let cfg = Config {
        ram_size: RAM_SIZE,
        disk: Vec::new(),
        fb_width: 64,
        fb_height: 64,
        decode_cache: true,
    };
    let mut machine = Machine::new(cfg);
    machine
        .load(RAM_BASE, &assembled.bytes)
        .map_err(|f| format!("load failed: {f:?}"))?;

    let exit = machine.run(MAX_STEPS);
    let uart = machine.board.take_uart_output();

    let mut out = String::new();
    let uart_text = String::from_utf8_lossy(&uart);
    if uart_text.is_empty() {
        out.push_str("uart: (empty)\n");
    } else {
        out.push_str("uart:\n");
        out.push_str(&uart_text);
        if !uart_text.ends_with('\n') {
            out.push('\n');
        }
    }

    let exit_str = match exit {
        Exit::Pass => String::from("Pass"),
        Exit::Fail(code) => format!("Fail({code})"),
        Exit::OutOfSteps => format!("OutOfSteps ({MAX_STEPS} steps)"),
    };
    let _ = writeln!(
        out,
        "exit: {exit_str} | instret: {} | pc: 0x{:016x}",
        machine.cpu.instret, machine.cpu.pc
    );

    for i in 0..8 {
        let reg_idx = 10 + i;
        let val = machine.cpu.x[reg_idx];
        let _ = writeln!(out, "a{i} (x{reg_idx}) = 0x{val:016x} ({val})");
    }

    Ok(Output::Text(out))
}
