//! Cobalt Logos IR optimizer, compiler, and bytecode runner demo.

use std::fmt::Write as _;

use logos::ir::{Body, Program};
use logos::line_col;

use super::{check_input, Demo, Output};

const RUN_FUEL: u64 = 1_000_000;
const MAX_IR_INSTS: usize = 256;

const DEFAULT_LOGOS: &str = "\
fn print(s: str) uses io = extern \"io.print\"

fn main() uses io {
  let a = 2 + 3 * 4
  let b = a
  let unused = 99 - 1
  let sum = b + 28
  print(\"answer = \" + str(sum) + \"\\n\")
}";

/// Registered Cobalt compiler and optimizer demo.
pub const DEMO: Demo = Demo {
    id: "cobalt",
    title: "Cobalt Compiler",
    default_input: DEFAULT_LOGOS,
    run,
};

/// Parse, type-check, optimize IR, compile to MBC1 bytecode, and execute `main`.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    if input.trim().is_empty() {
        return Err(String::from("empty Logos source"));
    }

    let ast = logos::parser::parse(input).map_err(|d| format_diag(input, "parse", &d))?;
    let mut prog = logos::check::check(&ast).map_err(|d| format_diag(input, "check", &d))?;

    let mut out = String::new();
    out.push_str("=== IR before optimize ===\n");
    format_program(&mut out, &prog);

    let stats = cobalt::opt::optimize(&mut prog);
    let _ = writeln!(
        out,
        "\n=== optimize stats ===\nfolded={}, copies={}, removed={}",
        stats.folded, stats.copies, stats.removed
    );

    out.push_str("\n=== IR after optimize ===\n");
    format_program(&mut out, &prog);

    let compiled = cobalt::compile(&prog);
    let bytecode = elysium::encode(&compiled.module);
    let _ = writeln!(
        out,
        "\n=== bytecode ===\nsize: {} bytes ({} funcs, {} consts, {} imports)",
        bytecode.len(),
        compiled.module.funcs.len(),
        compiled.module.consts.len(),
        compiled.module.imports.len()
    );

    let outcome = cobalt::run_main(&compiled, "", RUN_FUEL);
    out.push_str("\n=== run_main ===\n");
    if compiled.main.is_none() {
        out.push_str("status: ok (no main function)\n");
    } else {
        match outcome.result {
            Ok(()) => out.push_str("status: ok\n"),
            Err((trap, span)) => {
                let (line, col) = line_col(input, span.start);
                let _ = writeln!(out, "status: trap {trap:?} at {line}:{col}");
            }
        }
    }
    if !outcome.output.is_empty() {
        let _ = writeln!(out, "output:\n{}", outcome.output.trim_end_matches('\n'));
    }

    Ok(Output::Text(out))
}

fn format_diag(src: &str, stage: &str, d: &logos::Diag) -> String {
    let (line, col) = line_col(src, d.span.start);
    format!("{stage} error at {line}:{col}: {}", d.message)
}

fn format_program(out: &mut String, prog: &Program) {
    let mut shown = 0usize;
    for (idx, func) in prog.funcs.iter().enumerate() {
        match &func.body {
            Body::Extern(ext) => {
                let _ = writeln!(out, "fn#{idx} {} -> extern \"{ext}\"", func.name);
            }
            Body::Code { insts, .. } => {
                let _ = writeln!(
                    out,
                    "fn#{idx} {}(params={}, regs={}, insts={}):",
                    func.name,
                    func.params,
                    func.regs.len(),
                    insts.len()
                );
                for (pc, inst) in insts.iter().enumerate() {
                    if shown >= MAX_IR_INSTS {
                        out.push_str("  ... (IR listing truncated)\n");
                        return;
                    }
                    let _ = writeln!(out, "  {pc:03}: {inst:?}");
                    shown += 1;
                }
            }
        }
    }
}
