//! RV64IM assembly emitter for the validated Mica IR integer subset.

use super::runtime::emit_runtime;
use alloc::string::String;
use core::fmt::Write;
use mica::ast::{BinOp, UnOp};
use mica::ir::{Body, Func, FuncId, Inst, Lit, Program, Reg};

const MAX_CALL_DEPTH: u32 = 10_000;

/// Emit full Chisel RV64IM assembly for `p` starting at `entry_id`.
pub(super) fn emit_program(p: &Program, entry_id: FuncId) -> String {
    let mut cx = EmitCx {
        asm: String::with_capacity(4096),
        uid: 0,
    };
    emit_runtime(&mut cx.asm, entry_id);
    for (fid, func) in p.funcs.iter().enumerate() {
        cx.emit_func(fid as u32, func);
    }
    let _ = writeln!(cx.asm, ".balign 16\n_stack_limit:");
    cx.asm
}

struct EmitCx {
    asm: String,
    uid: u64,
}

impl EmitCx {
    fn next_id(&mut self) -> u64 {
        let id = self.uid;
        self.uid += 1;
        id
    }

    fn emit_func(&mut self, fid: FuncId, f: &Func) {
        let num_regs = f.regs.len();
        let raw_bytes = 16usize.saturating_add(num_regs.saturating_mul(8));
        let frame_size = (raw_bytes + 15) & !15;

        let _ = writeln!(self.asm, "\nfn_{fid}:");
        // Depth check (s11 holds current call depth, incremented on entry).
        let _ = writeln!(self.asm, "\taddi s11, s11, 1");
        let _ = writeln!(self.asm, "\tli t0, {MAX_CALL_DEPTH}");
        let ok_depth = self.next_id();
        let _ = writeln!(self.asm, "\tble s11, t0, .Lok_{ok_depth}");
        let _ = writeln!(self.asm, "\ttail _trap_stack_overflow");
        let _ = writeln!(self.asm, ".Lok_{ok_depth}:");

        // Stack pointer & image boundary check.
        if frame_size <= 2048 {
            let _ = writeln!(self.asm, "\taddi t1, sp, -{frame_size}");
        } else {
            let _ = writeln!(self.asm, "\tli t0, {frame_size}");
            let _ = writeln!(self.asm, "\tsub t1, sp, t0");
        }
        let _ = writeln!(self.asm, "\tla t2, _stack_limit");
        let ok_sp = self.next_id();
        let _ = writeln!(self.asm, "\tbgeu t1, t2, .Lok_{ok_sp}");
        let _ = writeln!(self.asm, "\ttail _trap_stack_overflow");
        let _ = writeln!(self.asm, ".Lok_{ok_sp}:");

        let _ = writeln!(self.asm, "\tsd ra, -8(sp)");
        let _ = writeln!(self.asm, "\tsd s0, -16(sp)");
        let _ = writeln!(self.asm, "\tmv s0, sp");
        let _ = writeln!(self.asm, "\tmv sp, t1");

        // Save incoming parameters a0..a{params-1} into slots 0..params-1.
        for i in 0..f.params {
            self.store_reg(&format!("a{i}"), i);
        }

        if let Body::Code { insts, .. } = &f.body {
            for inst in insts {
                self.emit_inst(fid, inst);
            }
        }

        // Safe fallthrough epilogue if control flows past the end of `insts`.
        let _ = writeln!(self.asm, "\tli a0, 0");
        let _ = writeln!(self.asm, ".Lfn_{fid}_ret:");
        let _ = writeln!(self.asm, "\tmv sp, s0");
        let _ = writeln!(self.asm, "\tld ra, -8(s0)");
        let _ = writeln!(self.asm, "\tld s0, -16(s0)");
        let _ = writeln!(self.asm, "\taddi s11, s11, -1");
        let _ = writeln!(self.asm, "\tret");
    }

    fn emit_inst(&mut self, fid: FuncId, inst: &Inst) {
        match inst {
            Inst::Lit(d, lit) => match lit {
                Lit::Int(v) => {
                    let _ = writeln!(self.asm, "\tli t0, {v}");
                    self.store_reg("t0", *d);
                }
                Lit::Bool(true) => {
                    let _ = writeln!(self.asm, "\tli t0, 1");
                    self.store_reg("t0", *d);
                }
                Lit::Bool(false) | Lit::Unit => {
                    self.store_reg("zero", *d);
                }
                Lit::Byte(_) | Lit::Str(_) | Lit::Fn(_) => {}
            },
            Inst::Move(d, s) => {
                if d != s {
                    self.load_reg("t0", *s);
                    self.store_reg("t0", *d);
                }
            }
            Inst::Un(d, op, s) => {
                self.load_reg("t0", *s);
                match op {
                    UnOp::Neg => {
                        let _ = writeln!(self.asm, "\tneg t2, t0");
                        let _ = writeln!(self.asm, "\tand t1, t0, t2");
                        let ok = self.next_id();
                        let _ = writeln!(self.asm, "\tbgez t1, .Lok_{ok}");
                        let _ = writeln!(self.asm, "\ttail _trap_overflow");
                        let _ = writeln!(self.asm, ".Lok_{ok}:");
                    }
                    UnOp::Not => {
                        let _ = writeln!(self.asm, "\tseqz t2, t0");
                    }
                }
                self.store_reg("t2", *d);
            }
            Inst::Bin(d, op, a, b) => self.emit_bin(*d, *op, *a, *b),
            Inst::Call(d, callee, args) => {
                for (i, &arg) in args.iter().enumerate() {
                    self.load_reg(&format!("a{i}"), arg);
                }
                let _ = writeln!(self.asm, "\tcall fn_{callee}");
                self.store_reg("a0", *d);
            }
            Inst::Label(lbl) => {
                let _ = writeln!(self.asm, ".Lfn_{fid}_l_{lbl}:");
            }
            Inst::Jump(lbl) => {
                let _ = writeln!(self.asm, "\tj .Lfn_{fid}_l_{lbl}");
            }
            Inst::Branch(cond, t_lbl, f_lbl) => {
                self.load_reg("t0", *cond);
                let br = self.next_id();
                let _ = writeln!(self.asm, "\tbeqz t0, .Lbr_{br}");
                let _ = writeln!(self.asm, "\tj .Lfn_{fid}_l_{t_lbl}");
                let _ = writeln!(self.asm, ".Lbr_{br}:");
                let _ = writeln!(self.asm, "\tj .Lfn_{fid}_l_{f_lbl}");
            }
            Inst::Ret(r) => {
                self.load_reg("a0", *r);
                let _ = writeln!(self.asm, "\tj .Lfn_{fid}_ret");
            }
            Inst::Assert(cond, _) => {
                self.load_reg("t0", *cond);
                let ok = self.next_id();
                let _ = writeln!(self.asm, "\tbnez t0, .Lok_{ok}");
                let _ = writeln!(self.asm, "\ttail _trap_assert");
                let _ = writeln!(self.asm, ".Lok_{ok}:");
            }
            _ => {}
        }
    }

    fn emit_bin(&mut self, d: Reg, op: BinOp, a: Reg, b: Reg) {
        self.load_reg("t0", a);
        self.load_reg("t1", b);
        match op {
            BinOp::Add => {
                let _ = writeln!(self.asm, "\tadd t2, t0, t1");
                let _ = writeln!(self.asm, "\tsltz t3, t1");
                let _ = writeln!(self.asm, "\tslt t4, t2, t0");
                let ok = self.next_id();
                let _ = writeln!(self.asm, "\tbeq t3, t4, .Lok_{ok}");
                let _ = writeln!(self.asm, "\ttail _trap_overflow");
                let _ = writeln!(self.asm, ".Lok_{ok}:");
            }
            BinOp::Sub => {
                let _ = writeln!(self.asm, "\tsub t2, t0, t1");
                let _ = writeln!(self.asm, "\tsltz t3, t1");
                let _ = writeln!(self.asm, "\tslt t4, t0, t2");
                let ok = self.next_id();
                let _ = writeln!(self.asm, "\tbeq t3, t4, .Lok_{ok}");
                let _ = writeln!(self.asm, "\ttail _trap_overflow");
                let _ = writeln!(self.asm, ".Lok_{ok}:");
            }
            BinOp::Mul => {
                let _ = writeln!(self.asm, "\tmul t2, t0, t1");
                let _ = writeln!(self.asm, "\tmulh t3, t0, t1");
                let _ = writeln!(self.asm, "\tsrai t4, t2, 63");
                let ok = self.next_id();
                let _ = writeln!(self.asm, "\tbeq t3, t4, .Lok_{ok}");
                let _ = writeln!(self.asm, "\ttail _trap_overflow");
                let _ = writeln!(self.asm, ".Lok_{ok}:");
            }
            BinOp::Div | BinOp::Rem => {
                let nz = self.next_id();
                let _ = writeln!(self.asm, "\tbnez t1, .Lok_{nz}");
                let _ = writeln!(self.asm, "\ttail _trap_div_zero");
                let _ = writeln!(self.asm, ".Lok_{nz}:");
                let ok = self.next_id();
                let _ = writeln!(self.asm, "\tli t2, -1");
                let _ = writeln!(self.asm, "\tbne t1, t2, .Lok_{ok}");
                let _ = writeln!(self.asm, "\tli t2, 1");
                let _ = writeln!(self.asm, "\tslli t2, t2, 63");
                let _ = writeln!(self.asm, "\tbne t0, t2, .Lok_{ok}");
                let _ = writeln!(self.asm, "\ttail _trap_overflow");
                let _ = writeln!(self.asm, ".Lok_{ok}:");
                let mnem = if op == BinOp::Div { "div" } else { "rem" };
                let _ = writeln!(self.asm, "\t{mnem} t2, t0, t1");
            }
            BinOp::Eq => {
                let _ = writeln!(self.asm, "\tsub t2, t0, t1");
                let _ = writeln!(self.asm, "\tseqz t2, t2");
            }
            BinOp::Ne => {
                let _ = writeln!(self.asm, "\tsub t2, t0, t1");
                let _ = writeln!(self.asm, "\tsnez t2, t2");
            }
            BinOp::Lt => {
                let _ = writeln!(self.asm, "\tslt t2, t0, t1");
            }
            BinOp::Le => {
                let _ = writeln!(self.asm, "\tslt t2, t1, t0");
                let _ = writeln!(self.asm, "\txori t2, t2, 1");
            }
            BinOp::Gt => {
                let _ = writeln!(self.asm, "\tslt t2, t1, t0");
            }
            BinOp::Ge => {
                let _ = writeln!(self.asm, "\tslt t2, t0, t1");
                let _ = writeln!(self.asm, "\txori t2, t2, 1");
            }
            BinOp::And => {
                let _ = writeln!(self.asm, "\tsnez t0, t0");
                let _ = writeln!(self.asm, "\tsnez t1, t1");
                let _ = writeln!(self.asm, "\tand t2, t0, t1");
            }
            BinOp::Or => {
                let _ = writeln!(self.asm, "\tor t2, t0, t1");
                let _ = writeln!(self.asm, "\tsnez t2, t2");
            }
            _ => {}
        }
        self.store_reg("t2", d);
    }

    fn load_reg(&mut self, dst: &str, r: Reg) {
        let off = slot_offset(r);
        if off >= -2048 {
            let _ = writeln!(self.asm, "\tld {dst}, {off}(s0)");
        } else {
            let _ = writeln!(self.asm, "\tli t6, {off}");
            let _ = writeln!(self.asm, "\tadd t6, s0, t6");
            let _ = writeln!(self.asm, "\tld {dst}, 0(t6)");
        }
    }

    fn store_reg(&mut self, src: &str, r: Reg) {
        let off = slot_offset(r);
        if off >= -2048 {
            let _ = writeln!(self.asm, "\tsd {src}, {off}(s0)");
        } else {
            let _ = writeln!(self.asm, "\tli t6, {off}");
            let _ = writeln!(self.asm, "\tadd t6, s0, t6");
            let _ = writeln!(self.asm, "\tsd {src}, 0(t6)");
        }
    }
}

fn slot_offset(r: Reg) -> i64 {
    -24 - 8 * i64::from(r)
}

use alloc::format;
