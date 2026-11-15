# T80 security review: calliope (read-only; nothing executed)
Read: regex/ast.rs parse_repeat_count (387-391) and parse_group (218), regex/compile.rs caps (8-9, 64), buffer.rs slice (70-80), lib.rs forbid(unsafe_code).
HIGH: none. MEDIUM: none.
LOW L1 regex/ast.rs:218 (parse_group, with parse_alt and parse_concat): recursion on nested groups has no depth cap. Input: a search pattern of 100k '(' characters (typed or pasted into the search prompt) recurses until the stack overflows, which aborts the process; the Windows main-thread stack is 1 MiB and frame size was not measured. Fix: add a nesting limit (e.g., 256) that returns RegexError.
No finding (sampled): repeat counts capped at 1000 (ast.rs:387-391); compiled program capped at 250,000 instructions (compile.rs:8) and loop depth at 64 (compile.rs:9); regex VM is a thread list with fixed-size generation arrays (vm.rs:27-30), so matching is linear in input; buffer.rs:70-80 asserts range and char boundaries (panics on bad callers; no typed-input path to it seen).
Not reviewed in depth: app.rs, command.rs, normal.rs, keys.rs (key and command input paths), undo.rs.
