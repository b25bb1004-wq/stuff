//! Chisel RV64IMAC assembler demo.

use std::fmt::Write as _;

use super::{check_input, Demo, Output};

const BASE_ADDR: u64 = 0x8000_0000;
const MAX_DUMP_BYTES: usize = 8 * 1024;

const DEFAULT_ASM: &str = "\
start:
    addi a0, zero, 42
    addi a1, zero, 13
    add  a2, a0, a1
    ebreak";

/// Registered Chisel RV64 assembler demo.
pub const DEMO: Demo = Demo {
    id: "chisel",
    title: "Chisel Assembler",
    default_input: DEFAULT_ASM,
    run,
};

/// Assemble RV64 source at `0x8000_0000` and return an addressed hex dump.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    if input.trim().is_empty() {
        return Err(String::from("empty assembly source"));
    }
    let assembled = chisel::assemble(input, BASE_ADDR).map_err(|e| e.to_string())?;
    if assembled.bytes.is_empty() {
        return Err(String::from("assembly produced 0 bytes"));
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "assembled {} bytes at 0x{BASE_ADDR:08x}",
        assembled.bytes.len()
    );

    let shown = assembled.bytes.len().min(MAX_DUMP_BYTES);
    for (row_idx, chunk) in assembled.bytes[..shown].chunks(16).enumerate() {
        let addr = BASE_ADDR + (row_idx as u64) * 16;
        let _ = write!(out, "0x{addr:08x}: ");
        for i in 0..16 {
            if let Some(b) = chunk.get(i) {
                let _ = write!(out, "{b:02x} ");
            } else {
                out.push_str("   ");
            }
        }
        out.push('|');
        for &b in chunk {
            let ch = if (0x20..=0x7e).contains(&b) {
                char::from(b)
            } else {
                '.'
            };
            out.push(ch);
        }
        out.push_str("|\n");
    }
    if assembled.bytes.len() > shown {
        let _ = writeln!(out, "... ({} more bytes)", assembled.bytes.len() - shown);
    }
    if !assembled.symbols.is_empty() {
        out.push_str("symbols:\n");
        for (name, addr) in assembled.symbols.iter().take(64) {
            let _ = writeln!(out, "  {name} = 0x{addr:08x}");
        }
    }
    Ok(Output::Text(out))
}
