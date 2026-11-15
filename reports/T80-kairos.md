# T80 security review: kairos (read-only; sampled; nothing executed)
Read: pool.rs MAX_THREADS (29) and Pool::new clamp (88-89), bin/kairos.rs read_input (40-50) and capacities (160-162), lib.rs forbid(unsafe_code); grep of src for process, network and unsafe APIs.
HIGH: none. MEDIUM: none. LOW: none.
No finding (sampled): worker count is clamped to 1..=64 (pool.rs:29, 89); the CLI reads a task file or stdin with no network or process use; vectors are sized from the already-parsed task count (bin/kairos.rs:160-162).
Not reviewed in depth: scheduler.rs (460 lines) and the test-only paths; time arithmetic in scheduling was not checked for overflow.
