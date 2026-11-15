//! Start-up stub, decimal UART printer, and trap handlers for Quartz RV64.

use core::fmt::Write;
use mica::ir::FuncId;

/// Emit `_start` and shared `_trap_*` handlers into `asm`.
pub(super) fn emit_runtime(asm: &mut alloc::string::String, entry_id: FuncId) {
    let _ = writeln!(
        asm,
        "_start:\n\
         \tli sp, 0x81000000\n\
         \tli s11, 0\n\
         \tcall fn_{entry_id}\n\
         \tli t5, 0x10000000\n\
         \tbgez a0, .Lprint_pos\n\
         \tli t0, 45\n\
         \tsb t0, 0(t5)\n\
         \tneg a0, a0\n\
         .Lprint_pos:\n\
         \taddi sp, sp, -32\n\
         \taddi t1, sp, 32\n\
         \tli t2, 10\n\
         .Lprint_loop:\n\
         \tremu t3, a0, t2\n\
         \tdivu a0, a0, t2\n\
         \taddi t3, t3, 48\n\
         \taddi t1, t1, -1\n\
         \tsb t3, 0(t1)\n\
         \tbnez a0, .Lprint_loop\n\
         \taddi t4, sp, 32\n\
         .Lprint_emit:\n\
         \tlbu t3, 0(t1)\n\
         \tsb t3, 0(t5)\n\
         \taddi t1, t1, 1\n\
         \tbltu t1, t4, .Lprint_emit\n\
         \taddi sp, sp, 32\n\
         \tli t0, 10\n\
         \tsb t0, 0(t5)\n\
         \tli t5, 0x00100000\n\
         \tli t0, 0x5555\n\
         \tsw t0, 0(t5)\n\
         .Lhalt:\n\
         \tj .Lhalt\n\
         \n\
         _trap_overflow:\n\
         \tli a0, 1\n\
         \tj _trap_common\n\
         _trap_div_zero:\n\
         \tli a0, 2\n\
         \tj _trap_common\n\
         _trap_assert:\n\
         \tli a0, 3\n\
         \tj _trap_common\n\
         _trap_stack_overflow:\n\
         \tli a0, 4\n\
         _trap_common:\n\
         \tli t5, 0x10000000\n\
         \tli t0, 116\n\
         \tsb t0, 0(t5)\n\
         \tli t0, 114\n\
         \tsb t0, 0(t5)\n\
         \tli t0, 97\n\
         \tsb t0, 0(t5)\n\
         \tli t0, 112\n\
         \tsb t0, 0(t5)\n\
         \tli t0, 32\n\
         \tsb t0, 0(t5)\n\
         \taddi t0, a0, 48\n\
         \tsb t0, 0(t5)\n\
         \tli t0, 10\n\
         \tsb t0, 0(t5)\n\
         \tli t5, 0x00100000\n\
         \tslli t0, a0, 16\n\
         \tli t1, 0x3333\n\
         \tor t0, t0, t1\n\
         \tsw t0, 0(t5)\n\
         .Ltrap_halt:\n\
         \tj .Ltrap_halt\n"
    );
}
