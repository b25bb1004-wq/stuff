//! Elysium MBC1 bytecode verifier, disassembler, and VM runner demo.

use std::fmt::Write as _;

use elysium::{Heap, Host, Limits, Module, Obj, Trap, Value, Vm};
use logos::line_col;

use super::{check_input, Demo, Output};

const RUN_FUEL: u64 = 1_000_000;
const MAX_HEAP_BYTES: usize = 4 * 1024 * 1024;
const MAX_CALL_DEPTH: u32 = 256;
const MAX_DISASM_OPS: usize = 512;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

const DEFAULT_LOGOS: &str = "\
fn print(s: str) uses io = extern \"io.print\"

fn fact(n: int) -> int {
  if n <= 1 { 1 } else { n * fact(n - 1) }
}

fn main() uses io {
  let r = fact(6)
  print(\"6! = \" + str(r) + \"\\n\")
}";

/// Registered Elysium bytecode VM demo.
pub const DEMO: Demo = Demo {
    id: "elysium",
    title: "Elysium VM",
    default_input: DEFAULT_LOGOS,
    run,
};

struct DemoHost {
    output: String,
    rng_state: u64,
}

impl DemoHost {
    fn new() -> Self {
        Self {
            output: String::new(),
            rng_state: 1,
        }
    }
}

impl Host for DemoHost {
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap> {
        match name {
            "io.print" => {
                if let Some(Value::Ref(h)) = args.first().copied() {
                    if let Ok(Obj::Str(s)) = heap.get(h) {
                        if self.output.len() < MAX_OUTPUT_BYTES {
                            let avail = MAX_OUTPUT_BYTES - self.output.len();
                            let take = s.floor_char_boundary(avail);
                            self.output.push_str(&s[..take]);
                        }
                    }
                }
                Ok(Value::Unit)
            }
            "io.read_line" => {
                let h = heap.alloc(Obj::Variant {
                    tag: 0,
                    fields: Vec::new(),
                })?;
                Ok(Value::Ref(h))
            }
            "time.now" => Ok(Value::Int(0)),
            "rand.next" => {
                self.rng_state = self.rng_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = self.rng_state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                Ok(Value::Int((z ^ (z >> 31)) as i64))
            }
            other => Err(Trap::Host(String::from(other))),
        }
    }
}

/// Compile Logos source via Cobalt, verify MBC1 bytecode, disassemble, and run on Elysium.
pub fn run(input: &str) -> Result<Output, String> {
    check_input(input)?;
    if input.trim().is_empty() {
        return Err(String::from("empty Logos source"));
    }

    let ast = logos::parser::parse(input).map_err(|d| format_diag(input, "parse", &d))?;
    let prog = logos::check::check(&ast).map_err(|d| format_diag(input, "check", &d))?;
    let compiled = cobalt::compile(&prog);

    elysium::verify(&compiled.module).map_err(|e| format!("verify error: {e:?}"))?;
    let encoded = elysium::encode(&compiled.module);
    let module = elysium::decode(&encoded).map_err(|e| format!("decode error: {e:?}"))?;

    let mut out = String::new();
    format_disasm(&mut out, &module, encoded.len());

    let host_names = ["io.print", "io.read_line", "time.now", "rand.next"];
    let mut vm = Vm::new(module, &host_names).map_err(|e| format!("vm load error: {e:?}"))?;
    let mut host = DemoHost::new();
    let limits = Limits {
        fuel: RUN_FUEL,
        max_heap_bytes: MAX_HEAP_BYTES,
        max_depth: MAX_CALL_DEPTH,
    };

    out.push_str("\n=== vm run ===\n");
    if compiled.main.is_none() {
        out.push_str("status: ok (no main function)\nsteps: 0\n");
    } else {
        match vm.run(&mut host, limits) {
            Ok(val) => {
                let _ = writeln!(out, "status: ok ({val:?})");
            }
            Err(trap) => {
                if let Some((f_idx, op_idx)) = vm.trap_site() {
                    let _ = writeln!(out, "status: trap {trap:?} at fn#{f_idx} op#{op_idx}");
                } else {
                    let _ = writeln!(out, "status: trap {trap:?}");
                }
            }
        }
        let _ = writeln!(out, "steps: {}", vm.fuel_used());
    }
    if !host.output.is_empty() {
        let _ = writeln!(out, "output:\n{}", host.output.trim_end_matches('\n'));
    }

    Ok(Output::Text(out))
}

fn format_diag(src: &str, stage: &str, d: &logos::Diag) -> String {
    let (line, col) = line_col(src, d.span.start);
    format!("{stage} error at {line}:{col}: {}", d.message)
}

fn format_disasm(out: &mut String, module: &Module, byte_len: usize) {
    let _ = writeln!(
        out,
        "MBC1 verified: {byte_len} bytes, {} consts, {} imports, {} funcs, entry={}",
        module.consts.len(),
        module.imports.len(),
        module.funcs.len(),
        module.entry
    );
    for (idx, c) in module.consts.iter().enumerate().take(64) {
        let _ = writeln!(out, "  const k{idx}: {c:?}");
    }
    for (idx, imp) in module.imports.iter().enumerate().take(64) {
        let _ = writeln!(out, "  import h{idx}: {imp}");
    }
    let mut shown = 0usize;
    for (f_idx, func) in module.funcs.iter().enumerate() {
        let _ = writeln!(
            out,
            "fn#{f_idx} {}(params={}, regs={}, ops={}):",
            func.name,
            func.params,
            func.regs,
            func.code.len()
        );
        for (pc, op) in func.code.iter().enumerate() {
            if shown >= MAX_DISASM_OPS {
                out.push_str("  ... (disassembly truncated)\n");
                return;
            }
            let _ = writeln!(out, "  {pc:04}: {op:?}");
            shown += 1;
        }
    }
}
