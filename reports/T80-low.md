# T80g: LOW fixes (sphinx, pigment, calliope, elysium, zeus, titan, aurum)
- sphinx HIGH fill_random (sphinx.rs:124-135): not fixed, outside the LOW scope; non-CSPRNG keygen fallback on win32 is the top item for the operator.
- sphinx L1 rng.rs:27-31 counter wrap repeats keystream: FIXED. refill now fails closed (expect) before the 2^32-block wrap; tests last_block_before_wrap_is_served, counter_wrap_fails_closed.
- sphinx L2 medusa.rs:68-80 ctr_xor counter wrap: SKIPPED. tests/cinnabar_leader.rs:150-167 pins wrapping_add semantics in the reference model; a guard needs a test-contract change (operator decision).
- sphinx L3 sealed.rs:21-27 silent empty Vec for low-order recipient: SKIPPED. API footgun, not a panic, alloc, overflow or compare defect.
- pigment L1 canvas.rs:46-60 0x0 fallback on allocation failure: SKIPPED. Documented behaviour; changing to Result breaks callers in helios, iris, odyssey, indium (outside allowed files).
- calliope L1 regex/ast.rs parse_group unbounded recursion: FIXED. MAX_GROUP_DEPTH = 256 returns RegexError; test regex_deep_nesting_is_rejected_not_overflowed.
- elysium L1 exec.rs push_call_frame reg_stack not charged to heap budget: FIXED. Register stack counts against max_heap_bytes and traps StackOverflow; test register_stack_growth_is_charged_to_heap_budget.
- zeus L1 boot.rs:241 guest RAM mapped RWX: SKIPPED. Page-permission policy, not a listed defect class; touches hypervisor boot path (guest-visible change).
- aurum L1 bin/aurum.rs:335-356 win32 keygen seed not a CSPRNG: SKIPPED. Not a listed defect class; the fix needs OS-RNG FFI (unsafe) in a zero-dependency crate. Serious: operator decision needed.
- titan L1 elf.rs:167 O(n^2) PT_LOAD overlap check (n <= 65535): SKIPPED. Quadratic time with a bounded n, not a listed defect class.
Gates: CHECK PASS calliope, elysium, sphinx. Nothing committed.
