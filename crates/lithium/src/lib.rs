//! lithium: a two-task RV64 kernel written in assembly, run on the silicon machine.
#![forbid(unsafe_code)]

use silicon::machine::{Config, Exit, Machine, RAM_BASE};

/// Kernel assembly source, assembled at `RAM_BASE`.
pub const KERNEL_SRC: &str = include_str!("../kernel/kernel.s");

/// Assembled kernel bytes, to be loaded at `RAM_BASE`.
pub fn image() -> Result<Vec<u8>, chisel::AsmError> {
    chisel::assemble(KERNEL_SRC, RAM_BASE).map(|out| out.bytes)
}

/// A machine with the kernel loaded at `RAM_BASE` and the pc there.
pub fn boot(cfg: Config) -> Result<Machine, String> {
    let bytes = image().map_err(|e| e.to_string())?;
    let mut machine = Machine::new(cfg);
    machine
        .load(RAM_BASE, &bytes)
        .map_err(|f| format!("{f:?}"))?;
    Ok(machine)
}

/// Boot a default machine, run up to `steps`, and return the exit and UART output.
/// If the kernel cannot be assembled or loaded, nothing runs: `(Exit::OutOfSteps, vec![])`.
pub fn run(steps: u64) -> (Exit, Vec<u8>) {
    match boot(Config::default()) {
        Ok(mut machine) => {
            let exit = machine.run(steps);
            (exit, machine.board.take_uart_output())
        }
        Err(_) => (Exit::OutOfSteps, Vec::new()),
    }
}
