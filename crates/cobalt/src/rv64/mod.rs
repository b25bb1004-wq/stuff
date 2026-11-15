//! Logos IR to RV64 machine code compiler and Silicon runner (integer subset).

mod emit;
mod runtime;
mod validate;

#[cfg(test)]
mod tests;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use logos::ir::Program;

/// Errors produced when lowering Logos IR to RV64 assembly or ELF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    /// Encountered an instruction, type, or construct outside the supported subset.
    Unsupported(String),
    /// Entry function was not found or did not have signature `fn() -> int`.
    NoEntry(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::Unsupported(what) => write!(f, "unsupported: {what}"),
            CodegenError::NoEntry(name) => write!(f, "missing or invalid entry '{name}'"),
        }
    }
}

impl core::error::Error for CodegenError {}

/// Base physical address of guest RAM on Silicon.
const RAM_BASE: u64 = 0x8000_0000;

/// Lower a Logos IR program to Chisel RV64IM assembly text starting at `entry`.
pub fn emit_asm(p: &Program, entry: &str) -> Result<String, CodegenError> {
    let entry_id = validate::validate_program(p, entry)?;
    Ok(emit::emit_program(p, entry_id))
}

/// Compile a Logos IR program into a little-endian RV64 ELF64 executable.
pub fn compile_elf(p: &Program, entry: &str) -> Result<Vec<u8>, CodegenError> {
    let asm = emit_asm(p, entry)?;
    let out = chisel::assemble(&asm, RAM_BASE)
        .map_err(|e| CodegenError::Unsupported(format!("assembler error: {e}")))?;
    let start_addr = out
        .symbols
        .iter()
        .find(|(name, _)| name == "_start")
        .map(|(_, addr)| *addr)
        .unwrap_or(RAM_BASE);
    let mem_size = out.bytes.len() as u64;
    let img = chisel::elf::Image {
        entry: start_addr,
        segments: alloc::vec![chisel::elf::Segment {
            vaddr: RAM_BASE,
            flags: chisel::elf::PF_R | chisel::elf::PF_X,
            data: out.bytes,
            mem_size,
        }],
        symbols: out.symbols,
    };
    chisel::elf::write(&img)
        .map_err(|e| CodegenError::Unsupported(format!("ELF writer error: {e}")))
}

/// Compile `p` to an RV64 ELF and run it on Silicon for up to `steps` ticks.
#[cfg(any(test, feature = "silicon"))]
pub fn run(p: &Program, entry: &str, steps: u64) -> (silicon::machine::Exit, String) {
    let elf = match compile_elf(p, entry) {
        Ok(bytes) => bytes,
        Err(err) => return (silicon::machine::Exit::Fail(255), format!("{err}")),
    };
    let img = match chisel::elf::read(&elf) {
        Ok(img) => img,
        Err(err) => return (silicon::machine::Exit::Fail(254), format!("{err}")),
    };
    let mut machine = silicon::machine::Machine::new(silicon::machine::Config::default());
    if let Err(err) = chisel::elf::load(&img, &mut machine) {
        return (silicon::machine::Exit::Fail(253), format!("{err:?}"));
    }
    let exit = machine.run(steps);
    let raw = machine.board.take_uart_output();
    let output = String::from_utf8_lossy(&raw).into_owned();
    (exit, output)
}
