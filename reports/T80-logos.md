# T80 security review: logos (language front end)
Method: code reading only, nothing executed. Sampled: parser/mod.rs depth guard (19, 153-165), parser/exprs.rs:72 and 256, check/lower.rs:82-90, check/names.rs:293-299, check/types.rs:316-331 (Expr::Repeat), eval/vm.rs:173 and 318-331 (NewFill), eval/mod.rs run_main fuel. Not read in depth: check/types.rs, check/layers.rs, print.rs.
HIGH
- eval/vm.rs:330 (Inst::NewFill): vec![val; n] with n taken from program data. The checker only type-checks the count (check/types.rs:316-331), with no bound. Input: a list repeat such as [0; 4000000000] in main. Result: tens of GB allocation, abort on failure. Fuel does not cover it, since it is one instruction. Fix: cap the element count (e.g. 1 << 20) and charge count x size to a heap budget before allocating.
MEDIUM: none. LOW: none
No finding: parser recursion is depth-guarded (parser/mod.rs:19, exprs.rs:72); lowering is depth-guarded (lower.rs:82-83); constant cycles and nesting are guarded (names.rs:293-299); non-test code has no unwrap, expect or panic.
