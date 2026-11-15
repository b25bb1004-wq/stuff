# T80 security review: cobalt (compiler)
Method: code reading only, nothing executed. MOVING TARGET: cobalt is being edited by another worker right now; this reflects the tree at review time.
Coverage: grep-level (panic sites, unsafe, recursion, depth guards) plus the logos guards it depends on. Not read in full: compile.rs, run.rs, rv64/emit.rs, rv64/validate.rs.
HIGH: none. MEDIUM: none. LOW: none found in sampled code.
No finding: unsafe is forbidden in all roots (lib.rs:3, bin/cobalt.rs:2, main.rs:2); the only non-test panic-like site is the usage path (bin/cobalt.rs:126); source goes through logos::parser::parse (bin/cobalt.rs:190), which has a depth limit of 200 (logos parser/mod.rs:19); interpreter call depth is capped (run.rs:245); rv64/mod.rs:67 calls chisel::elf::write only on compiler-built images.
Not verified: whether cobalt evaluates logos IR. If it does, it inherits the unbounded NewFill issue in logos eval/vm.rs:330.
