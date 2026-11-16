//! `silicon`: runs an RV64 ELF program on the Silicon machine and prints its UART to stdout.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::process::ExitCode;

use silicon::machine::{Config, Exit, Machine};

// The ELF reader and loader live in chisel, which depends on silicon, so this tool includes the
// same source file instead of copying it (silicon cannot depend on chisel).
#[allow(dead_code)]
#[path = "../../../chisel/src/elf.rs"]
mod elf;

const VERSION: &str = "silicon 0.1.0";
const DEFAULT_STEPS: u64 = 10_000_000;

const USAGE: &str = "\
usage: silicon run FILE.elf [--steps N] [--trace]
       silicon --help | --version

Commands:
  run FILE.elf [--steps N] [--trace]
        Load the RV64 ELF at its segment addresses, start at its entry point and
        run until the test finisher is written or N steps pass (default 10000000).
        UART output goes to stdout. Exit 0 on Pass, 1 on Fail, timeout or bad ELF.
        --trace   print each pc to stderr before its step

Example:
  silicon run hello.elf --steps 1000000
";

struct Run {
    path: String,
    steps: u64,
    trace: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("--version") => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Some("run") => match parse_run(&args[1..]) {
            Ok(run) => match execute(&run) {
                Ok(code) => code,
                Err(msg) => {
                    eprintln!("silicon: {msg}");
                    ExitCode::from(1)
                }
            },
            Err(msg) => usage_error(&msg),
        },
        Some(other) => usage_error(&format!("unknown command `{other}`")),
        None => usage_error("missing command"),
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("silicon: {msg}");
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn parse_run(args: &[String]) -> Result<Run, String> {
    let mut path = None;
    let mut steps = DEFAULT_STEPS;
    let mut trace = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--steps" => {
                let value = it.next().ok_or("--steps needs a number")?;
                steps = value
                    .parse()
                    .map_err(|_| format!("invalid step count `{value}`"))?;
            }
            "--trace" => trace = true,
            opt if opt.starts_with('-') => return Err(format!("unknown option `{opt}`")),
            file => {
                if path.replace(file.to_owned()).is_some() {
                    return Err(String::from("run takes one FILE.elf"));
                }
            }
        }
    }
    let path = path.ok_or("run needs FILE.elf")?;
    Ok(Run { path, steps, trace })
}

fn execute(run: &Run) -> Result<ExitCode, String> {
    let bytes = std::fs::read(&run.path).map_err(|e| format!("{}: {e}", run.path))?;
    let img = elf::read(&bytes).map_err(|e| format!("{}: {e}", run.path))?;
    let mut machine = Machine::new(Config::default());
    elf::load(&img, &mut machine).map_err(|f| format!("{}: load failed: {f:?}", run.path))?;
    machine.cpu.pc = img.entry;

    let mut stdout = io::stdout().lock();
    let mut exit = Exit::OutOfSteps;
    for _ in 0..run.steps {
        if run.trace {
            eprintln!("pc 0x{:016x}", machine.cpu.pc);
        }
        exit = machine.run(1);
        let uart = machine.board.take_uart_output();
        stdout
            .write_all(&uart)
            .map_err(|e| format!("stdout: {e}"))?;
        if exit != Exit::OutOfSteps {
            break;
        }
    }
    stdout.flush().map_err(|e| format!("stdout: {e}"))?;

    match exit {
        Exit::Pass => Ok(ExitCode::SUCCESS),
        Exit::Fail(code) => Err(format!("guest failed with code {code}")),
        Exit::OutOfSteps => Err(format!("no exit after {} steps", run.steps)),
    }
}
