# T80 security review: chisel (assembler and ELF)
Method: code reading only, nothing executed. Sampled: elf.rs read/load/write, expr.rs, macros.rs, asm.rs output cap, directives.rs .zero/.space. Not read in depth: parse_rv, encode_*, lexer.rs.
HIGH
- elf.rs:386-393 (read_symbols): each symbol clones its name up to the next NUL, with no total cap. Input: symtab of ~40k entries (24 B each, ~1 MB), all st_name=0 and st_info=0x12, plus a 1 MB strtab of letters ending in NUL. Result: ~40 GB of String allocations and ~4e10 byte scans. Fix: cap total name bytes or store (offset,len) instead of owned Strings.
- elf.rs:297 (read): each PT_LOAD copies p_filesz bytes with to_vec(), no total cap. Input: 65535 PT_LOAD headers, all p_offset=0, p_filesz=p_memsz=~3.7 MB (file size), disjoint vaddrs. Result: ~240 GB of copies. Fix: keep (offset,len) into the input, or cap the sum of filesz at the input length.
- expr.rs:148 (parse_unary) and expr.rs:166 (parse_primary via parse_add_sub): unbounded recursion. Input: one source line of 100000 minus signs or open parens. Result: stack overflow abort (Windows main thread is 1 MiB). Fix: depth counter that returns AsmError at about 256.
- macros.rs:109-113 and 142: caps are on statement count (1M) and per-line length (1 MiB) only; total bytes are uncapped. Input: a macro that nests 2 levels with fan-out 1000 and passes a 1 KiB argument gives ~1 GB of text; a 1 MiB argument gives ~1 TB. Fix: keep a running byte total next to out.len() and cap it (e.g. 64 MiB).
LOW
- elf.rs:318-322 (read overlap check): O(n^2) over up to 65535 PT_LOAD headers, ~2e9 comparisons. Fix: sort ranges and compare neighbours.
- elf.rs:81-107 (write): assert and expect panic on an Image. Only host code calls write (cobalt, titan build.rs), so low. Project rule says library code must not panic. Fix: return Result.
No finding: directives.rs:238 and expr_eval.rs:152 unreachable arms sit behind outer matches; asm.rs:126 caps total output at 256 MiB and breaks.
