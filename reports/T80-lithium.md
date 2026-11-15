# T80 security review: lithium
Method: code reading only, nothing executed. Sampled: src/lib.rs public fns (image, boot, run), unsafe and panic grep across src, grep for file, stdin or network input in bin/lithium.rs. Not read in depth: bin/lithium.rs, tests.
HIGH: none. MEDIUM: none. LOW: none.
No finding: both crate roots are forbid(unsafe_code) (lib.rs:2, bin/lithium.rs:2); zero non-test unwrap, expect or panic sites; no fs::read in bin/lithium.rs, so no untrusted file input found at grep level.
