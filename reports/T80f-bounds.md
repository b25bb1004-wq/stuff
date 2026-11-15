1. chisel elf.rs read_symbols: caps at 1,048,576 symbols and 16 MiB of name bytes (NULs included); each name scan stops at the bytes left in the cap; over cap is ElfError::Unsupported.
2. chisel elf.rs read: caps at 256 MiB of PT_LOAD file bytes summed over all segments and 4096 program headers; the bounds check runs first, so an out-of-file p_filesz still gives Truncated.
3. chisel expr.rs: depth 256 in parse_unary/parse_primary; also 256 operators per expression, since a long binary chain builds a deep AST that eval recurses over (added beyond the list).
4. chisel macros.rs: 64 MiB of substituted bytes per top-level invocation across all nesting levels; over cap is AsmError "macro expansion too large".
5. logos eval/vm.rs NewFill: a count over 16M elements traps with Trap::Overflow before any allocation; no new Trap variant, because athena and neon match on Trap exhaustively.
6. neon: new src/guest_alloc.rs (safe, host-built): neon_alloc uses try_reserve_exact and returns null on failure; free and copy accept only live (ptr, len) pairs; an unknown copy gives an empty slice, so titan_boot reports the ELF error instead of "invalid ELF pointer".
7. Tests, one per item: elf 4, expr 2, macros 1, logos 1, guest_alloc 2; no existing test changed (the elf reorder keeps leader_read_errors' Truncated).
8. Gates: chisel, logos, neon CHECK PASS on host; wasm32 cargo check OK; wasm32 clippy flags neon_free's raw-pointer pattern, already at HEAD and outside the gate. Not done: elf write() panics (LOW). Nothing committed.
