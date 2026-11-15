//! Runnable demo for the chisel RV64IMAC assembler and ELF64 helper.

use chisel::assemble;
use chisel::elf::{self, Image, Segment, PF_R, PF_X};

fn main() {
    let base = 0x1_0000u64;
    let src = "\
        .equ COUNT, 5\n\
    _start:\n\
        li a0, 0\n\
        li a1, COUNT\n\
    loop:\n\
        add a0, a0, a1\n\
        addi a1, a1, -1\n\
        bnez a1, loop\n\
        ebreak\n\
    data:\n\
        .word 42\n\
    ";

    println!("assembling RV64IMAC program at 0x{base:x}");
    let out = match assemble(src, base) {
        Ok(out) => out,
        Err(err) => {
            eprintln!("assembly failed: {err}");
            std::process::exit(1);
        }
    };

    println!("assembled {} bytes", out.bytes.len());
    for (name, addr) in &out.symbols {
        println!("symbol {name} = 0x{addr:x}");
    }

    let bad_src = "addi x1, x2, 0\nbadinst x1, x2\n";
    if let Err(err) = assemble(bad_src, base) {
        println!("expected error on bad input: {err}");
    } else {
        eprintln!("expected assembly error on bad input");
        std::process::exit(1);
    }

    let seg = Segment {
        vaddr: base,
        flags: PF_R | PF_X,
        data: out.bytes.clone(),
        mem_size: out.bytes.len() as u64,
    };
    let image = Image {
        entry: base,
        segments: vec![seg],
        symbols: out.symbols.clone(),
    };

    let elf_bytes = match elf::write(&image) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("ELF write failed: {err}");
            std::process::exit(1);
        }
    };
    println!("wrote ELF64 image ({} bytes)", elf_bytes.len());

    let parsed = match elf::read(&elf_bytes) {
        Ok(img) => img,
        Err(err) => {
            eprintln!("ELF read failed: {err}");
            std::process::exit(1);
        }
    };

    println!(
        "parsed ELF64: entry=0x{:x}, segments={}, symbols={}",
        parsed.entry,
        parsed.segments.len(),
        parsed.symbols.len()
    );
    assert_eq!(parsed, image);
    println!("ELF64 round-trip verified");
}
