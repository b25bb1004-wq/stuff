//! Demo: encode a tiny guest program, run it on the board, then trap a bare hart on ECALL.
//! Deterministic: no clock, no randomness, no network.

use silicon::inst::{decode, encode, Inst};
use silicon::machine::{Config, Exit, Machine, RAM_BASE};
use silicon::{Cpu, Ram, Trap};

/// Little-endian bytes of a list of instructions, ready to load into RAM.
fn image(insts: &[Inst]) -> Vec<u8> {
    insts
        .iter()
        .flat_map(|&i| encode(i).to_le_bytes())
        .collect()
}

fn main() {
    // Round trip: the encoder and decoder agree on one instruction.
    let add = Inst::Addi {
        rd: 1,
        rs1: 0,
        imm: 5,
    };
    let word = encode(add);
    println!("addi x1, x0, 5 encodes to {word:#010x}");
    println!(
        "decodes back to the same instruction: {}",
        decode(word) == Some(add)
    );

    // Guest program: print "Hi\n" on the UART, then write 0x5555 to the test finisher.
    let mut prog = vec![Inst::Lui {
        rd: 5,
        imm: 0x1000_0000,
    }];
    for byte in b"Hi\n" {
        prog.push(Inst::Addi {
            rd: 6,
            rs1: 0,
            imm: i64::from(*byte),
        });
        prog.push(Inst::Sb {
            rs1: 5,
            rs2: 6,
            imm: 0,
        });
    }
    prog.extend([
        Inst::Lui {
            rd: 7,
            imm: 0x0010_0000,
        },
        Inst::Lui { rd: 8, imm: 0x5000 },
        Inst::Addi {
            rd: 8,
            rs1: 8,
            imm: 0x555,
        },
        Inst::Sw {
            rs1: 7,
            rs2: 8,
            imm: 0,
        },
    ]);

    // A machine with 1 MiB of RAM and the default devices.
    let mut machine = Machine::new(Config {
        ram_size: 1 << 20,
        ..Config::default()
    });
    if machine.load(RAM_BASE, &image(&prog)).is_err() {
        println!("program does not fit in RAM");
        return;
    }
    let exit = machine.run(1_000);
    let text = machine.board.take_uart_output();
    println!(
        "machine run: {exit:?} after {} instructions",
        machine.cpu.instret
    );
    println!("uart output: {:?}", String::from_utf8_lossy(&text));
    println!("run ended by pass: {}", exit == Exit::Pass);

    // A bare hart on plain RAM: ECALL in machine mode comes back as a trap value.
    let mut ram = Ram::new(RAM_BASE, 4096);
    let bare = image(&[
        Inst::Addi {
            rd: 1,
            rs1: 0,
            imm: 5,
        },
        Inst::Ecall,
    ]);
    if ram.load(RAM_BASE, &bare).is_err() {
        println!("bare program does not fit");
        return;
    }
    let mut cpu = Cpu::new(RAM_BASE);
    let result = cpu.run(&mut ram, 10);
    println!("bare hart: x1 = {}, instret = {}", cpu.x[1], cpu.instret);
    println!(
        "bare hart: run returned {result:?}, is ecall: {}",
        result == Err(Trap::Ecall)
    );
}
