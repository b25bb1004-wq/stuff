# chisel

`chisel` is a two-pass RV64IMAC assembler and ELF64 helper library in Rust with no external runtime dependencies beyond `quartz`. It turns RISC-V assembly source text into flat machine code bytes and symbol tables matching LLVM 19 encoding, and provides an in-memory ELF64 writer, reader, and loader for running assembled programs on a `quartz` machine.

## Features

- Two-pass assembly of RV64I, M, A, and compressed C instructions, plus CSR and system instructions.
- Pseudo-instructions such as `li` and branch aliases, along with named labels and numeric local labels (`1:`, `1f`, `1b`).
- Constant definitions via `.equ` and `.set`, and arithmetic expressions evaluated at assembly time.
- Data and alignment directives: `.byte`, `.half`/`.2byte`, `.word`/`.4byte`/`.long`, `.dword`/`.8byte`/`.quad`, `.zero`, `.space`, `.ascii`, `.asciz`/`.string`, `.balign`, `.p2align`, and `.align`.
- Little-endian RV64 ELF64 serialization (`elf::write`), parsing (`elf::read`), and loading into `quartz::machine::Machine` (`elf::load`) with `PT_LOAD` segments and symbol tables.
- Line-numbered error reporting via `AsmError` and structured ELF validation errors via `ElfError`.

## Public API

| Item | Description |
| --- | --- |
| `assemble(src, base)` | Assemble RV64IMAC source text into `Result<Output, AsmError>` starting at `base` |
| `Output` | Assembled `bytes: Vec<u8>` and `symbols: Vec<(String, u64)>` |
| `AsmError` | Assembly error with 1-based `line` and `msg` |
| `elf::Image` | In-memory RV64 ELF64 image (`entry`, `segments`, `symbols`) |
| `elf::Segment` | Loadable `PT_LOAD` segment (`vaddr`, `flags`, `data`, `mem_size`) |
| `elf::PF_R`, `elf::PF_W`, `elf::PF_X` | Segment permission flags |
| `elf::ElfError` | ELF64 parsing error (`Truncated`, `BadMagic`, `Unsupported`, `BadSegment`) |
| `elf::write(img)` | Serialize an `Image` to little-endian RV64 ELF64 bytes |
| `elf::read(bytes)` | Parse little-endian RV64 ELF64 bytes into `Result<Image, ElfError>` |
| `elf::load(img, m)` | Load `Image` segments into a `quartz::machine::Machine` and set `pc` |

## Demo

```sh
cargo run -p chisel --example demo
```

```text
assembling RV64IMAC program at 0x10000
assembled 28 bytes
symbol _start = 0x10000
symbol loop = 0x10008
symbol data = 0x10018
expected error on bad input: line 2: unknown mnemonic 'badinst'
wrote ELF64 image (4528 bytes)
parsed ELF64: entry=0x10000, segments=1, symbols=3
ELF64 round-trip verified
```

## Tests

```sh
cargo test -p chisel
```
