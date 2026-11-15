# T80 security review: elysium (bytecode VM and verifier)
Method: code reading only, nothing executed. Sampled: verify.rs (verify, verify_func, Cx::check), decode.rs, leb.rs, vm.rs (new, call), debug.rs start, exec.rs push_call_frame, exec_obj.rs exec_new_fill, heap.rs alloc. Not read in depth: exec_ops.rs, value.rs.
HIGH: none. MEDIUM: none.
LOW
- exec.rs:291 (push_call_frame): reg_stack grows by callee.regs per frame and is not counted in max_heap_bytes. Input: module with a function of 4096 registers (module.rs:13 MAX_REGS) recursing to Limits max_depth 1024. Result: about 64 MiB at default limits (1024 x 4096 x 16-byte Value, size inferred, not measured). Fix: charge reg_stack growth to the heap budget or cap its total length.
No finding: Vm::new verifies before any run (vm.rs:66-68); host args and call arity are checked (debug.rs:181-184, exec.rs:282); GetField and NewVariant indexes are checked at runtime (exec_obj.rs:128); decode counts are bounded by remaining input (leb.rs:110-113); NewFill checks fuel and heap before allocating (exec_obj.rs:78-91); verifier covers registers, spans, jump targets, call arity, constants and last-instruction fall-through.
